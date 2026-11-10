use std::fs;
use std::path::{Path, PathBuf};

use evdev::Device;

use crate::config::{
    default_edgepad_config_path, load_edgepad_config, DeviceConfig, EdgepadConfig,
};
use crate::core::{GestureDirection, SliderAxis, Zone};
use crate::device::{
    discover_device_report, summary_from_device, touchpad_candidates, AxisInfo, DeviceSummary,
};

mod service;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusConfig {
    pub config_path: Option<PathBuf>,
    pub device_override: Option<DeviceConfig>,
    pub input_root: PathBuf,
    pub service_name: String,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            config_path: None,
            device_override: None,
            input_root: PathBuf::from("/dev/input"),
            service_name: "edgepad.service".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSeverity {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSubject {
    Service,
    Config,
    Device,
    Size,
    Zones,
    Bindings,
}

impl StatusSubject {
    pub fn label(self) -> &'static str {
        match self {
            Self::Service => "Service",
            Self::Config => "Config",
            Self::Device => "Device",
            Self::Size => "Size",
            Self::Zones => "Zones",
            Self::Bindings => "Bindings",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub severity: StatusSeverity,
    pub subject: StatusSubject,
    pub detail: String,
}

impl StatusLine {
    fn ok(subject: StatusSubject, detail: impl Into<String>) -> Self {
        Self {
            severity: StatusSeverity::Ok,
            subject,
            detail: detail.into(),
        }
    }

    fn warn(subject: StatusSubject, detail: impl Into<String>) -> Self {
        Self {
            severity: StatusSeverity::Warn,
            subject,
            detail: detail.into(),
        }
    }

    fn fail(subject: StatusSubject, detail: impl Into<String>) -> Self {
        Self {
            severity: StatusSeverity::Fail,
            subject,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusResult {
    Running,
    NeedsAttention,
    NotRunning,
    Misconfigured,
}

impl StatusResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NeedsAttention => "needs attention",
            Self::NotRunning => "not running",
            Self::Misconfigured => "misconfigured",
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Self::Running | Self::NeedsAttention => 0,
            Self::NotRunning | Self::Misconfigured => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    pub lines: Vec<StatusLine>,
    pub result: StatusResult,
}

pub fn run_status(config: &StatusConfig) -> StatusReport {
    let mut lines = vec![service::status_line(&config.service_name)];

    match load_status_config(config) {
        Ok((config_path, edgepad_config)) => {
            lines.push(StatusLine::ok(
                StatusSubject::Config,
                config_path.display().to_string(),
            ));
            let (device_line, device) =
                device_status_line(&edgepad_config.device, &config.input_root);
            let size = PhysicalSize::from_device(device.as_ref());
            lines.push(device_line);
            lines.push(size.status_line());
            lines.push(bindings_status_line(&edgepad_config));
            lines.push(zones_status_line(&edgepad_config, size));
        }
        Err(err) => {
            lines.push(StatusLine::fail(StatusSubject::Config, err));
        }
    }

    let result = status_result(&lines);
    StatusReport { lines, result }
}

fn load_status_config(config: &StatusConfig) -> Result<(PathBuf, EdgepadConfig), String> {
    let config_path = match &config.config_path {
        Some(path) => path.clone(),
        None => default_edgepad_config_path()?,
    };

    match fs::metadata(&config_path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(format!("not a file: {}", config_path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("not found: {}", config_path.display()));
        }
        Err(err) => {
            return Err(format!(
                "failed to inspect {}: {err}",
                config_path.display()
            ))
        }
    }

    let mut edgepad_config = load_edgepad_config(&config_path)?;
    if let Some(device) = &config.device_override {
        edgepad_config.device = device.clone();
    }

    Ok((config_path, edgepad_config))
}

fn device_status_line(
    device: &DeviceConfig,
    input_root: &Path,
) -> (StatusLine, Option<DeviceSummary>) {
    let selected = match device {
        DeviceConfig::Path(path) => Device::open(path)
            .map(|device| summary_from_device(path.clone(), &device))
            .map_err(|err| {
                format!(
                    "configured path cannot be opened: {} ({err})",
                    path.display()
                )
            }),
        DeviceConfig::Auto => auto_status_device(input_root),
    };
    match selected {
        Ok(device) => (
            StatusLine::ok(
                StatusSubject::Device,
                format!("{}\n{}", device.name, device.path.display()),
            ),
            Some(device),
        ),
        Err(err) => (StatusLine::fail(StatusSubject::Device, err), None),
    }
}

fn auto_status_device(input_root: &Path) -> Result<DeviceSummary, String> {
    let discovery = discover_device_report(input_root).map_err(|err| {
        format!(
            "auto failed: failed to list {}: {err}",
            input_root.display()
        )
    })?;
    if discovery.event_node_count == 0 {
        return Err(format!(
            "auto failed: no event devices under {}",
            input_root.display()
        ));
    }
    if discovery.summaries.is_empty() {
        return Err(format!(
            "auto failed: {} event device(s) found under {}, but none were readable",
            discovery.event_node_count,
            input_root.display()
        ));
    }
    match touchpad_candidates(&discovery.summaries).as_slice() {
        [] => Err(format!(
            "auto failed: no touchpad candidates among {} readable event device(s)",
            discovery.summaries.len()
        )),
        [candidate] => Ok((*candidate).clone()),
        candidates => Err(format!(
            "auto failed: multiple touchpad candidates: {}",
            candidates
                .iter()
                .map(|candidate| candidate.path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[derive(Clone, Copy, Default)]
struct PhysicalSize {
    width: Option<f64>,
    height: Option<f64>,
}

impl PhysicalSize {
    fn from_device(device: Option<&DeviceSummary>) -> Self {
        Self {
            width: device
                .and_then(|device| device.x_range)
                .and_then(AxisInfo::length_mm),
            height: device
                .and_then(|device| device.y_range)
                .and_then(AxisInfo::length_mm),
        }
    }

    fn status_line(self) -> StatusLine {
        let detail = match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w:.1} x {h:.1} mm (width x height)"),
            (Some(w), None) => format!("width {w:.1} mm; height unavailable"),
            (None, Some(h)) => format!("width unavailable; height {h:.1} mm"),
            (None, None) => "unavailable (device dimensions not reported)".to_string(),
        };
        StatusLine::ok(StatusSubject::Size, detail)
    }
}

fn distance_label(fraction: f32, length: Option<f64>) -> String {
    match length {
        Some(mm) => format!(
            "{:.1}% / {:.1} mm",
            fraction * 100.0,
            mm * f64::from(fraction)
        ),
        None => format!("{:.1}% / unknown", fraction * 100.0),
    }
}

fn zones_status_line(config: &EdgepadConfig, size: PhysicalSize) -> StatusLine {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        return StatusLine::fail(StatusSubject::Zones, "none configured");
    }
    let widths = config.active_edge_widths();
    let mut rows = vec![format!(
        "{:<8}{:<14}{:<22}{}",
        "Edge", "Mode", "Zone width", "Slider step"
    )];
    for zone in ordered_zones() {
        let (width, length) = match zone {
            Zone::Left => (widths.left, size.width),
            Zone::Right => (widths.right, size.width),
            Zone::Top => (widths.top, size.height),
            Zone::Bottom => (widths.bottom, size.height),
        };
        let active = config.gestures.iter().any(|binding| binding.zone == zone)
            || config.sliders.iter().any(|binding| binding.zone == zone);
        let width = if active {
            distance_label(width, length)
        } else {
            "-".into()
        };
        let step = config
            .sliders
            .iter()
            .find(|slider| slider.zone == zone)
            .map(|slider| {
                let length = match slider.axis {
                    SliderAxis::Horizontal => size.width,
                    SliderAxis::Vertical => size.height,
                };
                distance_label(slider.step, length)
            })
            .unwrap_or_else(|| "-".into());
        rows.push(format!(
            "{:<8}{:<14}{:<22}{}",
            zone_name(zone),
            if active { "active" } else { "passthrough" },
            width,
            step
        ));
    }
    if size.width.is_none() || size.height.is_none() {
        rows.push(String::new());
        rows.push("unknown = physical distance unavailable".into());
    }
    StatusLine::ok(StatusSubject::Zones, rows.join("\n"))
}

fn bindings_status_line(config: &EdgepadConfig) -> StatusLine {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        return StatusLine::fail(StatusSubject::Bindings, "none configured");
    }
    let mut taps = 0;
    let mut double_taps = 0;
    let mut swipes = 0;
    for binding in &config.gestures {
        match binding.direction {
            GestureDirection::Tap => taps += 1,
            GestureDirection::DoubleTap => double_taps += 1,
            _ => swipes += 1,
        }
    }
    let counts = [
        (taps, "tap"),
        (double_taps, "double-tap"),
        (swipes, "swipe"),
        (config.sliders.len(), "slider"),
    ];
    let detail = counts
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, name)| format!("{count} {name}{}", if count == 1 { "" } else { "s" }))
        .collect::<Vec<_>>()
        .join(", ");
    StatusLine::ok(StatusSubject::Bindings, detail)
}

fn status_result(lines: &[StatusLine]) -> StatusResult {
    if lines
        .iter()
        .any(|line| line.subject != StatusSubject::Service && line.severity == StatusSeverity::Fail)
    {
        StatusResult::Misconfigured
    } else if lines
        .iter()
        .any(|line| line.subject == StatusSubject::Service && line.severity == StatusSeverity::Fail)
    {
        StatusResult::NotRunning
    } else if lines
        .iter()
        .any(|line| line.severity == StatusSeverity::Warn)
    {
        StatusResult::NeedsAttention
    } else {
        StatusResult::Running
    }
}

fn ordered_zones() -> [Zone; 4] {
    [Zone::Left, Zone::Right, Zone::Top, Zone::Bottom]
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        EdgeWidthOverrides, GestureActionConfig, GestureBindingConfig, DEFAULT_SWIPE_MIN_DISTANCE,
        DEFAULT_TAP_MIN_DURATION_MS,
    };
    use crate::core::GestureDirection;

    fn measured_device() -> DeviceSummary {
        DeviceSummary {
            path: PathBuf::from("/dev/input/event7"),
            name: "Test touchpad".into(),
            vendor: 1,
            product: 1,
            kind: crate::device::DeviceKind::Touchpad,
            slot_range: None,
            x_range: Some(AxisInfo {
                min: 100,
                max: 4900,
                resolution: 32,
            }),
            y_range: Some(AxisInfo {
                min: -100,
                max: 1900,
                resolution: 20,
            }),
        }
    }

    fn four_slider_config() -> EdgepadConfig {
        EdgepadConfig::parse(
            r#"
            edge_width = 0.10
            bottom_edge_width = 0.05
            [[sliders]]
            zone = "left"
            step = 0.04
            up = ["example", "up"]
            down = ["example", "down"]
            [[sliders]]
            zone = "right"
            step = 0.04
            up = ["example", "up"]
            down = ["example", "down"]
            [[sliders]]
            zone = "top"
            step = 0.12
            left = ["example", "left"]
            right = ["example", "right"]
            [[sliders]]
            zone = "bottom"
            step = 0.12
            left = ["example", "left"]
            right = ["example", "right"]
        "#,
        )
        .expect("valid sliders")
    }

    #[test]
    fn measurements_use_each_axis_resolution_and_configured_overrides() {
        let size = PhysicalSize::from_device(Some(&measured_device()));
        let config = four_slider_config();
        assert_eq!(size.width, Some(150.0));
        assert_eq!(size.height, Some(100.0));
        assert!(size
            .status_line()
            .detail
            .contains("150.0 x 100.0 mm (width x height)"));
        let table = zones_status_line(&config, size).detail;
        let rows: Vec<Vec<&str>> = table
            .lines()
            .skip(1)
            .take(4)
            .map(|row| row.split_whitespace().collect())
            .collect();
        assert_eq!(
            rows,
            [
                vec!["left", "active", "10.0%", "/", "15.0", "mm", "4.0%", "/", "4.0", "mm"],
                vec!["right", "active", "10.0%", "/", "15.0", "mm", "4.0%", "/", "4.0", "mm"],
                vec!["top", "active", "10.0%", "/", "10.0", "mm", "12.0%", "/", "18.0", "mm"],
                vec!["bottom", "active", "5.0%", "/", "5.0", "mm", "12.0%", "/", "18.0", "mm"],
            ]
        );
    }

    #[test]
    fn missing_resolution_keeps_percentages_and_the_other_axis_measurements() {
        let mut device = measured_device();
        device.y_range.as_mut().unwrap().resolution = 0;
        let size = PhysicalSize::from_device(Some(&device));
        let config = four_slider_config();
        assert_eq!(size.width, Some(150.0));
        assert_eq!(size.height, None);
        assert!(size.status_line().detail.contains("height unavailable"));
        let table = zones_status_line(&config, size).detail;
        assert!(table.contains("10.0% / unknown"));
        assert!(table.contains("4.0% / unknown"));
        assert!(table.contains("12.0% / 18.0 mm"));
        assert_eq!(
            zones_status_line(&config, size).severity,
            StatusSeverity::Ok
        );
    }

    #[test]
    fn missing_device_or_axis_metadata_does_not_invent_measurements() {
        let mut device = measured_device();
        device.x_range = None;
        device.y_range = None;
        for size in [
            PhysicalSize::from_device(None),
            PhysicalSize::from_device(Some(&device)),
        ] {
            assert_eq!(size.width, None);
            assert_eq!(size.height, None);
            assert!(size
                .status_line()
                .detail
                .contains("unavailable (device dimensions not reported)"));
            assert_eq!(distance_label(0.1, size.width), "10.0% / unknown");
        }
    }

    #[test]
    fn zones_status_reports_active_and_passthrough_edges() {
        let config = EdgepadConfig {
            device: DeviceConfig::Auto,
            edge_width: 0.20,
            edge_width_overrides: EdgeWidthOverrides::default(),
            tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
            swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
            gestures: vec![
                GestureBindingConfig {
                    zone: Zone::Right,
                    direction: GestureDirection::Up,
                    action: GestureActionConfig::Log,
                },
                GestureBindingConfig {
                    zone: Zone::Top,
                    direction: GestureDirection::Left,
                    action: GestureActionConfig::Log,
                },
            ],
            sliders: Vec::new(),
            ..EdgepadConfig::default()
        };

        let line = zones_status_line(&config, PhysicalSize::default());

        assert_eq!(line.severity, StatusSeverity::Ok);
        assert_eq!(line.subject, StatusSubject::Zones);
        let rows: Vec<Vec<&str>> = line
            .detail
            .lines()
            .skip(1)
            .take(4)
            .map(|row| row.split_whitespace().collect())
            .collect();
        assert_eq!(rows[0], ["left", "passthrough", "-", "-"]);
        assert_eq!(rows[1], ["right", "active", "20.0%", "/", "unknown", "-"]);
        assert_eq!(rows[2], ["top", "active", "20.0%", "/", "unknown", "-"]);
        assert_eq!(rows[3], ["bottom", "passthrough", "-", "-"]);
    }

    #[test]
    fn bindings_status_counts_gesture_types_instead_of_programs() {
        let config = EdgepadConfig {
            device: DeviceConfig::Auto,
            edge_width: 0.10,
            edge_width_overrides: EdgeWidthOverrides::default(),
            tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
            swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
            gestures: vec![
                GestureBindingConfig {
                    zone: Zone::Left,
                    direction: GestureDirection::Up,
                    action: GestureActionConfig::Command {
                        argv: vec!["pamixer".to_string(), "-i".to_string(), "5".to_string()],
                    },
                },
                GestureBindingConfig {
                    zone: Zone::Left,
                    direction: GestureDirection::Down,
                    action: GestureActionConfig::Command {
                        argv: vec!["pamixer".to_string(), "-d".to_string(), "5".to_string()],
                    },
                },
                GestureBindingConfig {
                    zone: Zone::Right,
                    direction: GestureDirection::Tap,
                    action: GestureActionConfig::Log,
                },
                GestureBindingConfig {
                    zone: Zone::Right,
                    direction: GestureDirection::DoubleTap,
                    action: GestureActionConfig::Log,
                },
            ],
            sliders: four_slider_config().sliders,
            ..EdgepadConfig::default()
        };

        let line = bindings_status_line(&config);

        assert_eq!(line.detail, "1 tap, 1 double-tap, 2 swipes, 4 sliders");
    }

    #[test]
    fn status_result_prioritizes_misconfiguration_over_service_state() {
        let lines = vec![
            StatusLine::fail(StatusSubject::Service, "inactive"),
            StatusLine::fail(StatusSubject::Device, "auto failed"),
        ];

        assert_eq!(status_result(&lines), StatusResult::Misconfigured);
    }
}

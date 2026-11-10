use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use evdev::Device;

use crate::config::{
    default_edgepad_config_path, load_edgepad_config, DeviceConfig, EdgepadConfig,
};
use crate::core::{EdgeWidths, SliderAxis, Zone};
use crate::device::{
    discover_device_report, format_device_line, summary_from_device, touchpad_candidates, AxisInfo,
    DeviceSummary,
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
    Sliders,
    Actions,
}

impl StatusSubject {
    pub fn label(self) -> &'static str {
        match self {
            Self::Service => "Service",
            Self::Config => "Config",
            Self::Device => "Device",
            Self::Size => "Size",
            Self::Sliders => "Sliders",
            Self::Zones => "Zones",
            Self::Actions => "Actions",
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
            lines.push(zones_status_line(&edgepad_config, size));
            lines.extend(slider_status_lines(&edgepad_config, size));
            lines.push(actions_status_line(&edgepad_config));
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
            StatusLine::ok(StatusSubject::Device, format_device_line(&device)),
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
        let dimension = |value: Option<f64>| match value {
            Some(mm) => format!("approximately {mm:.1} mm"),
            None => "unavailable".to_string(),
        };
        StatusLine::ok(
            StatusSubject::Size,
            format!(
                "width {}; height {} (device-reported; requires valid axis range and resolution)",
                dimension(self.width),
                dimension(self.height)
            ),
        )
    }
}

fn slider_status_lines(config: &EdgepadConfig, size: PhysicalSize) -> Vec<StatusLine> {
    config
        .sliders
        .iter()
        .map(|slider| {
            let (axis, length) = match slider.axis {
                SliderAxis::Horizontal => ("width", size.width),
                SliderAxis::Vertical => ("height", size.height),
            };
            let distance = match length {
                Some(mm) => format!("approximately {:.1} mm", mm * f64::from(slider.step)),
                None => "physical distance unavailable".to_string(),
            };
            StatusLine::ok(
                StatusSubject::Sliders,
                format!(
                    "{}: {:.1}% of {axis} ({distance}) per step",
                    zone_name(slider.zone),
                    slider.step * 100.0
                ),
            )
        })
        .collect()
}

fn zones_status_line(config: &EdgepadConfig, size: PhysicalSize) -> StatusLine {
    let active_zones: Vec<Zone> = ordered_zones()
        .iter()
        .copied()
        .filter(|zone| {
            config.gestures.iter().any(|binding| binding.zone == *zone)
                || config.sliders.iter().any(|binding| binding.zone == *zone)
        })
        .collect();
    let inactive_zones: Vec<Zone> = ordered_zones()
        .iter()
        .copied()
        .filter(|zone| !active_zones.contains(zone))
        .collect();

    if active_zones.is_empty() {
        return StatusLine::fail(StatusSubject::Zones, "none configured");
    }

    StatusLine::ok(
        StatusSubject::Zones,
        format!(
            "{} active; {} passthrough; widths: {}",
            zone_list_label(&active_zones),
            zone_list_label(&inactive_zones),
            edge_widths_label(config.active_edge_widths(), size)
        ),
    )
}

fn actions_status_line(config: &EdgepadConfig) -> StatusLine {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        return StatusLine::fail(StatusSubject::Actions, "no bindings configured");
    }

    let mut commands = config
        .gestures
        .iter()
        .filter_map(|binding| match &binding.action {
            crate::config::GestureActionConfig::Command { argv } => argv.first(),
            crate::config::GestureActionConfig::Log => None,
        })
        .collect::<BTreeSet<_>>();
    for slider in &config.sliders {
        if let Some(program) = slider.negative.argv.first() {
            commands.insert(program);
        }
        if let Some(program) = slider.positive.argv.first() {
            commands.insert(program);
        }
    }
    let command_count = commands.len();

    let detail = match command_count {
        0 => format!(
            "{}, {}; log actions only",
            binding_count_label(config.gestures.len()),
            slider_count_label(config.sliders.len())
        ),
        1 => format!(
            "{}, {}, 1 command",
            binding_count_label(config.gestures.len()),
            slider_count_label(config.sliders.len())
        ),
        count => format!(
            "{}, {}, {count} commands",
            binding_count_label(config.gestures.len()),
            slider_count_label(config.sliders.len())
        ),
    };

    StatusLine::ok(StatusSubject::Actions, detail)
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

fn zone_list_label(zones: &[Zone]) -> String {
    if zones.is_empty() {
        return "none".to_string();
    }

    zones
        .iter()
        .map(|zone| zone_name(*zone))
        .collect::<Vec<_>>()
        .join(", ")
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

fn edge_widths_label(widths: EdgeWidths, size: PhysicalSize) -> String {
    format!(
        "left {}, right {}, top {}, bottom {}",
        edge_width_label(widths.left, size.width),
        edge_width_label(widths.right, size.width),
        edge_width_label(widths.top, size.height),
        edge_width_label(widths.bottom, size.height)
    )
}

fn edge_width_label(width: f32, length_mm: Option<f64>) -> String {
    if width <= f32::EPSILON {
        "off".to_string()
    } else {
        match length_mm {
            Some(mm) => format!(
                "{:.1}% (approximately {:.1} mm)",
                width * 100.0,
                mm * f64::from(width)
            ),
            None => format!("{:.1}%", width * 100.0),
        }
    }
}

fn binding_count_label(count: usize) -> String {
    match count {
        1 => "1 gesture binding".to_string(),
        _ => format!("{count} gesture bindings"),
    }
}

fn slider_count_label(count: usize) -> String {
    match count {
        1 => "1 slider".to_string(),
        _ => format!("{count} sliders"),
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
            .contains("width approximately 150.0 mm; height approximately 100.0 mm"));
        assert_eq!(zones_status_line(&config, size).detail,
            "left, right, top, bottom active; none passthrough; widths: left 10.0% (approximately 15.0 mm), right 10.0% (approximately 15.0 mm), top 10.0% (approximately 10.0 mm), bottom 5.0% (approximately 5.0 mm)");
        let sliders = slider_status_lines(&config, size);
        let details: Vec<_> = sliders.iter().map(|line| line.detail.as_str()).collect();
        assert_eq!(
            details,
            [
                "left: 4.0% of height (approximately 4.0 mm) per step",
                "right: 4.0% of height (approximately 4.0 mm) per step",
                "top: 12.0% of width (approximately 18.0 mm) per step",
                "bottom: 12.0% of width (approximately 18.0 mm) per step",
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
        assert!(zones_status_line(&config, size)
            .detail
            .contains("top 10.0%, bottom 5.0%"));
        let sliders = slider_status_lines(&config, size);
        assert_eq!(
            sliders[0].detail,
            "left: 4.0% of height (physical distance unavailable) per step"
        );
        assert!(sliders[3].detail.contains("approximately 18.0 mm"));
        assert!(sliders
            .iter()
            .all(|line| line.severity == StatusSeverity::Ok));
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
                .contains("width unavailable; height unavailable"));
            assert_eq!(edge_width_label(0.0, size.width), "off");
        }
        assert_eq!(edge_width_label(0.0, Some(150.0)), "off");
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
        assert_eq!(
            line.detail,
            "right, top active; left, bottom passthrough; widths: left off, right 20.0%, top 20.0%, bottom off"
        );
    }

    #[test]
    fn actions_status_counts_unique_command_programs() {
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
            ],
            sliders: Vec::new(),
            ..EdgepadConfig::default()
        };

        let line = actions_status_line(&config);

        assert_eq!(line.detail, "3 gesture bindings, 0 sliders, 1 command");
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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use evdev::Device;

use crate::config::{
    default_edgepad_config_path, load_edgepad_config, DeviceConfig, EdgepadConfig,
};
use crate::core::{EdgeWidths, Zone};
use crate::device::{discover_device_report, format_device_line, touchpad_candidates};

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
    Zones,
    Actions,
}

impl StatusSubject {
    pub fn label(self) -> &'static str {
        match self {
            Self::Service => "Service",
            Self::Config => "Config",
            Self::Device => "Device",
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
    let mut lines = vec![service_status_line(&config.service_name)];

    match load_status_config(config) {
        Ok((config_path, edgepad_config)) => {
            lines.push(StatusLine::ok(
                StatusSubject::Config,
                config_path.display().to_string(),
            ));
            lines.push(device_status_line(
                &edgepad_config.device,
                &config.input_root,
            ));
            lines.push(zones_status_line(&edgepad_config));
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

fn device_status_line(device: &DeviceConfig, input_root: &Path) -> StatusLine {
    match device {
        DeviceConfig::Path(path) => explicit_device_status_line(path),
        DeviceConfig::Auto => auto_device_status_line(input_root),
    }
}

fn explicit_device_status_line(path: &Path) -> StatusLine {
    match Device::open(path) {
        Ok(device) => StatusLine::ok(
            StatusSubject::Device,
            format!(
                "{} {}",
                path.display(),
                device.name().unwrap_or("unknown touchpad")
            ),
        ),
        Err(err) => StatusLine::fail(
            StatusSubject::Device,
            format!(
                "configured path cannot be opened: {} ({err})",
                path.display()
            ),
        ),
    }
}

fn auto_device_status_line(input_root: &Path) -> StatusLine {
    match discover_device_report(input_root) {
        Ok(discovery) if discovery.event_node_count == 0 => StatusLine::fail(
            StatusSubject::Device,
            format!(
                "auto failed: no event devices under {}",
                input_root.display()
            ),
        ),
        Ok(discovery) if discovery.summaries.is_empty() => StatusLine::fail(
            StatusSubject::Device,
            format!(
                "auto failed: {} event device(s) found under {}, but none were readable",
                discovery.event_node_count,
                input_root.display()
            ),
        ),
        Ok(discovery) => match touchpad_candidates(&discovery.summaries).as_slice() {
            [] => StatusLine::fail(
                StatusSubject::Device,
                format!(
                    "auto failed: no touchpad candidates among {} readable event device(s)",
                    discovery.summaries.len()
                ),
            ),
            [candidate] => StatusLine::ok(StatusSubject::Device, format_device_line(candidate)),
            candidates => {
                let devices = candidates
                    .iter()
                    .map(|candidate| candidate.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                StatusLine::fail(
                    StatusSubject::Device,
                    format!("auto failed: multiple touchpad candidates: {devices}"),
                )
            }
        },
        Err(err) => StatusLine::fail(
            StatusSubject::Device,
            format!(
                "auto failed: failed to list {}: {err}",
                input_root.display()
            ),
        ),
    }
}

fn zones_status_line(config: &EdgepadConfig) -> StatusLine {
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
            edge_widths_label(config.active_edge_widths())
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

fn service_status_line(service_name: &str) -> StatusLine {
    match systemctl_user_show(service_name) {
        Ok(output) if output.status_success => {
            let load_state = property_value(&output.stdout, "LoadState").unwrap_or("unknown");
            let active_state = property_value(&output.stdout, "ActiveState").unwrap_or("unknown");
            let sub_state = property_value(&output.stdout, "SubState").unwrap_or("unknown");

            if load_state == "not-found" {
                StatusLine::fail(
                    StatusSubject::Service,
                    format!("{service_name} is not installed"),
                )
            } else if active_state == "active" {
                StatusLine::ok(StatusSubject::Service, format!("active ({sub_state})"))
            } else if active_state == "failed" {
                StatusLine::fail(StatusSubject::Service, format!("failed ({sub_state})"))
            } else {
                StatusLine::fail(
                    StatusSubject::Service,
                    format!("{active_state} ({sub_state})"),
                )
            }
        }
        Ok(output) => StatusLine::warn(
            StatusSubject::Service,
            format!(
                "unknown: systemctl --user failed: {}",
                output.stderr_or_stdout()
            ),
        ),
        Err(err) => StatusLine::warn(StatusSubject::Service, format!("unknown: {err}")),
    }
}

fn systemctl_user_show(service_name: &str) -> Result<CommandOutput, String> {
    command_output(
        "systemctl",
        &[
            "--user",
            "show",
            service_name,
            "-p",
            "LoadState",
            "-p",
            "ActiveState",
            "-p",
            "SubState",
        ],
    )
}

fn command_output(program: &str, args: &[&str]) -> Result<CommandOutput, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("failed to run {program}: {err}"))?;

    Ok(CommandOutput {
        status_success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandOutput {
    status_success: bool,
    stdout: String,
    stderr: String,
}

impl CommandOutput {
    fn stderr_or_stdout(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            stderr.to_string()
        } else {
            self.stdout.trim().to_string()
        }
    }
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

fn edge_widths_label(widths: EdgeWidths) -> String {
    format!(
        "left {}, right {}, top {}, bottom {}",
        edge_width_label(widths.left),
        edge_width_label(widths.right),
        edge_width_label(widths.top),
        edge_width_label(widths.bottom)
    )
}

fn edge_width_label(width: f32) -> String {
    if width <= f32::EPSILON {
        "off".to_string()
    } else {
        format!("{:.1}%", width * 100.0)
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

fn property_value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GestureActionConfig, GestureBindingConfig, DEFAULT_TAP_MIN_DURATION_MS};
    use crate::core::GestureDirection;

    #[test]
    fn zones_status_reports_active_and_passthrough_edges() {
        let config = EdgepadConfig {
            device: DeviceConfig::Auto,
            edge_width: 0.20,
            tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
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
        };

        let line = zones_status_line(&config);

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
            tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
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

    #[test]
    fn service_status_parses_active_systemd_unit() {
        let output = CommandOutput {
            status_success: true,
            stdout: "LoadState=loaded\nActiveState=active\nSubState=running\n".to_string(),
            stderr: String::new(),
        };

        assert_eq!(
            property_value(&output.stdout, "ActiveState"),
            Some("active")
        );
        assert_eq!(property_value(&output.stdout, "SubState"), Some("running"));
    }
}

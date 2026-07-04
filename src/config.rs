use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use crate::core::{Gesture, GestureDirection, Zone};
use crate::device::{
    discover_device_report, format_device_line, touchpad_candidates, DiscoveryReport,
};

pub const DEFAULT_EDGE_WIDTH: f32 = 0.10;

#[derive(Debug, Clone, PartialEq)]
pub struct EdgepadConfig {
    pub device: DeviceConfig,
    pub edge_width: f32,
    pub gestures: Vec<GestureBindingConfig>,
}

impl Default for EdgepadConfig {
    fn default() -> Self {
        Self {
            device: DeviceConfig::Auto,
            edge_width: DEFAULT_EDGE_WIDTH,
            gestures: Vec::new(),
        }
    }
}

impl EdgepadConfig {
    pub fn parse(input: &str) -> Result<Self, String> {
        let raw = toml::from_str::<RawEdgepadConfig>(input)
            .map_err(|err| format!("invalid TOML config: {err}"))?;
        let mut config = Self::default();
        let mut gesture_keys = BTreeSet::new();

        if let Some(device) = raw.device {
            config.device = DeviceConfig::parse(&device)
                .map_err(|err| format!("invalid device config: {err}"))?;
        }
        if let Some(edge_width) = raw.edge_width {
            config.edge_width = validate_edge_width(edge_width, "edge_width")?;
        }

        for (index, raw_binding) in raw.gestures.into_iter().enumerate() {
            let binding = GestureBindingConfig::from_raw(index, raw_binding)?;
            let gesture_key = (binding.zone, binding.direction);
            if !gesture_keys.insert(gesture_key) {
                return Err(format!(
                    "duplicate gesture binding {}.{}",
                    zone_name(binding.zone),
                    direction_name(binding.direction)
                ));
            }
            config.gestures.push(binding);
        }

        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceConfig {
    Auto,
    Path(PathBuf),
}

impl DeviceConfig {
    pub fn parse(raw: &str) -> Result<Self, String> {
        raw.parse()
    }

    pub fn resolve(&self, input_root: &Path) -> Result<PathBuf, String> {
        match self {
            Self::Auto => resolve_auto_touchpad(input_root),
            Self::Path(path) => Ok(path.clone()),
        }
    }
}

impl FromStr for DeviceConfig {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let value = raw.trim();
        if value.is_empty() {
            return Err("device must be `auto` or an event node path".to_string());
        }
        if value == "auto" {
            Ok(Self::Auto)
        } else {
            Ok(Self::Path(PathBuf::from(value)))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GestureBindingConfig {
    pub zone: Zone,
    pub direction: GestureDirection,
    pub action: GestureActionConfig,
}

impl GestureBindingConfig {
    pub fn matches(&self, gesture: &Gesture) -> bool {
        self.zone == gesture.zone && self.direction == gesture.direction
    }

    fn from_raw(index: usize, raw: RawGestureBindingConfig) -> Result<Self, String> {
        let label = gesture_label(index);
        let zone = parse_zone(&raw.zone)
            .ok_or_else(|| format!("{}.zone must be one of: left, right, top, bottom", label))?;
        let direction = parse_direction(&raw.direction).ok_or_else(|| {
            format!(
                "{}.direction must be one of: up, down, left, right, tap",
                label
            )
        })?;
        let action = GestureActionConfig::from_raw(index, raw.action)?;

        Ok(Self {
            zone,
            direction,
            action,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GestureActionConfig {
    Log,
    Command { argv: Vec<String> },
}

impl GestureActionConfig {
    fn from_raw(index: usize, raw: RawGestureActionConfig) -> Result<Self, String> {
        match raw {
            RawGestureActionConfig::Command(argv) => {
                Self::command(argv).map_err(|err| format!("{}.action: {err}", gesture_label(index)))
            }
            RawGestureActionConfig::Log { log: true } => Ok(Self::Log),
            RawGestureActionConfig::Log { log: false } => {
                Err(format!("{}.action.log must be true", gesture_label(index)))
            }
        }
    }

    pub fn command<I, S>(argv: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let argv = argv.into_iter().map(Into::into).collect::<Vec<_>>();
        if argv.is_empty() {
            return Err("gesture command action requires at least a program".to_string());
        }
        Ok(Self::Command { argv })
    }
}

pub fn load_edgepad_config(path: &Path) -> Result<EdgepadConfig, String> {
    let input = fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    EdgepadConfig::parse(&input)
        .map_err(|err| format!("failed to parse config {}: {err}", path.display()))
}

pub fn parse_edge_width(raw_value: &str) -> Result<f32, String> {
    let parsed = raw_value
        .parse::<f32>()
        .map_err(|_| "--edge-width must be > 0 and < 0.5".to_string())?;
    validate_edge_width(parsed, "--edge-width")
}

fn validate_edge_width(parsed: f32, name: &str) -> Result<f32, String> {
    if !(parsed > 0.0 && parsed < 0.5) {
        return Err(format!("{name} must be > 0 and < 0.5"));
    }
    Ok(parsed)
}

fn resolve_auto_touchpad(input_root: &Path) -> Result<PathBuf, String> {
    let report = discover_device_report(input_root)
        .map_err(|err| format!("failed to list {}: {err}", input_root.display()))?;
    resolve_auto_touchpad_from_report(input_root, &report)
}

fn resolve_auto_touchpad_from_report(
    input_root: &Path,
    report: &DiscoveryReport,
) -> Result<PathBuf, String> {
    if report.event_node_count == 0 {
        return Err(format!(
            "device=auto found no event devices under {}",
            input_root.display()
        ));
    }

    if report.summaries.is_empty() {
        return Err(format!(
            "device=auto found no readable event devices under {} ({}; try sudo, group input, or seat/logind ACLs)",
            input_root.display(),
            event_node_count_text(report.event_node_count)
        ));
    }

    let candidates = touchpad_candidates(&report.summaries);
    match candidates.as_slice() {
        [] => Err(format!(
            "device=auto found no touchpad candidates under {} (readable non-touchpad devices: {})",
            input_root.display(),
            report.summaries.len()
        )),
        [candidate] => Ok(candidate.path.clone()),
        _ => {
            let mut message = format!(
                "device=auto matched multiple touchpad candidates under {}; pass --device <event-node> explicitly",
                input_root.display()
            );
            for candidate in candidates {
                message.push_str("\n  ");
                message.push_str(&format_device_line(candidate));
            }
            Err(message)
        }
    }
}

fn parse_zone(raw: &str) -> Option<Zone> {
    match raw {
        "left" => Some(Zone::Left),
        "right" => Some(Zone::Right),
        "top" => Some(Zone::Top),
        "bottom" => Some(Zone::Bottom),
        _ => None,
    }
}

fn parse_direction(raw: &str) -> Option<GestureDirection> {
    match raw {
        "up" => Some(GestureDirection::Up),
        "down" => Some(GestureDirection::Down),
        "left" => Some(GestureDirection::Left),
        "right" => Some(GestureDirection::Right),
        "tap" => Some(GestureDirection::Tap),
        _ => None,
    }
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

fn direction_name(direction: GestureDirection) -> &'static str {
    match direction {
        GestureDirection::Up => "up",
        GestureDirection::Down => "down",
        GestureDirection::Left => "left",
        GestureDirection::Right => "right",
        GestureDirection::Tap => "tap",
    }
}

fn event_node_count_text(count: usize) -> String {
    if count == 1 {
        "1 event node was present but could not be opened".to_string()
    } else {
        format!("{count} event nodes were present but could not be opened")
    }
}

fn gesture_label(index: usize) -> String {
    format!("gestures[{index}]")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEdgepadConfig {
    device: Option<String>,
    edge_width: Option<f32>,
    #[serde(default)]
    gestures: Vec<RawGestureBindingConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGestureBindingConfig {
    zone: String,
    direction: String,
    action: RawGestureActionConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawGestureActionConfig {
    Command(Vec<String>),
    Log { log: bool },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{AxisInfo, DeviceKind, DeviceSummary};

    #[test]
    fn device_config_parses_auto_and_path() {
        assert_eq!(DeviceConfig::parse("auto"), Ok(DeviceConfig::Auto));
        assert_eq!(
            DeviceConfig::parse("/dev/input/event7"),
            Ok(DeviceConfig::Path(PathBuf::from("/dev/input/event7")))
        );
    }

    #[test]
    fn auto_device_resolution_reports_empty_root() {
        let report = DiscoveryReport {
            summaries: Vec::new(),
            event_node_count: 0,
            unreadable_count: 0,
        };

        let result = resolve_auto_touchpad_from_report(Path::new("/tmp/input"), &report);

        assert_eq!(
            result.as_ref().err().map(String::as_str),
            Some("device=auto found no event devices under /tmp/input")
        );
    }

    #[test]
    fn auto_device_resolution_selects_single_touchpad_candidate() {
        let report = DiscoveryReport {
            summaries: vec![device_summary(
                "/dev/input/event5",
                "Example Touchpad",
                DeviceKind::Touchpad,
            )],
            event_node_count: 1,
            unreadable_count: 0,
        };

        let result = resolve_auto_touchpad_from_report(Path::new("/dev/input"), &report);

        assert_eq!(result, Ok(PathBuf::from("/dev/input/event5")));
    }

    #[test]
    fn auto_device_resolution_rejects_ambiguous_touchpads() {
        let report = DiscoveryReport {
            summaries: vec![
                device_summary("/dev/input/event5", "First Touchpad", DeviceKind::Touchpad),
                device_summary("/dev/input/event6", "Second Touchpad", DeviceKind::Touchpad),
            ],
            event_node_count: 2,
            unreadable_count: 0,
        };

        let result = resolve_auto_touchpad_from_report(Path::new("/dev/input"), &report)
            .expect_err("ambiguous auto resolution should fail");

        assert!(result.contains("device=auto matched multiple touchpad candidates"));
        assert!(result.contains("/dev/input/event5"));
        assert!(result.contains("/dev/input/event6"));
    }

    #[test]
    fn edgepad_config_parses_device_edge_width_and_gesture_bindings() {
        let config = EdgepadConfig::parse(
            r#"
            device = "/dev/input/event7"
            edge_width = 0.20

            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }

            [[gestures]]
            zone = "right"
            direction = "down"
            action = ["notify-send", "edgepad", "right-down"]
            "#,
        )
        .expect("config should parse");

        assert_eq!(
            config.device,
            DeviceConfig::Path(PathBuf::from("/dev/input/event7"))
        );
        assert_eq!(config.edge_width, 0.20);
        assert_eq!(
            config.gestures,
            vec![
                GestureBindingConfig {
                    zone: Zone::Left,
                    direction: GestureDirection::Right,
                    action: GestureActionConfig::Log,
                },
                GestureBindingConfig {
                    zone: Zone::Right,
                    direction: GestureDirection::Down,
                    action: GestureActionConfig::Command {
                        argv: vec![
                            "notify-send".to_string(),
                            "edgepad".to_string(),
                            "right-down".to_string(),
                        ],
                    },
                },
            ]
        );
    }

    #[test]
    fn edgepad_config_rejects_duplicate_gesture_binding() {
        let result = EdgepadConfig::parse(
            r#"
            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }

            [[gestures]]
            zone = "left"
            direction = "right"
            action = ["notify-send", "duplicate"]
            "#,
        );

        assert_eq!(
            result.as_ref().err().map(String::as_str),
            Some("duplicate gesture binding left.right")
        );
    }

    #[test]
    fn edgepad_config_rejects_empty_command_action_array() {
        let result = EdgepadConfig::parse(
            r#"
            [[gestures]]
            zone = "top"
            direction = "right"
            action = []
            "#,
        );

        assert_eq!(
            result.as_ref().err().map(String::as_str),
            Some("gestures[0].action: gesture command action requires at least a program")
        );
    }

    #[test]
    fn gesture_binding_matches_recognized_gesture() {
        let binding = GestureBindingConfig {
            zone: Zone::Top,
            direction: GestureDirection::Down,
            action: GestureActionConfig::Log,
        };
        let gesture = Gesture {
            zone: Zone::Top,
            direction: GestureDirection::Down,
            slot: 0,
            tracking_id: 10,
        };

        assert!(binding.matches(&gesture));
    }

    fn device_summary(path: &str, name: &str, kind: DeviceKind) -> DeviceSummary {
        DeviceSummary {
            path: PathBuf::from(path),
            name: name.to_string(),
            vendor: 0x1234,
            product: 0x5678,
            kind,
            slot_range: matches!(kind, DeviceKind::Touchpad | DeviceKind::Touchscreen)
                .then_some(AxisInfo { min: 0, max: 4 }),
            x_range: matches!(kind, DeviceKind::Touchpad | DeviceKind::Touchscreen)
                .then_some(AxisInfo { min: 0, max: 1000 }),
            y_range: matches!(kind, DeviceKind::Touchpad | DeviceKind::Touchscreen)
                .then_some(AxisInfo { min: 0, max: 700 }),
        }
    }
}

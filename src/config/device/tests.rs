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

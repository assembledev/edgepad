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

    let err = result.expect_err("empty root should fail");
    assert!(
        matches!(&err, DeviceResolveError::NoEventDevices { input_root } if input_root == Path::new("/tmp/input"))
    );
    assert_eq!(
        err.to_string(),
        "device=auto found no event devices under /tmp/input"
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

    assert_eq!(
        result.expect("one candidate should resolve"),
        PathBuf::from("/dev/input/event5")
    );
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

    assert!(
        matches!(&result, DeviceResolveError::AmbiguousTouchpads { candidates, .. } if candidates.len() == 2)
    );
    let message = result.to_string();
    assert!(message.contains("device=auto matched multiple touchpad candidates"));
    assert!(message.contains("\n  /dev/input/event5"));
    assert!(message.contains("\n  /dev/input/event6"));
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

#[test]
fn auto_device_resolution_distinguishes_unreadable_nodes_from_non_touchpads() {
    for count in [1, 3] {
        let report = DiscoveryReport {
            summaries: Vec::new(),
            event_node_count: count,
            unreadable_count: count,
        };
        let err = resolve_auto_touchpad_from_report(Path::new("/input"), &report)
            .expect_err("unreadable event nodes should fail");
        assert!(
            matches!(&err, DeviceResolveError::NoReadableEventDevices { event_node_count, .. } if *event_node_count == count)
        );
        assert!(err.to_string().contains(&event_node_count_text(count)));
    }

    let report = DiscoveryReport {
        summaries: vec![device_summary(
            "/input/event0",
            "Keyboard",
            DeviceKind::Other,
        )],
        event_node_count: 1,
        unreadable_count: 0,
    };
    let err = resolve_auto_touchpad_from_report(Path::new("/input"), &report)
        .expect_err("a non-touchpad should not resolve");
    assert!(matches!(
        err,
        DeviceResolveError::NoTouchpadCandidates {
            readable_count: 1,
            ..
        }
    ));
}

#[test]
fn auto_device_discovery_preserves_io_source_and_path() {
    let input_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let err = DeviceConfig::Auto
        .resolve(&input_root)
        .expect_err("a regular file cannot be an input directory");
    assert!(
        matches!(&err, DeviceResolveError::Discovery { input_root: path, .. } if path == &input_root)
    );
    let source = err
        .source()
        .and_then(|source| source.downcast_ref::<io::Error>())
        .expect("discovery should retain the io::Error");
    assert_eq!(source.raw_os_error(), Some(libc::ENOTDIR));
    assert_eq!(
        err.to_string(),
        format!("failed to list {}: {source}", input_root.display())
    );
}

#[test]
fn explicit_device_resolution_defers_opening_to_the_proxy() {
    let path = PathBuf::from("/does-not-exist/edgepad/event7");
    assert_eq!(
        DeviceConfig::Path(path.clone())
            .resolve(Path::new("/unused"))
            .unwrap(),
        path
    );
}

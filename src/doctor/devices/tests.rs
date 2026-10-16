use super::*;
use crate::device::{DeviceKind, DeviceSummary};

fn summary(name: &str, kind: DeviceKind) -> DeviceSummary {
    DeviceSummary {
        path: PathBuf::from(name),
        name: name.to_string(),
        vendor: 1,
        product: 2,
        kind,
        slot_range: None,
        x_range: None,
        y_range: None,
    }
}

fn discovery(
    summaries: Vec<DeviceSummary>,
    unreadable_count: usize,
) -> io::Result<DiscoveryReport> {
    Ok(DiscoveryReport {
        event_node_count: summaries.len() + unreadable_count,
        summaries,
        unreadable_count,
    })
}

#[test]
fn auto_selection_uses_the_only_touchpad_among_readable_devices() {
    let (path, check) = auto_selection(
        discovery(
            vec![
                summary("/input/event1", DeviceKind::Touchscreen),
                summary("/input/event2", DeviceKind::Touchpad),
                summary("/input/event3", DeviceKind::Other),
            ],
            1,
        ),
        Path::new("/input"),
    );
    assert_eq!(path, Some(PathBuf::from("/input/event2")));
    assert_eq!(check.status, CheckStatus::Ok);
    assert!(check.detail.contains("/input/event2"));
}

#[test]
fn ambiguous_touchpads_are_reported_without_selecting_one() {
    let (path, check) = auto_selection(
        discovery(
            vec![
                summary("/input/event1", DeviceKind::Touchpad),
                summary("/input/event2", DeviceKind::Touchpad),
            ],
            0,
        ),
        Path::new("/input"),
    );
    assert_eq!(path, None);
    assert_eq!(check.status, CheckStatus::Fail);
    assert!(check.detail.contains("/input/event1, /input/event2"));
}

#[test]
fn discovery_failures_distinguish_empty_unreadable_and_unsuitable_devices() {
    for (report, detail) in [
        (discovery(vec![], 0), "no event devices found"),
        (
            discovery(vec![], 2),
            "2 event node(s) found under /input, but none were readable",
        ),
        (
            discovery(vec![summary("/input/event1", DeviceKind::Other)], 0),
            "no touchpad candidates among 1 readable",
        ),
        (
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            "failed to list /input",
        ),
    ] {
        let (path, check) = auto_selection(report, Path::new("/input"));
        assert_eq!(path, None);
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.detail.contains(detail), "{}", check.detail);
    }
}

#[test]
fn failed_input_access_does_not_skip_the_uinput_probe() {
    let dir = crate::doctor::test_support::TestDir::new();
    let event = dir.path().join("event0");
    let uinput = dir.path().join("uinput");
    std::fs::write(&uinput, "").unwrap();
    let result = check(
        &DeviceConfig::Path(event.clone()),
        &dir.path().join("unused-input-root"),
        &uinput,
    );
    assert_eq!(result.access.touchpad_path, Some(event));
    assert_eq!(result.access.uinput_path, uinput);
    assert!(!result.access.both_devices_open);
    assert_eq!(
        result
            .checks
            .iter()
            .map(|check| (check.label, check.status))
            .collect::<Vec<_>>(),
        [
            ("touchpad", CheckStatus::Warn),
            ("input", CheckStatus::Fail),
            ("uinput", CheckStatus::Ok),
        ],
    );
}

use std::fs;
use std::path::PathBuf;

use edgepad::device::{
    classify_device, event_device_paths, format_device_line, AxisInfo, DeviceKind, DeviceSummary,
    EventDeviceCapabilities,
};

#[test]
fn classify_touchpad_from_multitouch_pointer_capabilities() {
    let caps = EventDeviceCapabilities {
        has_mt_slot: true,
        has_mt_tracking_id: true,
        has_mt_position_x: true,
        has_mt_position_y: true,
        has_abs_x: true,
        has_abs_y: true,
        is_pointer: true,
        is_direct: false,
        is_buttonpad: true,
    };

    assert_eq!(classify_device(&caps), DeviceKind::Touchpad);
}

#[test]
fn classify_touchscreen_separately_from_touchpad() {
    let caps = EventDeviceCapabilities {
        has_mt_slot: true,
        has_mt_tracking_id: true,
        has_mt_position_x: true,
        has_mt_position_y: true,
        has_abs_x: true,
        has_abs_y: true,
        is_pointer: false,
        is_direct: true,
        is_buttonpad: false,
    };

    assert_eq!(classify_device(&caps), DeviceKind::Touchscreen);
}

#[test]
fn event_device_paths_only_returns_event_number_nodes_sorted_numerically() {
    let root = unique_temp_dir("edgepad-device-paths");
    fs::create_dir_all(&root).expect("temp root should be created");
    fs::write(root.join("event10"), b"").expect("event10 should be created");
    fs::write(root.join("event2"), b"").expect("event2 should be created");
    fs::write(root.join("event0"), b"").expect("event0 should be created");
    fs::write(root.join("mouse0"), b"").expect("mouse0 should be ignored");
    fs::write(root.join("eventfoo"), b"").expect("eventfoo should be ignored");

    let paths = event_device_paths(&root).expect("temp root should list");

    assert_eq!(
        paths,
        vec![
            root.join("event0"),
            root.join("event2"),
            root.join("event10")
        ]
    );

    fs::remove_dir_all(root).expect("temp root should be removed");
}

#[test]
fn format_device_line_prints_stable_debug_summary() {
    let summary = DeviceSummary {
        path: PathBuf::from("/dev/input/event5"),
        name: "Example Touchpad".to_string(),
        vendor: 0x1234,
        product: 0x5678,
        kind: DeviceKind::Touchpad,
        slot_range: Some(AxisInfo { min: 0, max: 4 }),
        x_range: Some(AxisInfo { min: 0, max: 1000 }),
        y_range: Some(AxisInfo { min: 0, max: 700 }),
    };

    assert_eq!(
        format_device_line(&summary),
        "/dev/input/event5 kind=touchpad name=\"Example Touchpad\" id=1234:5678 slots=0..=4 x=0..=1000 y=0..=700"
    );
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{}-{}-{}",
        prefix,
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ))
}

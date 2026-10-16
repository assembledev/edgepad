use super::*;
use crate::config::{
    EdgeWidthOverrides, GestureActionConfig, GestureBindingConfig, DEFAULT_SWIPE_MIN_DISTANCE,
    DEFAULT_TAP_MIN_DURATION_MS,
};
use crate::core::GestureDirection;
use std::path::PathBuf;

#[test]
fn active_zones_detail_reports_daemon_claim_widths() {
    let config = EdgepadConfig {
        device: DeviceConfig::Auto,
        edge_width: 0.20,
        edge_width_overrides: EdgeWidthOverrides {
            right: Some(0.14),
            ..EdgeWidthOverrides::default()
        },
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

    assert_eq!(
        active_zones_detail(&config),
        "claiming right, top; passthrough left, bottom; widths: left off, right 14.0%, top 20.0%, bottom off"
    );
}

#[test]
fn config_device_uses_config_when_no_cli_override_is_present() {
    let config = EdgepadConfig {
        device: DeviceConfig::Path(PathBuf::from("/dev/input/event7")),
        edge_width: 0.10,
        edge_width_overrides: EdgeWidthOverrides::default(),
        tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
        swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
        gestures: Vec::new(),
        sliders: Vec::new(),
        ..EdgepadConfig::default()
    };

    let (device, check) = select_device(Some(&config), None);

    assert_eq!(
        device,
        DeviceConfig::Path(PathBuf::from("/dev/input/event7"))
    );
    assert_eq!(check.status, CheckStatus::Ok);
    assert!(check.detail.contains("/dev/input/event7"));
}

#[test]
fn config_device_uses_cli_override_when_present() {
    let config = EdgepadConfig {
        device: DeviceConfig::Auto,
        edge_width: 0.10,
        edge_width_overrides: EdgeWidthOverrides::default(),
        tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
        swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
        gestures: Vec::new(),
        sliders: Vec::new(),
        ..EdgepadConfig::default()
    };

    let (device, check) = select_device(
        Some(&config),
        Some(&DeviceConfig::Path(PathBuf::from("/dev/input/event9"))),
    );

    assert_eq!(
        device,
        DeviceConfig::Path(PathBuf::from("/dev/input/event9"))
    );
    assert_eq!(check.status, CheckStatus::Warn);
    assert!(check.detail.contains("overridden by --device"));
}

#[test]
fn config_failures_keep_the_device_override_available_for_diagnostics() {
    let dir = crate::doctor::test_support::TestDir::new();
    let missing = dir.path().join("missing.toml");
    let invalid = dir.path().join("invalid.toml");
    fs::write(&invalid, "[broken").unwrap();
    let device = DeviceConfig::Path(dir.path().join("event0"));

    for (path, failed_check) in [(&missing, "path"), (&invalid, "file")] {
        let result = check(Some(path), Some(&device));
        assert!(result.loaded.is_none());
        assert_eq!(result.device, device);
        assert!(result
            .checks
            .iter()
            .any(|check| check.label == failed_check && check.status == CheckStatus::Fail));
        let device_check = result.checks.last().unwrap();
        assert_eq!(device_check.label, "device");
        assert_eq!(device_check.status, CheckStatus::Warn);
        assert!(device_check.detail.contains("using --device"));
    }

    let fallback = check(Some(&missing), None);
    assert_eq!(fallback.device, DeviceConfig::Auto);
    assert!(fallback.loaded.is_none());
    assert_eq!(
        fallback.checks.last().unwrap().detail,
        "using auto because config was not loaded"
    );
}

#[test]
fn loaded_config_keeps_bindings_when_the_device_is_overridden() {
    let dir = crate::doctor::test_support::TestDir::new();
    let path = dir.path().join("edgepad.toml");
    fs::write(
        &path,
        r#"
device = "auto"
[[gestures]]
zone = "left"
direction = "tap"
action = { log = true }
"#,
    )
    .unwrap();
    let device = DeviceConfig::Path(dir.path().join("event0"));
    let result = check(Some(&path), Some(&device));
    assert_eq!(result.device, device);
    let loaded = result
        .loaded
        .unwrap_or_else(|| panic!("configuration checks: {:?}", result.checks));
    assert_eq!(loaded.device, DeviceConfig::Auto);
    assert_eq!(loaded.gestures.len(), 1);
    assert_eq!(loaded.gestures[0].zone, Zone::Left);
    assert_eq!(loaded.gestures[0].action, GestureActionConfig::Log);
    assert!(result
        .checks
        .iter()
        .all(|check| check.status != CheckStatus::Fail));
}

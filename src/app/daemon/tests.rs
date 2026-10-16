use super::*;
use edgepad::core::Zone;
use std::fs;
use std::sync::atomic::AtomicU64;

static TEST_PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn unique_test_config_path(label: &str) -> PathBuf {
    let sequence = TEST_PATH_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("{label}-{}-{sequence}.toml", std::process::id()))
}

fn parse_test_daemon_config(input: &str) -> EdgepadConfig {
    EdgepadConfig::parse(input).expect("test daemon config should parse")
}

#[test]
fn daemon_startup_retry_timeout_env_parses_millis() {
    assert_eq!(
        parse_daemon_startup_retry_timeout_ms("250").expect("timeout should parse"),
        Duration::from_millis(250)
    );
    assert!(parse_daemon_startup_retry_timeout_ms("wat").is_err());
}

mod startup;

#[test]
fn daemon_reload_applies_valid_config_and_preserves_cli_overrides() {
    let config_path = unique_test_config_path("edgepad-reload-valid");
    let device_override = DeviceConfig::Path(PathBuf::from("/dev/input/event7"));
    let overrides = DaemonConfigOverrides {
        device: Some(device_override.clone()),
        edge_width: Some(0.15),
    };
    let mut initial = parse_test_daemon_config(
        r#"
            device = "/dev/input/event1"
            edge_width = 0.10

            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }
            "#,
    );
    overrides.apply(&mut initial);
    fs::write(
        &config_path,
        r#"
            device = "/dev/input/event9"
            edge_width = 0.30
            right_edge_width = 0.35

            [[gestures]]
            zone = "right"
            direction = "down"
            action = { log = true }
            "#,
    )
    .expect("reload config should be written");
    let args = DaemonArgs {
        config: initial,
        config_path: config_path.clone(),
        input_root: PathBuf::from("/dev/input"),
        overrides,
    };
    let reload = ReloadToken::new();
    let mut runtime =
        DaemonRuntimeHandler::new(&args, reload.clone()).expect("runtime should start");

    reload.request();
    let recognition = runtime
        .apply_pending_reload()
        .expect("valid reload should apply");

    assert_eq!(runtime.active_config().device, device_override);
    assert_eq!(runtime.active_config().edge_width, 0.15);
    assert_eq!(runtime.active_config().edge_width_overrides.right, None);
    assert_eq!(runtime.active_config().gestures[0].zone, Zone::Right);
    assert_eq!(recognition.edge_widths.right, 0.15);
    assert_eq!(recognition.edge_widths.left, 0.0);
    runtime.shutdown();
    fs::remove_file(config_path).expect("test config should be removed");
}

#[test]
fn daemon_reload_rejects_invalid_config_without_changing_active_config() {
    let config_path = unique_test_config_path("edgepad-reload-invalid");
    let initial = parse_test_daemon_config(
        r#"
            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }
            "#,
    );
    fs::write(&config_path, "this is not toml = [")
        .expect("invalid reload config should be written");
    let args = DaemonArgs {
        config: initial.clone(),
        config_path: config_path.clone(),
        input_root: PathBuf::from("/dev/input"),
        overrides: DaemonConfigOverrides::default(),
    };
    let reload = ReloadToken::new();
    let mut runtime =
        DaemonRuntimeHandler::new(&args, reload.clone()).expect("runtime should start");

    reload.request();

    assert!(runtime.apply_pending_reload().is_none());
    assert_eq!(runtime.active_config(), &initial);
    runtime.shutdown();
    fs::remove_file(config_path).expect("test config should be removed");
}

#[test]
fn daemon_reload_rejects_palm_rejection_change_as_restart_only() {
    let config_path = unique_test_config_path("edgepad-reload-palm-rejection");
    let initial = parse_test_daemon_config(
        r#"
            device = "auto"
            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }
            "#,
    );
    fs::write(
        &config_path,
        r#"
            device = "auto"
            palm_rejection = false
            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }
            "#,
    )
    .unwrap();
    let args = DaemonArgs {
        config: initial.clone(),
        config_path: config_path.clone(),
        input_root: PathBuf::from("/dev/input"),
        overrides: DaemonConfigOverrides::default(),
    };
    let reload = ReloadToken::new();
    let mut runtime = DaemonRuntimeHandler::new(&args, reload.clone()).unwrap();
    reload.request();
    assert!(runtime.apply_pending_reload().is_none());
    assert_eq!(runtime.active_config(), &initial);
    runtime.shutdown();
    fs::remove_file(config_path).unwrap();
}

#[test]
fn daemon_reload_rejects_device_change_as_restart_only() {
    let config_path = unique_test_config_path("edgepad-reload-device");
    let initial = parse_test_daemon_config(
        r#"
            device = "auto"

            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }
            "#,
    );
    fs::write(
        &config_path,
        r#"
            device = "/dev/input/event9"

            [[gestures]]
            zone = "right"
            direction = "down"
            action = { log = true }
            "#,
    )
    .expect("reload config should be written");
    let args = DaemonArgs {
        config: initial.clone(),
        config_path: config_path.clone(),
        input_root: PathBuf::from("/dev/input"),
        overrides: DaemonConfigOverrides::default(),
    };
    let reload = ReloadToken::new();
    let mut runtime =
        DaemonRuntimeHandler::new(&args, reload.clone()).expect("runtime should start");

    reload.request();

    assert!(runtime.apply_pending_reload().is_none());
    assert_eq!(runtime.active_config(), &initial);
    runtime.shutdown();
    fs::remove_file(config_path).expect("test config should be removed");
}

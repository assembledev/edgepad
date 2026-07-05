use std::path::PathBuf;
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn daemon_cli_accepts_explicit_device_before_device_open() {
    let config_path = unique_temp_path("edgepad-daemon-explicit-device-config");
    let missing_device = unique_temp_path("edgepad-missing-daemon-device");
    write_daemon_config(&config_path, "auto");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "0")
        .arg("daemon")
        .arg("--config")
        .arg(&config_path)
        .arg("--device")
        .arg(&missing_device)
        .arg("--edge-width")
        .arg("0.20")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "daemon should still fail for missing device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "daemon should parse arguments before device open, stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&missing_device.display().to_string()),
        "stderr was: {stderr}"
    );

    std::fs::remove_file(config_path).expect("config should be removed");
}

#[test]
fn daemon_cli_rejects_invalid_edge_width_before_auto_discovery() {
    let root = unique_temp_dir("edgepad-daemon-bad-edge-width");
    std::fs::create_dir_all(&root).expect("temp root should be created");

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "0")
        .arg("daemon")
        .arg("--device")
        .arg("auto")
        .arg("--input-root")
        .arg(&root)
        .arg("--edge-width")
        .arg("0.80")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "bad edge width should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--edge-width must be > 0 and < 0.5"),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains("device=auto found no event devices"),
        "edge-width validation should happen before discovery, stderr was: {stderr}"
    );

    std::fs::remove_dir_all(root).expect("temp root should be removed");
}

#[test]
fn daemon_cli_reports_empty_auto_input_root_without_touching_real_hardware() {
    let config_path = unique_temp_path("edgepad-daemon-empty-input-root-config");
    let root = unique_temp_dir("edgepad-daemon-empty-input-root");
    write_daemon_config(&config_path, "auto");
    std::fs::create_dir_all(&root).expect("temp root should be created");

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "0")
        .arg("daemon")
        .arg("--config")
        .arg(&config_path)
        .arg("--device")
        .arg("auto")
        .arg("--input-root")
        .arg(&root)
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "empty auto root should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("device=auto found no event devices"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&root.display().to_string()),
        "stderr was: {stderr}"
    );

    std::fs::remove_file(config_path).expect("config should be removed");
    std::fs::remove_dir_all(root).expect("temp root should be removed");
}

#[test]
fn daemon_cli_times_out_startup_retry_with_clear_error() {
    let config_path = unique_temp_path("edgepad-daemon-retry-empty-input-root-config");
    let root = unique_temp_dir("edgepad-daemon-retry-empty-input-root");
    write_daemon_config(&config_path, "auto");
    std::fs::create_dir_all(&root).expect("temp root should be created");

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "1")
        .arg("daemon")
        .arg("--config")
        .arg(&config_path)
        .arg("--device")
        .arg("auto")
        .arg("--input-root")
        .arg(&root)
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "empty auto root should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("edgepad daemon startup timed out"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("last error: device=auto found no event devices"),
        "stderr was: {stderr}"
    );

    std::fs::remove_file(config_path).expect("config should be removed");
    std::fs::remove_dir_all(root).expect("temp root should be removed");
}

#[test]
fn daemon_cli_reports_missing_default_config_path() {
    let config_home = unique_temp_dir("edgepad-daemon-missing-default-config-home");
    let _ = std::fs::remove_dir_all(&config_home);
    std::fs::create_dir_all(&config_home).expect("config home should be created");
    let expected_config = config_home.join("edgepad/edgepad.toml");

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "0")
        .env("XDG_CONFIG_HOME", &config_home)
        .arg("daemon")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "missing config should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("daemon config not found"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&expected_config.display().to_string()),
        "stderr was: {stderr}"
    );

    std::fs::remove_dir_all(config_home).expect("config home should be removed");
}

#[test]
fn daemon_cli_loads_default_config_path_from_xdg_config_home() {
    let config_home = unique_temp_dir("edgepad-daemon-xdg-config-home");
    let config_path = config_home.join("edgepad/edgepad.toml");
    let missing_device = unique_temp_path("edgepad-daemon-xdg-config-device");
    let _ = std::fs::remove_dir_all(&config_home);
    let _ = std::fs::remove_file(&missing_device);
    write_daemon_config(&config_path, &missing_device.display().to_string());

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "0")
        .env("XDG_CONFIG_HOME", &config_home)
        .arg("daemon")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "daemon should still fail for missing configured device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("edgepad daemon: config={}", config_path.display())),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("failed to open device"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&missing_device.display().to_string()),
        "stderr was: {stderr}"
    );

    std::fs::remove_dir_all(config_home).expect("config home should be removed");
}

#[test]
fn daemon_cli_rejects_empty_gesture_config_before_device_open() {
    let config_path = unique_temp_path("edgepad-daemon-empty-gesture-config");
    let missing_device = unique_temp_path("edgepad-daemon-empty-gesture-device");
    let _ = std::fs::remove_file(&missing_device);
    std::fs::write(
        &config_path,
        format!("device = \"{}\"\n", missing_device.display()),
    )
    .expect("config should be written");

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "0")
        .arg("daemon")
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "empty gestures should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has no gesture bindings"),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains("failed to open device"),
        "daemon should reject config before opening the device, stderr was: {stderr}"
    );

    std::fs::remove_file(config_path).expect("config should be removed");
}

#[test]
fn daemon_cli_loads_config_and_allows_device_override() {
    let config_path = unique_temp_path("edgepad-daemon-config");
    let configured_device = unique_temp_path("edgepad-configured-daemon-device");
    let override_device = unique_temp_path("edgepad-override-daemon-device");
    let _ = std::fs::remove_file(&configured_device);
    let _ = std::fs::remove_file(&override_device);
    std::fs::write(
        &config_path,
        format!(
            r#"
device = "{}"
edge_width = 0.20

[[gestures]]
zone = "left"
direction = "right"
action = ["notify-send", "edgepad", "left-right"]
"#,
            configured_device.display()
        ),
    )
    .expect("config should be written");

    let output = edgepad()
        .env("EDGEPAD_DAEMON_STARTUP_RETRY_MS", "0")
        .arg("daemon")
        .arg("--config")
        .arg(&config_path)
        .arg("--device")
        .arg(&override_device)
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "daemon should still fail for missing override device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "daemon should parse config before device open, stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&override_device.display().to_string()),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains(&configured_device.display().to_string()),
        "CLI --device should override config device, stderr was: {stderr}"
    );

    std::fs::remove_file(config_path).expect("config should be removed");
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

fn write_daemon_config(path: &std::path::Path, device: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("config parent should be created");
    }
    std::fs::write(
        path,
        format!(
            r#"
device = "{device}"
edge_width = 0.20

[[gestures]]
zone = "left"
direction = "right"
action = ["notify-send", "edgepad", "left-right"]
"#,
        ),
    )
    .expect("config should be written");
}

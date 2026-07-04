use std::path::PathBuf;
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn daemon_cli_accepts_explicit_device_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-daemon-device");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("daemon")
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
}

#[test]
fn daemon_cli_rejects_invalid_edge_width_before_auto_discovery() {
    let root = unique_temp_dir("edgepad-daemon-bad-edge-width");
    std::fs::create_dir_all(&root).expect("temp root should be created");

    let output = edgepad()
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
    let root = unique_temp_dir("edgepad-daemon-empty-input-root");
    std::fs::create_dir_all(&root).expect("temp root should be created");

    let output = edgepad()
        .arg("daemon")
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

    std::fs::remove_dir_all(root).expect("temp root should be removed");
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

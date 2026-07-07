use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn doctor_cli_checks_config_and_action_executables() {
    let root = unique_temp_dir("edgepad-doctor-cli-input-root");
    let config_path = unique_temp_path("edgepad-doctor-cli-config");
    let missing_uinput = unique_temp_path("edgepad-doctor-cli-uinput");
    let missing_action = unique_temp_path("edgepad-doctor-cli-missing-action");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&config_path);
    let _ = fs::remove_file(&missing_uinput);
    let _ = fs::remove_file(&missing_action);
    fs::create_dir_all(&root).expect("input root should be created");
    write_config(&config_path, "auto", &missing_action);

    let output = edgepad()
        .arg("doctor")
        .arg("--config")
        .arg(&config_path)
        .arg("--input-root")
        .arg(&root)
        .arg("--uinput")
        .arg(&missing_uinput)
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "doctor should report failures");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Config"), "stdout was: {stdout}");
    assert!(stdout.contains("path"), "stdout was: {stdout}");
    assert!(
        stdout.contains(&config_path.display().to_string()),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("file"), "stdout was: {stdout}");
    assert!(
        stdout.contains("device auto, edge width 10.0%, 1 gesture binding"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("zones"), "stdout was: {stdout}");
    assert!(
        stdout.contains("claiming right; passthrough left, top, bottom"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("widths: left off, right 10.0%, top off, bottom off"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("Actions"), "stdout was: {stdout}");
    assert!(stdout.contains("command"), "stdout was: {stdout}");
    assert!(
        stdout.contains(&missing_action.display().to_string()),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("problem:"), "stdout was: {stdout}");
    assert!(stdout.contains("right.up"), "stdout was: {stdout}");
    assert!(stdout.contains("Summary"), "stdout was: {stdout}");
    assert!(
        stdout.contains("result: problems found"),
        "stdout was: {stdout}"
    );

    fs::remove_dir_all(root).expect("input root should be removed");
    fs::remove_file(config_path).expect("config should be removed");
}

#[test]
fn doctor_cli_uses_config_device_without_cli_override() {
    let root = unique_temp_dir("edgepad-doctor-cli-config-device-root");
    let config_path = unique_temp_path("edgepad-doctor-cli-config-device");
    let missing_device = unique_temp_path("edgepad-doctor-cli-configured-device");
    let missing_uinput = unique_temp_path("edgepad-doctor-cli-config-device-uinput");
    let action = unique_temp_executable("edgepad-doctor-cli-action");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&config_path);
    let _ = fs::remove_file(&missing_device);
    let _ = fs::remove_file(&missing_uinput);
    fs::create_dir_all(&root).expect("input root should be created");
    write_config(&config_path, &missing_device.display().to_string(), &action);

    let output = edgepad()
        .arg("doctor")
        .arg("--config")
        .arg(&config_path)
        .arg("--input-root")
        .arg(&root)
        .arg("--uinput")
        .arg(&missing_uinput)
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "doctor should report failures");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Config"), "stdout was: {stdout}");
    assert!(stdout.contains("device"), "stdout was: {stdout}");
    assert!(
        stdout.contains(&missing_device.display().to_string()),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("skipped because explicit device was provided"),
        "stdout was: {stdout}"
    );

    fs::remove_dir_all(root).expect("input root should be removed");
    fs::remove_file(config_path).expect("config should be removed");
    fs::remove_file(action).expect("action executable should be removed");
}

#[test]
fn doctor_cli_device_override_wins_over_config_device() {
    let root = unique_temp_dir("edgepad-doctor-cli-device-override-root");
    let config_path = unique_temp_path("edgepad-doctor-cli-device-override-config");
    let configured_device = unique_temp_path("edgepad-doctor-cli-configured-device");
    let override_device = unique_temp_path("edgepad-doctor-cli-override-device");
    let missing_uinput = unique_temp_path("edgepad-doctor-cli-device-override-uinput");
    let action = unique_temp_executable("edgepad-doctor-cli-override-action");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&config_path);
    let _ = fs::remove_file(&configured_device);
    let _ = fs::remove_file(&override_device);
    let _ = fs::remove_file(&missing_uinput);
    fs::create_dir_all(&root).expect("input root should be created");
    write_config(
        &config_path,
        &configured_device.display().to_string(),
        &action,
    );

    let output = edgepad()
        .arg("doctor")
        .arg("--config")
        .arg(&config_path)
        .arg("--device")
        .arg(&override_device)
        .arg("--input-root")
        .arg(&root)
        .arg("--uinput")
        .arg(&missing_uinput)
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "doctor should report failures");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("overridden by --device"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains(&configured_device.display().to_string()),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains(&override_device.display().to_string()),
        "stdout was: {stdout}"
    );

    fs::remove_dir_all(root).expect("input root should be removed");
    fs::remove_file(config_path).expect("config should be removed");
    fs::remove_file(action).expect("action executable should be removed");
}

fn write_config(path: &Path, device: &str, action: &Path) {
    fs::write(
        path,
        format!(
            r#"
device = "{device}"
edge_width = 0.10

[[gestures]]
zone = "right"
direction = "up"
action = ["{}"]
"#,
            action.display()
        ),
    )
    .expect("config should be written");
}

fn unique_temp_executable(prefix: &str) -> PathBuf {
    let path = unique_temp_path(prefix);
    fs::write(&path, "#!/bin/sh\n").expect("action executable should be written");
    let mut permissions = fs::metadata(&path)
        .expect("metadata should be available")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("permissions should be updated");
    path
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

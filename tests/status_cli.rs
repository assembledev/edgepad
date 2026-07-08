use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn status_cli_summarizes_config_zones_actions_and_device_state() {
    let root = unique_temp_dir("edgepad-status-cli-input-root");
    let config_path = unique_temp_path("edgepad-status-cli-config");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&config_path);
    fs::create_dir_all(&root).expect("input root should be created");
    write_status_config(&config_path, "auto");

    let output = edgepad()
        .arg("status")
        .arg("--config")
        .arg(&config_path)
        .arg("--input-root")
        .arg(&root)
        .output()
        .expect("edgepad binary should run");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("edgepad status"), "stdout was: {stdout}");
    assert!(stdout.contains("Service"), "stdout was: {stdout}");
    assert!(stdout.contains("Config"), "stdout was: {stdout}");
    assert!(
        stdout.contains(&config_path.display().to_string()),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Device   auto failed: no event devices"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Zones    right active; left, top, bottom passthrough"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("widths: left off, right 10.0%, top off, bottom off"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Actions  1 gesture binding, 0 sliders, 1 command"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Result   misconfigured"),
        "stdout was: {stdout}"
    );

    fs::remove_dir_all(root).expect("input root should be removed");
    fs::remove_file(config_path).expect("config should be removed");
}

#[test]
fn status_cli_loads_default_config_path_from_xdg_config_home() {
    let root = unique_temp_dir("edgepad-status-cli-default-root");
    let config_home = unique_temp_dir("edgepad-status-cli-config-home");
    let config_path = config_home.join("edgepad/edgepad.toml");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&config_home);
    fs::create_dir_all(&root).expect("input root should be created");
    write_status_config(&config_path, "auto");

    let output = edgepad()
        .env("XDG_CONFIG_HOME", &config_home)
        .arg("status")
        .arg("--input-root")
        .arg(&root)
        .output()
        .expect("edgepad binary should run");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&config_path.display().to_string()),
        "stdout was: {stdout}"
    );

    fs::remove_dir_all(root).expect("input root should be removed");
    fs::remove_dir_all(config_home).expect("config home should be removed");
}

#[test]
fn status_cli_rejects_unknown_option() {
    let output = edgepad()
        .arg("status")
        .arg("--wat")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown status option --wat"),
        "stderr was: {stderr}"
    );
}

fn write_status_config(path: &Path, device: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("config parent should be created");
    }
    fs::write(
        path,
        format!(
            r#"
device = "{device}"
edge_width = 0.10

[[gestures]]
zone = "right"
direction = "up"
action = ["notify-send", "edgepad", "right-up"]
"#,
        ),
    )
    .expect("config should be written");
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

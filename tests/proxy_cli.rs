use std::path::PathBuf;
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn proxy_cli_requires_frames_and_mode() {
    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg("/dev/input/event-does-not-matter")
        .arg("--dry-run")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "proxy without --frames should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("edgepad proxy --frames N (--dry-run | --uinput --grab)"),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_rejects_zero_frame_limit_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-zero-frame-limit");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("0")
        .arg("--dry-run")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "zero frame limit should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--frames must be a positive integer"),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_requires_mode_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-no-mode");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "proxy should require an explicit mode"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("proxy requires either --dry-run or --uinput --grab"),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_rejects_uinput_without_grab_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-uinput-without-grab");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .arg("--uinput")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "--uinput without --grab should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("proxy --uinput requires --grab"),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains("failed to open device"),
        "mode validation should happen before device open, stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_rejects_grab_without_uinput_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-grab-without-uinput");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .arg("--grab")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "--grab without --uinput should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("proxy --grab requires --uinput"),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains("failed to open device"),
        "mode validation should happen before device open, stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_accepts_uinput_grab_arguments_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-uinput-grab");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .arg("--uinput")
        .arg("--grab")
        .arg("--built-in-defaults")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "proxy should still fail for missing device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "--uinput --grab should parse before device open, stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&missing_device.display().to_string()),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_rejects_no_grab_mode() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-no-grab");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .arg("--uinput")
        .arg("--no-grab")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "--no-grab should not exist");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown proxy option --no-grab"),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_accepts_dry_run_arguments_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-dry-run");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .arg("--dry-run")
        .arg("--built-in-defaults")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "proxy should still fail for missing device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "--dry-run should parse before device open, stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&missing_device.display().to_string()),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_accepts_edge_width_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-edge-width");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .arg("--edge-width")
        .arg("0.20")
        .arg("--dry-run")
        .arg("--built-in-defaults")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "proxy should still fail for missing device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "--edge-width should parse before device open, stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_rejects_invalid_edge_width_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-bad-edge-width");
    let _ = std::fs::remove_file(&missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--device")
        .arg(&missing_device)
        .arg("--frames")
        .arg("2")
        .arg("--edge-width")
        .arg("0.80")
        .arg("--dry-run")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "bad edge width should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--edge-width must be > 0 and < 0.5"),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains("failed to open device"),
        "edge-width validation should happen before device open, stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_requires_device_with_built_in_defaults() {
    let output = edgepad()
        .arg("proxy")
        .arg("--frames")
        .arg("2")
        .arg("--dry-run")
        .arg("--built-in-defaults")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "missing device should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("proxy --built-in-defaults requires --device"),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_uses_config_device_and_recognition_profile() {
    let config_path = unique_temp_path("edgepad-proxy-config-profile.toml");
    let missing_device = unique_temp_path("edgepad-proxy-config-device");
    let _ = std::fs::remove_file(&config_path);
    let _ = std::fs::remove_file(&missing_device);
    write_proxy_config(&config_path, &missing_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--config")
        .arg(&config_path)
        .arg("--frames")
        .arg("2")
        .arg("--edge-width")
        .arg("0.22")
        .arg("--dry-run")
        .output()
        .expect("edgepad binary should run");

    std::fs::remove_file(&config_path).expect("config should be removed");

    assert!(
        !output.status.success(),
        "proxy should still fail for missing configured device"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains(&format!("profile: config {}", config_path.display())),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("edge_widths=left=0.220 right=0.000 top=0.000 bottom=0.000"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("sliders=1"), "stdout was: {stdout}");
    assert!(
        stdout.contains("tap_timing=available"),
        "stdout was: {stdout}"
    );
    assert!(
        stderr.contains(&missing_device.display().to_string()),
        "stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_device_override_wins_over_config_device() {
    let config_path = unique_temp_path("edgepad-proxy-device-override-config.toml");
    let configured_device = unique_temp_path("edgepad-proxy-configured-device");
    let override_device = unique_temp_path("edgepad-proxy-override-device");
    let _ = std::fs::remove_file(&config_path);
    let _ = std::fs::remove_file(&configured_device);
    let _ = std::fs::remove_file(&override_device);
    write_proxy_config(&config_path, &configured_device);

    let output = edgepad()
        .arg("proxy")
        .arg("--config")
        .arg(&config_path)
        .arg("--device")
        .arg(&override_device)
        .arg("--frames")
        .arg("2")
        .arg("--dry-run")
        .output()
        .expect("edgepad binary should run");

    std::fs::remove_file(&config_path).expect("config should be removed");

    assert!(
        !output.status.success(),
        "missing override device should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&override_device.display().to_string()),
        "stderr was: {stderr}"
    );
    assert!(
        !stderr.contains(&configured_device.display().to_string()),
        "CLI device should replace config device, stderr was: {stderr}"
    );
}

#[test]
fn proxy_cli_rejects_config_with_built_in_defaults() {
    let output = edgepad()
        .arg("proxy")
        .arg("--config")
        .arg("/tmp/edgepad.toml")
        .arg("--built-in-defaults")
        .arg("--frames")
        .arg("2")
        .arg("--dry-run")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "conflicting profiles should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("proxy --config and --built-in-defaults are mutually exclusive"),
        "stderr was: {stderr}"
    );
}

fn write_proxy_config(path: &std::path::Path, device: &std::path::Path) {
    std::fs::write(
        path,
        format!(
            r#"
device = "{}"
edge_width = 0.17
tap_min_duration_ms = 120
swipe_min_distance = 0.03

[[sliders]]
zone = "left"
step = 0.05
up = ["notify-send", "up"]
down = ["notify-send", "down"]
"#,
            device.display()
        ),
    )
    .expect("config should be written");
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

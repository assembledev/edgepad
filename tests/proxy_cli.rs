use std::path::PathBuf;
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn proxy_cli_requires_device_frames_and_dry_run() {
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
        stderr.contains("edgepad proxy --device <event-node> --frames N --dry-run"),
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
fn proxy_cli_requires_dry_run_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-proxy-device-no-dry-run");
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
        "proxy should require --dry-run for now"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("proxy currently requires --dry-run"),
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

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

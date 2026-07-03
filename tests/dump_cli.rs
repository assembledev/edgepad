use std::path::PathBuf;
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn dump_cli_requires_device_and_out_arguments() {
    let output = edgepad()
        .arg("dump")
        .arg("--device")
        .arg("/dev/input/event-does-not-matter")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "dump without --out should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw]"),
        "stderr was: {stderr}"
    );
}

#[test]
fn dump_cli_accepts_positive_frames_argument_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-device-with-frame-limit");
    let out_path = unique_temp_path("edgepad-missing-device-with-frame-limit-output.ev");
    let _ = std::fs::remove_file(&missing_device);
    let _ = std::fs::remove_file(&out_path);

    let output = edgepad()
        .arg("dump")
        .arg("--device")
        .arg(&missing_device)
        .arg("--out")
        .arg(&out_path)
        .arg("--frames")
        .arg("2")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "dump should still fail for missing device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "--frames should parse before device open, stderr was: {stderr}"
    );
    assert!(
        !out_path.exists(),
        "dump should open the input device before creating the output file"
    );
}

#[test]
fn dump_cli_rejects_zero_frame_limit_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-device-zero-frame-limit");
    let out_path = unique_temp_path("edgepad-missing-device-zero-frame-limit-output.ev");
    let _ = std::fs::remove_file(&missing_device);
    let _ = std::fs::remove_file(&out_path);

    let output = edgepad()
        .arg("dump")
        .arg("--device")
        .arg(&missing_device)
        .arg("--out")
        .arg(&out_path)
        .arg("--frames")
        .arg("0")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "zero frame limit should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--frames must be a positive integer"),
        "stderr was: {stderr}"
    );
    assert!(
        !out_path.exists(),
        "invalid CLI args must not create output file"
    );
}

#[test]
fn dump_cli_reports_missing_device_without_creating_output_file() {
    let missing_device = unique_temp_path("edgepad-missing-device");
    let out_path = unique_temp_path("edgepad-missing-device-output.ev");
    let _ = std::fs::remove_file(&missing_device);
    let _ = std::fs::remove_file(&out_path);

    let output = edgepad()
        .arg("dump")
        .arg("--device")
        .arg(&missing_device)
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "dump should fail for missing device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains(&missing_device.display().to_string()),
        "stderr was: {stderr}"
    );
    assert!(
        !out_path.exists(),
        "dump should open the input device before creating the output file"
    );
}

#[test]
fn dump_cli_accepts_raw_flag_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-device-raw-dump");
    let out_path = unique_temp_path("edgepad-missing-device-raw-dump-output.ev");
    let _ = std::fs::remove_file(&missing_device);
    let _ = std::fs::remove_file(&out_path);

    let output = edgepad()
        .arg("dump")
        .arg("--raw")
        .arg("--device")
        .arg(&missing_device)
        .arg("--out")
        .arg(&out_path)
        .arg("--frames")
        .arg("2")
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "dump should still fail for missing device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to open device"),
        "--raw should parse before device open, stderr was: {stderr}"
    );
    assert!(
        !out_path.exists(),
        "dump should open the input device before creating the output file"
    );
}

#[test]
fn dump_cli_rejects_unknown_dump_flag_before_device_open() {
    let missing_device = unique_temp_path("edgepad-missing-device-bad-flag");
    let out_path = unique_temp_path("edgepad-missing-device-bad-flag-output.ev");
    let _ = std::fs::remove_file(&missing_device);
    let _ = std::fs::remove_file(&out_path);

    let output = edgepad()
        .arg("dump")
        .arg("--device")
        .arg(&missing_device)
        .arg("--out")
        .arg(&out_path)
        .arg("--wat")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "unknown dump flag should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw]"),
        "stderr was: {stderr}"
    );
    assert!(
        !out_path.exists(),
        "invalid CLI args must not create output file"
    );
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

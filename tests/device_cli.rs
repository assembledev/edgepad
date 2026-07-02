use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn devices_cli_handles_empty_input_root_without_touching_real_hardware() {
    let root = unique_temp_dir("edgepad-devices-cli-empty");
    fs::create_dir_all(&root).expect("temp root should be created");

    let output = edgepad()
        .arg("devices")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("edgepad binary should run");

    fs::remove_dir_all(&root).expect("temp root should be removed");

    assert_no_devices_success(output);
}

#[test]
fn devices_cli_handles_missing_input_root_without_touching_real_hardware() {
    let root = unique_temp_dir("edgepad-devices-cli-missing");
    let _ = fs::remove_dir_all(&root);

    let output = edgepad()
        .arg("devices")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("edgepad binary should run");

    assert_no_devices_success(output);
}

#[test]
fn devices_cli_reports_unreadable_event_nodes_as_permission_hint() {
    let root = unique_temp_dir("edgepad-devices-cli-unreadable");
    fs::create_dir_all(&root).expect("temp root should be created");
    fs::write(root.join("event0"), b"not an evdev device")
        .expect("fake event node should be created");

    let output = edgepad()
        .arg("devices")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("edgepad binary should run");

    fs::remove_dir_all(&root).expect("temp root should be removed");

    assert!(
        output.status.success(),
        "devices command should still succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no readable event devices found"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("1 event node"), "stdout was: {stdout}");
    assert!(stdout.contains("try sudo"), "stdout was: {stdout}");
}

fn assert_no_devices_success(output: std::process::Output) {
    assert!(
        output.status.success(),
        "devices command should succeed for empty/missing input root, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no event devices found"),
        "stdout was: {stdout}"
    );
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn replay_cli_prints_summary_for_valid_fixture() {
    let output = edgepad()
        .arg("replay")
        .arg(fixture("left-edge-swipe-right.ev"))
        .output()
        .expect("edgepad binary should run");

    assert!(
        output.status.success(),
        "expected replay command to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("frames: 3"), "stdout was: {stdout}");
    assert!(
        stdout.contains("passthrough_events: 0"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("gestures: 1"), "stdout was: {stdout}");
    assert!(
        stdout.contains("gesture slot=0 tracking_id=123 zone=left direction=right"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("resync_required: false"),
        "stdout was: {stdout}"
    );
}

#[test]
fn replay_cli_uses_metadata_capabilities_when_present() {
    let path = unique_temp_path("edgepad-replay-metadata-slot-range.ev");
    fs::write(
        &path,
        r#"
# slots: 0..=0
# x: 0..=1000
# y: 0..=700

ABS_MT_SLOT 1
ABS_MT_TRACKING_ID 123
ABS_MT_POSITION_X 500
ABS_MT_POSITION_Y 300
SYN_REPORT
"#,
    )
    .expect("metadata fixture should be written");

    let output = edgepad()
        .arg("replay")
        .arg(&path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&path).expect("metadata fixture should be removed");

    assert!(
        !output.status.success(),
        "replay should fail because metadata limits slots to 0..=0"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("SlotOutOfRange"), "stderr was: {stderr}");
    assert!(stderr.contains("min: 0"), "stderr was: {stderr}");
    assert!(stderr.contains("max: 0"), "stderr was: {stderr}");
}

#[test]
fn replay_cli_returns_nonzero_for_engine_error() {
    let output = edgepad()
        .arg("replay")
        .arg(fixture("duplicate-tracking-id.ev"))
        .output()
        .expect("edgepad binary should run");

    assert!(
        !output.status.success(),
        "expected replay command to fail for duplicate tracking id"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("replay failed"), "stderr was: {stderr}");
    assert!(stderr.contains("SlotAlreadyActive"), "stderr was: {stderr}");
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

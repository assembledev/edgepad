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
fn replay_raw_cli_summarizes_composed_output_without_forwarding_raw_legacy_globals() {
    let path = unique_temp_path("edgepad-raw-replay-mixed.raw.ev");
    fs::write(
        &path,
        r#"
# edgepad .ev dump
# slots: 0..=4
# x: 0..=1000
# y: 0..=700

EV_KEY BTN_TOUCH 1
EV_ABS ABS_X 20
EV_ABS ABS_Y 300
EV_ABS ABS_MT_SLOT 0
EV_ABS ABS_MT_TRACKING_ID 100
EV_ABS ABS_MT_POSITION_X 20
EV_ABS ABS_MT_POSITION_Y 300
EV_ABS ABS_MT_SLOT 1
EV_ABS ABS_MT_TRACKING_ID 200
EV_ABS ABS_MT_POSITION_X 520
EV_ABS ABS_MT_POSITION_Y 320
EV_SYN SYN_REPORT 0
"#,
    )
    .expect("raw fixture should be written");

    let output = edgepad()
        .arg("replay-raw")
        .arg(&path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&path).expect("raw fixture should be removed");

    assert!(
        output.status.success(),
        "expected raw replay command to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("raw_frames: 1"), "stdout was: {stdout}");
    assert!(
        stdout.contains("raw_events: total=11"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("recognizer_passthrough_events: 4"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("composed_events: 11"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("gestures: 0"), "stdout was: {stdout}");
    assert!(
        stdout.contains("resync_required: false"),
        "stdout was: {stdout}"
    );
}

#[test]
fn replay_raw_cli_includes_final_release_when_capture_ends_mid_passthrough_contact() {
    let path = unique_temp_path("edgepad-raw-replay-active-passthrough-at-end.raw.ev");
    fs::write(
        &path,
        r#"
# edgepad .ev dump
# slots: 0..=4
# x: 0..=4000
# y: 0..=2500

EV_ABS ABS_MT_TRACKING_ID 123
EV_ABS ABS_MT_POSITION_X 1200
EV_ABS ABS_MT_POSITION_Y 900
EV_SYN SYN_REPORT 0
"#,
    )
    .expect("raw fixture should be written");

    let output = edgepad()
        .arg("replay-raw")
        .arg(&path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&path).expect("raw fixture should be removed");

    assert!(
        output.status.success(),
        "expected raw replay command to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("raw_frames: 1"), "stdout was: {stdout}");
    assert!(
        stdout.contains("recognizer_passthrough_events: 3"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("composed_events: 10"),
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
fn replay_cli_explains_capture_without_contact_start() {
    let path = unique_temp_path("edgepad-replay-midstream-capture.ev");
    fs::write(
        &path,
        r#"
# slots: 0..=4
# x: 0..=4000
# y: 0..=2500

ABS_MT_POSITION_X 1200
ABS_MT_POSITION_Y 900
SYN_REPORT
"#,
    )
    .expect("midstream fixture should be written");

    let output = edgepad()
        .arg("replay")
        .arg(&path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&path).expect("midstream fixture should be removed");

    assert!(
        output.status.success(),
        "expected replay command to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("events: total=2"), "stdout was: {stdout}");
    assert!(
        stdout.contains("contacts: started=0 ended=0"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("diagnosis: no contact starts found"),
        "stdout was: {stdout}"
    );
}

#[test]
fn replay_cli_explains_frame_budget_stopping_mid_contact_without_bad_lift_hint() {
    let path = unique_temp_path("edgepad-replay-active-at-end.ev");
    fs::write(
        &path,
        r#"
# slots: 0..=4
# x: 0..=4000
# y: 0..=2500

ABS_MT_TRACKING_ID 123
ABS_MT_POSITION_X 1200
ABS_MT_POSITION_Y 900
SYN_REPORT
"#,
    )
    .expect("active-at-end fixture should be written");

    let output = edgepad()
        .arg("replay")
        .arg(&path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&path).expect("active-at-end fixture should be removed");

    assert!(
        output.status.success(),
        "expected replay command to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("diagnosis: capture ended with active contact(s); frame budget likely stopped mid-contact"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("diagnosis_hint: for edge gesture captures, perform the gesture, release it, then place a finger in the center until --frames finishes"),
        "stdout was: {stdout}"
    );
    assert!(
        !stdout.contains("lift fingers before capture stops"),
        "stdout was: {stdout}"
    );
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

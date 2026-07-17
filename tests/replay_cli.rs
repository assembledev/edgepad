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
        .arg("--built-in-defaults")
        .output()
        .expect("edgepad binary should run");

    assert!(
        output.status.success(),
        "expected replay command to succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("profile: built-in defaults"),
        "stdout was: {stdout}"
    );
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
EV_SYN SYN_REPORT 0 0
"#,
    )
    .expect("raw fixture should be written");

    let output = edgepad()
        .arg("replay-raw")
        .arg(&path)
        .arg("--built-in-defaults")
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
EV_SYN SYN_REPORT 0 0
"#,
    )
    .expect("raw fixture should be written");

    let output = edgepad()
        .arg("replay-raw")
        .arg(&path)
        .arg("--built-in-defaults")
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
        stdout.contains("recognizer_passthrough_events: 4"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("composed_events: 11"),
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
SYN_REPORT 0
"#,
    )
    .expect("metadata fixture should be written");

    let output = edgepad()
        .arg("replay")
        .arg(&path)
        .arg("--built-in-defaults")
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
SYN_REPORT 0
"#,
    )
    .expect("midstream fixture should be written");

    let output = edgepad()
        .arg("replay")
        .arg(&path)
        .arg("--built-in-defaults")
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
SYN_REPORT 0
"#,
    )
    .expect("active-at-end fixture should be written");

    let output = edgepad()
        .arg("replay")
        .arg(&path)
        .arg("--built-in-defaults")
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
        .arg("--built-in-defaults")
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

#[test]
fn replay_cli_loads_default_config_profile_and_edge_width() {
    let config_home = unique_temp_path("edgepad-replay-default-config-home");
    let config_path = config_home.join("edgepad/edgepad.toml");
    let _ = fs::remove_dir_all(&config_home);
    write_config(
        &config_path,
        r#"
edge_width = 0.01

[[gestures]]
zone = "left"
direction = "right"
action = { log = true }
"#,
    );

    let output = edgepad()
        .arg("replay")
        .arg(fixture("left-edge-swipe-right.ev"))
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("edgepad binary should run");

    fs::remove_dir_all(&config_home).expect("config home should be removed");

    assert!(
        output.status.success(),
        "config replay should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("profile: config {}", config_path.display())),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("edge_widths=left=0.010 right=0.000 top=0.000 bottom=0.000"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("gestures: 0"), "stdout was: {stdout}");
    assert!(
        !stdout.contains("zone=left direction=right"),
        "narrow configured edge should leave the fixture contact in passthrough: {stdout}"
    );
}

#[test]
fn replay_cli_uses_configured_swipe_threshold() {
    let config_path = unique_temp_path("edgepad-replay-swipe-config.toml");
    let _ = fs::remove_file(&config_path);
    write_config(
        &config_path,
        r#"
edge_width = 0.10
swipe_min_distance = 0.30

[[gestures]]
zone = "left"
direction = "tap"
action = { log = true }
"#,
    );

    let output = edgepad()
        .arg("replay")
        .arg(fixture("left-edge-swipe-right.ev"))
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&config_path).expect("config should be removed");

    assert!(
        output.status.success(),
        "config replay should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("swipe_min_distance=0.300"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("zone=left direction=tap"),
        "configured threshold should classify the movement as a tap: {stdout}"
    );
}

#[test]
fn replay_cli_applies_tap_min_duration_from_frame_timestamps() {
    let config_path = unique_temp_path("edgepad-replay-tap-duration-config.toml");
    let short_path = unique_temp_path("edgepad-replay-short-tap.ev");
    let accepted_path = unique_temp_path("edgepad-replay-accepted-tap.ev");
    for path in [&config_path, &short_path, &accepted_path] {
        let _ = fs::remove_file(path);
    }
    write_config(
        &config_path,
        r#"
edge_width = 0.10
tap_min_duration_ms = 80

[[gestures]]
zone = "left"
direction = "tap"
action = { log = true }
"#,
    );
    fs::write(&short_path, tap_fixture(79_999)).expect("short tap fixture should be written");
    fs::write(&accepted_path, tap_fixture(80_000)).expect("accepted tap fixture should be written");

    let short = edgepad()
        .arg("replay")
        .arg(&short_path)
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("edgepad binary should run");
    let accepted = edgepad()
        .arg("replay")
        .arg(&accepted_path)
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("edgepad binary should run");

    for path in [&config_path, &short_path, &accepted_path] {
        fs::remove_file(path).expect("temporary replay input should be removed");
    }

    assert!(
        short.status.success(),
        "short tap replay should succeed, stderr: {}",
        String::from_utf8_lossy(&short.stderr)
    );
    let short_stdout = String::from_utf8_lossy(&short.stdout);
    assert!(
        short_stdout.contains("gestures: 0"),
        "tap shorter than the configured duration must be rejected: {short_stdout}"
    );

    assert!(
        accepted.status.success(),
        "accepted tap replay should succeed, stderr: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let accepted_stdout = String::from_utf8_lossy(&accepted.stdout);
    assert!(
        accepted_stdout.contains("gesture slot=0 tracking_id=123 zone=left direction=tap"),
        "tap at the configured duration must be recognized: {accepted_stdout}"
    );
}

#[test]
fn replay_cli_never_executes_configured_actions() {
    let config_path = unique_temp_path("edgepad-replay-action-config.toml");
    let action_marker = unique_temp_path("edgepad-replay-action-marker");
    let _ = fs::remove_file(&config_path);
    let _ = fs::remove_file(&action_marker);
    write_config(
        &config_path,
        &format!(
            r#"
edge_width = 0.10

[[gestures]]
zone = "left"
direction = "right"
action = ["sh", "-c", "touch {}"]
"#,
            action_marker.display()
        ),
    );

    let output = edgepad()
        .arg("replay")
        .arg(fixture("left-edge-swipe-right.ev"))
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&config_path).expect("config should be removed");

    assert!(
        output.status.success(),
        "config replay should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !action_marker.exists(),
        "replay must recognize configured gestures without executing their actions"
    );
}

#[test]
fn replay_raw_cli_uses_configured_slider_profile() {
    let config_path = unique_temp_path("edgepad-replay-raw-slider-config.toml");
    let raw_path = unique_temp_path("edgepad-replay-raw-slider.raw.ev");
    let _ = fs::remove_file(&config_path);
    let _ = fs::remove_file(&raw_path);
    write_config(
        &config_path,
        r#"
edge_width = 0.10

[[sliders]]
zone = "left"
step = 0.10
up = ["notify-send", "up"]
down = ["notify-send", "down"]
"#,
    );
    fs::write(
        &raw_path,
        r#"
# slots: 0..=4
# x: 0..=1000
# y: 0..=1000

EV_ABS ABS_MT_SLOT 0
EV_ABS ABS_MT_TRACKING_ID 123
EV_ABS ABS_MT_POSITION_X 20
EV_ABS ABS_MT_POSITION_Y 700
EV_SYN SYN_REPORT 0 0

EV_ABS ABS_MT_SLOT 0
EV_ABS ABS_MT_POSITION_Y 200
EV_SYN SYN_REPORT 0 50000

EV_ABS ABS_MT_SLOT 0
EV_ABS ABS_MT_TRACKING_ID -1
EV_SYN SYN_REPORT 0 100000
"#,
    )
    .expect("raw fixture should be written");

    let output = edgepad()
        .arg("replay-raw")
        .arg(&raw_path)
        .arg("--config")
        .arg(&config_path)
        .output()
        .expect("edgepad binary should run");

    fs::remove_file(&config_path).expect("config should be removed");
    fs::remove_file(&raw_path).expect("raw fixture should be removed");

    assert!(
        output.status.success(),
        "raw config replay should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sliders=1"), "stdout was: {stdout}");
    assert!(
        stdout.contains("zone=left direction=up"),
        "configured slider should emit upward steps: {stdout}"
    );
}

#[test]
fn replay_cli_requires_explicit_built_in_profile_without_config() {
    let config_home = unique_temp_path("edgepad-replay-missing-config-home");
    let _ = fs::remove_dir_all(&config_home);
    fs::create_dir_all(&config_home).expect("config home should be created");

    let output = edgepad()
        .arg("replay")
        .arg(fixture("left-edge-swipe-right.ev"))
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .expect("edgepad binary should run");

    fs::remove_dir_all(&config_home).expect("config home should be removed");

    assert!(!output.status.success(), "missing config should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("recognition config not found"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("pass --config <file> or --built-in-defaults"),
        "stderr was: {stderr}"
    );
}

#[test]
fn replay_cli_rejects_config_with_built_in_defaults() {
    let output = edgepad()
        .arg("replay")
        .arg(fixture("left-edge-swipe-right.ev"))
        .arg("--config")
        .arg("/tmp/edgepad.toml")
        .arg("--built-in-defaults")
        .output()
        .expect("edgepad binary should run");

    assert!(!output.status.success(), "conflicting profiles should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("replay --config and --built-in-defaults are mutually exclusive"),
        "stderr was: {stderr}"
    );
}

fn write_config(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("config parent should be created");
    }
    fs::write(path, contents).expect("config should be written");
}

fn tap_fixture(released_at_us: u64) -> String {
    format!(
        "ABS_MT_SLOT 0\nABS_MT_TRACKING_ID 123\nABS_MT_POSITION_X 20\nABS_MT_POSITION_Y 300\nSYN_REPORT 0\n\nABS_MT_SLOT 0\nABS_MT_TRACKING_ID -1\nSYN_REPORT {released_at_us}\n"
    )
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
}

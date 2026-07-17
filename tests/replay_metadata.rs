use std::time::Duration;

use edgepad::core::{AxisRange, Capabilities, Event};
use edgepad::replay::{parse_replay_file, ReplayError, ReplayFrame};

#[test]
fn parse_replay_file_reads_capability_header_and_frames() {
    let replay = parse_replay_file(
        r#"
# edgepad .ev dump
# device: /dev/input/event5
# slots: 0..=4
# x: 10..=1210
# y: 20..=820

ABS_MT_SLOT 0
ABS_MT_TRACKING_ID 123
ABS_MT_POSITION_X 20
ABS_MT_POSITION_Y 300
SYN_REPORT 16000
"#,
    )
    .expect("metadata replay should parse");

    assert_eq!(
        replay.capabilities,
        Some(Capabilities {
            slot_min: 0,
            slot_max: 4,
            x: AxisRange { min: 10, max: 1210 },
            y: AxisRange { min: 20, max: 820 },
        })
    );
    assert_eq!(
        replay.frames,
        vec![ReplayFrame {
            events: vec![
                Event::slot(0),
                Event::tracking_id(123),
                Event::x(20),
                Event::y(300),
            ],
            timestamp: Duration::from_micros(16000),
        }]
    );
}

#[test]
fn parse_replay_file_keeps_fixtures_without_capability_metadata() {
    let replay = parse_replay_file(include_str!("fixtures/left-edge-swipe-right.ev"))
        .expect("fixture without capability metadata should still parse");

    assert_eq!(replay.capabilities, None);
    assert_eq!(replay.frames.len(), 3);
}

#[test]
fn parse_replay_file_rejects_partial_capability_metadata() {
    let err = parse_replay_file(
        r#"
# slots: 0..=4
ABS_MT_SLOT 0
SYN_REPORT 0
"#,
    )
    .expect_err("partial metadata should not silently fall back to defaults");

    assert_eq!(err, ReplayError::MissingMetadataField { field: "x" });
}

#[test]
fn parse_replay_file_rejects_invalid_metadata_range() {
    let err = parse_replay_file("# x: nope\n").expect_err("invalid metadata should be rejected");

    assert_eq!(
        err,
        ReplayError::InvalidMetadata {
            line: 1,
            name: "x".to_string(),
            value: "nope".to_string(),
        }
    );
}

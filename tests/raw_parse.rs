use std::time::Duration;

use edgepad::core::{AxisRange, Capabilities};
use edgepad::raw::{
    parse_raw_dump_file, parse_raw_frames, RawDumpFile, RawEvent, RawFrame, RawParseError, EV_ABS,
    EV_KEY, EV_MSC,
};

fn frame(timestamp_us: u64, events: Vec<RawEvent>) -> RawFrame {
    RawFrame::new_at(events, Duration::from_micros(timestamp_us))
}

#[test]
fn parse_raw_frames_reads_named_raw_dump_lines_and_splits_on_syn_report() {
    let input = r#"
# edgepad .ev dump
# device: /dev/input/event5
EV_KEY BTN_TOUCH 1
EV_ABS ABS_X 640
EV_ABS ABS_Y 320
EV_ABS ABS_MT_SLOT 1
EV_ABS ABS_MT_TRACKING_ID 200
EV_ABS ABS_MT_POSITION_X 500
EV_ABS ABS_MT_POSITION_Y 300
EV_MSC MSC_TIMESTAMP 16000
EV_SYN SYN_REPORT 0 16000

EV_ABS ABS_MT_SLOT 1
EV_ABS ABS_MT_POSITION_X 510
EV_ABS ABS_MT_POSITION_Y 310
EV_SYN SYN_REPORT 0 32000
"#;

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![
            frame(
                16000,
                vec![
                    RawEvent::btn_touch(true),
                    RawEvent::abs_x(640),
                    RawEvent::abs_y(320),
                    RawEvent::abs_mt_slot(1),
                    RawEvent::abs_mt_tracking_id(200),
                    RawEvent::abs_mt_position_x(500),
                    RawEvent::abs_mt_position_y(300),
                    RawEvent::msc_timestamp(16000),
                ]
            ),
            frame(
                32000,
                vec![
                    RawEvent::abs_mt_slot(1),
                    RawEvent::abs_mt_position_x(510),
                    RawEvent::abs_mt_position_y(310),
                ]
            ),
        ])
    );
}

#[test]
fn parse_raw_frames_preserves_numeric_fallback_events() {
    let input = "EV_KEY 65535 1\nEV_ABS 65535 42\nEV_MSC 5 16000\nEV_65535 65534 123\nEV_SYN SYN_REPORT 0 16000\n";

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![frame(
            16000,
            vec![
                RawEvent::new(EV_KEY, 65535, 1),
                RawEvent::new(EV_ABS, 65535, 42),
                RawEvent::new(EV_MSC, 5, 16000),
                RawEvent::new(65535, 65534, 123),
            ]
        )])
    );
}

#[test]
fn parse_raw_frames_preserves_syn_dropped_as_standalone_frame() {
    let input = "EV_ABS ABS_MT_SLOT 1\nEV_SYN SYN_DROPPED 0 16000\nEV_ABS ABS_MT_SLOT 0\nEV_SYN SYN_REPORT 0 32000\n";

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![
            frame(16000, vec![RawEvent::syn_dropped()]),
            frame(32000, vec![RawEvent::abs_mt_slot(0)]),
        ])
    );
}

#[test]
fn parse_raw_frames_ignores_comments_blank_lines_and_inline_comments() {
    let input = "# comment\n\nEV_ABS ABS_MT_SLOT 1 # inline\nEV_SYN SYN_REPORT 0 16000\n";

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![frame(16000, vec![RawEvent::abs_mt_slot(1)])])
    );
}

#[test]
fn parse_raw_frames_requires_timestamp_on_every_frame_boundary() {
    assert_eq!(
        parse_raw_frames("EV_ABS ABS_MT_SLOT 1\nEV_SYN SYN_REPORT 0\n"),
        Err(RawParseError::MissingField {
            line: 2,
            field: "timestamp_us",
        })
    );
}

#[test]
fn parse_raw_frames_rejects_non_monotonic_timestamps() {
    assert_eq!(
        parse_raw_frames(
            "EV_ABS ABS_MT_SLOT 1\nEV_SYN SYN_REPORT 0 20\nEV_ABS ABS_MT_SLOT 1\nEV_SYN SYN_REPORT 0 10\n"
        ),
        Err(RawParseError::NonMonotonicTimestamp {
            line: 4,
            previous_us: 20,
            current_us: 10,
        })
    );
}

#[test]
fn parse_raw_frames_rejects_unterminated_frame() {
    assert_eq!(
        parse_raw_frames("EV_ABS ABS_MT_SLOT 1\n"),
        Err(RawParseError::UnterminatedFrame { line: 1 })
    );
}

#[test]
fn parse_raw_frames_rejects_missing_value_with_line_number() {
    assert_eq!(
        parse_raw_frames("EV_ABS ABS_MT_SLOT\n"),
        Err(RawParseError::MissingField {
            line: 1,
            field: "value",
        })
    );
}

#[test]
fn parse_raw_frames_rejects_unknown_named_code_with_line_number() {
    assert_eq!(
        parse_raw_frames("EV_ABS ABS_MT_WHATEVER 1\n"),
        Err(RawParseError::UnknownCode {
            line: 1,
            event_type: "EV_ABS".to_string(),
            code: "ABS_MT_WHATEVER".to_string(),
        })
    );
}

#[test]
fn parse_raw_frames_rejects_invalid_numeric_value_with_line_number() {
    assert_eq!(
        parse_raw_frames("EV_KEY BTN_TOUCH nope\n"),
        Err(RawParseError::InvalidInteger {
            line: 1,
            field: "value",
            value: "nope".to_string(),
        })
    );
}

#[test]
fn parse_raw_dump_file_reads_capability_header_and_frames() {
    let input = r#"
# edgepad .ev dump
# device: /dev/input/event5
# slots: 0..=4
# x: 10..=1210
# y: 20..=820

EV_KEY BTN_TOUCH 1
EV_ABS ABS_MT_SLOT 1
EV_SYN SYN_REPORT 0 16000
"#;

    assert_eq!(
        parse_raw_dump_file(input),
        Ok(RawDumpFile {
            capabilities: Some(Capabilities {
                slot_min: 0,
                slot_max: 4,
                x: AxisRange { min: 10, max: 1210 },
                y: AxisRange { min: 20, max: 820 },
            }),
            frames: vec![frame(
                16000,
                vec![RawEvent::btn_touch(true), RawEvent::abs_mt_slot(1),]
            )],
        })
    );
}

#[test]
fn parse_raw_dump_file_accepts_capture_without_capability_metadata() {
    let input = "EV_ABS ABS_MT_SLOT 1\nEV_SYN SYN_REPORT 0 16000\n";

    assert_eq!(
        parse_raw_dump_file(input),
        Ok(RawDumpFile {
            capabilities: None,
            frames: vec![frame(16000, vec![RawEvent::abs_mt_slot(1)])],
        })
    );
}

#[test]
fn parse_raw_dump_file_rejects_partial_capability_metadata() {
    let input = "# slots: 0..=4\n# x: 10..=1210\nEV_SYN SYN_REPORT 0 0\n";

    assert_eq!(
        parse_raw_dump_file(input),
        Err(RawParseError::MissingMetadataField { field: "y" })
    );
}

#[test]
fn parse_raw_dump_file_rejects_invalid_capability_metadata_range() {
    let input = "# slots: nope\n# x: 10..=1210\n# y: 20..=820\n";

    assert_eq!(
        parse_raw_dump_file(input),
        Err(RawParseError::InvalidMetadataRange {
            line: 1,
            field: "slots",
            value: "nope".to_string(),
        })
    );
}

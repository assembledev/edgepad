use std::time::Duration;

use edgepad::core::{AxisRange, Capabilities, Event};
use edgepad::raw::{parse_raw_dump_file, RawParseError};
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

#[test]
fn replay_formats_reject_invalid_capability_ranges_with_matching_context() {
    for (index, field) in ["slots", "x", "y"].into_iter().enumerate() {
        for value in [
            "4..=0",
            "nope",
            "0..=2147483648",
            "-2147483649..=0",
            "1..=2..=3",
        ] {
            let mut ranges = ["0..=4", "10..=1210", "20..=820"];
            ranges[index] = value;
            let input = format!(
                "# capture metadata\n# slots: {}\n# x: {}\n# y: {}\n",
                ranges[0], ranges[1], ranges[2]
            );
            let line = index + 2;

            assert_eq!(
                parse_replay_file(&input),
                Err(ReplayError::InvalidMetadata {
                    line,
                    name: field.to_string(),
                    value: value.to_string(),
                }),
                "parsed replay should reject {field}={value}"
            );
            assert_eq!(
                parse_raw_dump_file(&input),
                Err(RawParseError::InvalidMetadataRange {
                    line,
                    field,
                    value: value.to_string(),
                }),
                "raw replay should reject {field}={value}"
            );
        }
    }
}

#[test]
fn replay_formats_preserve_valid_capability_headers() {
    let input = "\
# unrelated: ignored
   # slots: 3..=5
# x: -1000..=1000
# y: 42..=42
# x: -500..=1500
";
    let expected = Some(Capabilities {
        slot_min: 3,
        slot_max: 5,
        x: AxisRange {
            min: -500,
            max: 1500,
        },
        y: AxisRange { min: 42, max: 42 },
    });

    assert_eq!(
        parse_replay_file(input)
            .expect("valid parsed header should load")
            .capabilities,
        expected
    );
    assert_eq!(
        parse_raw_dump_file(input)
            .expect("valid raw header should load")
            .capabilities,
        expected
    );
}

#[test]
fn replay_formats_require_all_fields_when_capability_metadata_is_present() {
    for (input, field) in [
        ("# x: 10..=1210\n# y: 20..=820\n", "slots"),
        ("# slots: 0..=4\n# y: 20..=820\n", "x"),
        ("# slots: 0..=4\n# x: 10..=1210\n", "y"),
    ] {
        assert_eq!(
            parse_replay_file(input),
            Err(ReplayError::MissingMetadataField { field })
        );
        assert_eq!(
            parse_raw_dump_file(input),
            Err(RawParseError::MissingMetadataField { field })
        );
    }
}

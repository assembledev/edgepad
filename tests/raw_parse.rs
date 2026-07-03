use edgepad::raw::{parse_raw_frames, RawEvent, RawFrame, RawParseError, EV_ABS, EV_KEY};

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
EV_SYN SYN_REPORT 0

EV_ABS ABS_MT_SLOT 1
EV_ABS ABS_MT_POSITION_X 510
EV_ABS ABS_MT_POSITION_Y 310
EV_SYN SYN_REPORT 0
"#;

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![
            RawFrame::new(vec![
                RawEvent::btn_touch(true),
                RawEvent::abs_x(640),
                RawEvent::abs_y(320),
                RawEvent::abs_mt_slot(1),
                RawEvent::abs_mt_tracking_id(200),
                RawEvent::abs_mt_position_x(500),
                RawEvent::abs_mt_position_y(300),
            ]),
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(1),
                RawEvent::abs_mt_position_x(510),
                RawEvent::abs_mt_position_y(310),
            ]),
        ])
    );
}

#[test]
fn parse_raw_frames_preserves_numeric_fallback_events() {
    let input = "EV_KEY 65535 1\nEV_ABS 65535 42\nEV_65535 65534 123\nEV_SYN SYN_REPORT 0\n";

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![RawFrame::new(vec![
            RawEvent::new(EV_KEY, 65535, 1),
            RawEvent::new(EV_ABS, 65535, 42),
            RawEvent::new(65535, 65534, 123),
        ])])
    );
}

#[test]
fn parse_raw_frames_preserves_syn_dropped_as_standalone_frame() {
    let input =
        "EV_ABS ABS_MT_SLOT 1\nEV_SYN SYN_DROPPED 0\nEV_ABS ABS_MT_SLOT 0\nEV_SYN SYN_REPORT 0\n";

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![
            RawFrame::new(vec![RawEvent::abs_mt_slot(1)]),
            RawFrame::new(vec![RawEvent::syn_dropped()]),
            RawFrame::new(vec![RawEvent::abs_mt_slot(0)]),
        ])
    );
}

#[test]
fn parse_raw_frames_ignores_comments_blank_lines_and_inline_comments() {
    let input = "# comment\n\nEV_ABS ABS_MT_SLOT 1 # inline\nEV_SYN SYN_REPORT 0\n";

    assert_eq!(
        parse_raw_frames(input),
        Ok(vec![RawFrame::new(vec![RawEvent::abs_mt_slot(1)])])
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

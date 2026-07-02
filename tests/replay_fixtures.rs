use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine, Event, GestureDirection, Zone};
use edgepad::replay::{parse_frames, run_frames, ReplayError};

fn test_caps() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 1,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

#[test]
fn parses_left_edge_swipe_fixture_into_frames() {
    let frames = parse_frames(include_str!("fixtures/left-edge-swipe-right.ev"))
        .expect("fixture should parse");

    assert_eq!(
        frames,
        vec![
            vec![
                Event::slot(0),
                Event::tracking_id(123),
                Event::x(20),
                Event::y(300),
            ],
            vec![Event::slot(0), Event::x(220), Event::y(310)],
            vec![Event::slot(0), Event::tracking_id(-1)],
        ]
    );
}

#[test]
fn parsed_fixture_drives_engine_to_left_swipe_right_without_passthrough() {
    let frames = parse_frames(include_str!("fixtures/left-edge-swipe-right.ev"))
        .expect("fixture should parse");
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    let outputs = run_frames(&mut engine, &frames).expect("fixture should run through engine");

    let passthrough: Vec<_> = outputs
        .iter()
        .flat_map(|output| output.passthrough.iter().copied())
        .collect();
    let gestures: Vec<_> = outputs
        .iter()
        .flat_map(|output| output.gestures.iter().copied())
        .collect();

    assert!(passthrough.is_empty());
    assert_eq!(gestures.len(), 1);
    assert_eq!(gestures[0].zone, Zone::Left);
    assert_eq!(gestures[0].direction, GestureDirection::Right);
}

#[test]
fn parser_ignores_comments_blank_lines_and_inline_comments() {
    let input = r#"
# full line comment
ABS_MT_SLOT 1 # inline comment
ABS_MT_TRACKING_ID 7
ABS_MT_POSITION_X 500
ABS_MT_POSITION_Y 300
SYN_REPORT
"#;

    let frames = parse_frames(input).expect("comments should be ignored");

    assert_eq!(
        frames,
        vec![vec![
            Event::slot(1),
            Event::tracking_id(7),
            Event::x(500),
            Event::y(300),
        ]]
    );
}

#[test]
fn parser_rejects_unknown_event_with_line_number() {
    let err = parse_frames("ABS_MT_POSITION_Z 1\n").expect_err("unknown event should fail");

    assert_eq!(
        err,
        ReplayError::UnknownEvent {
            line: 1,
            name: "ABS_MT_POSITION_Z".to_string(),
        }
    );
}

#[test]
fn parser_rejects_missing_value_with_line_number() {
    let err = parse_frames("ABS_MT_SLOT\n").expect_err("missing value should fail");

    assert_eq!(
        err,
        ReplayError::MissingValue {
            line: 1,
            name: "ABS_MT_SLOT".to_string(),
        }
    );
}

#[test]
fn parser_rejects_invalid_integer_value_with_line_number() {
    let err = parse_frames("ABS_MT_SLOT nope\n").expect_err("invalid integer should fail");

    assert_eq!(
        err,
        ReplayError::InvalidValue {
            line: 1,
            name: "ABS_MT_SLOT".to_string(),
            value: "nope".to_string(),
        }
    );
}

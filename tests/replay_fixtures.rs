use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, Engine, Event, FrameOutput, Gesture, GestureDirection,
    SlotError, Zone,
};
use edgepad::replay::{parse_frames, run_frames, ReplayError};

fn test_caps() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 1,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

fn run_fixture(input: &str) -> Result<Vec<FrameOutput>, SlotError> {
    let frames = parse_frames(input).expect("fixture should parse");
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    run_frames(&mut engine, &frames)
}

fn collect_passthrough(outputs: &[FrameOutput]) -> Vec<Event> {
    outputs
        .iter()
        .flat_map(|output| output.passthrough.iter().copied())
        .collect()
}

fn collect_gestures(outputs: &[FrameOutput]) -> Vec<Gesture> {
    outputs
        .iter()
        .flat_map(|output| output.gestures.iter().copied())
        .collect()
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
    let outputs = run_fixture(include_str!("fixtures/left-edge-swipe-right.ev"))
        .expect("fixture should run through engine");

    let passthrough = collect_passthrough(&outputs);
    let gestures = collect_gestures(&outputs);

    assert!(passthrough.is_empty());
    assert_eq!(gestures.len(), 1);
    assert_eq!(gestures[0].zone, Zone::Left);
    assert_eq!(gestures[0].direction, GestureDirection::Right);
}

#[test]
fn center_touch_fixture_passes_through_from_down_to_release() {
    let outputs = run_fixture(include_str!("fixtures/center-touch-passthrough.ev"))
        .expect("center touch fixture should run through engine");

    assert_eq!(
        collect_passthrough(&outputs),
        vec![
            Event::slot(1),
            Event::tracking_id(200),
            Event::x(500),
            Event::y(300),
            Event::slot(1),
            Event::x(510),
            Event::y(310),
            Event::slot(1),
            Event::tracking_id(-1),
        ]
    );
    assert!(collect_gestures(&outputs).is_empty());
}

#[test]
fn mixed_edge_and_center_fixture_claims_only_edge_slot() {
    let outputs = run_fixture(include_str!("fixtures/mixed-edge-and-center.ev"))
        .expect("mixed fixture should run through engine");

    assert_eq!(
        collect_passthrough(&outputs),
        vec![
            Event::slot(1),
            Event::tracking_id(200),
            Event::x(500),
            Event::y(300),
            Event::slot(1),
            Event::x(510),
            Event::y(310),
            Event::slot(1),
            Event::tracking_id(-1),
        ]
    );

    let gestures = collect_gestures(&outputs);
    assert_eq!(gestures.len(), 1);
    assert_eq!(gestures[0].slot, 0);
    assert_eq!(gestures[0].zone, Zone::Left);
    assert_eq!(gestures[0].direction, GestureDirection::Right);
}

#[test]
fn duplicate_tracking_id_fixture_is_rejected() {
    let err = run_fixture(include_str!("fixtures/duplicate-tracking-id.ev"))
        .expect_err("duplicate tracking id without release must be rejected");

    assert_eq!(
        err,
        SlotError::SlotAlreadyActive {
            slot: 0,
            active_tracking_id: 300,
            new_tracking_id: 301,
        }
    );
}

#[test]
fn syn_dropped_fixture_clears_state_and_allows_fresh_contact() {
    let outputs = run_fixture(include_str!("fixtures/syn-dropped-reset.ev"))
        .expect("SYN_DROPPED fixture should recover after reset");

    assert_eq!(outputs.len(), 3);
    assert!(outputs[1].resync_required);
    assert!(outputs[1].passthrough.is_empty());
    assert!(outputs[1].gestures.is_empty());

    assert_eq!(
        outputs[2].passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(401),
            Event::x(500),
            Event::y(300),
        ]
    );
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

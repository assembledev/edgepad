use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine, Event, FrameOutput};
use edgepad::replay::{parse_frames, run_frames};

fn test_caps() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 1,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

fn run_fixture(input: &str) -> Vec<FrameOutput> {
    let frames = parse_frames(input).expect("fixture should parse");
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    run_frames(&mut engine, &frames).expect("fixture should run through engine")
}

fn passthrough_frames(outputs: &[FrameOutput]) -> Vec<Vec<Event>> {
    outputs
        .iter()
        .filter_map(|output| (!output.passthrough.is_empty()).then_some(output.passthrough.clone()))
        .collect()
}

#[test]
fn claimed_edge_touch_produces_no_passthrough_frames() {
    let outputs = run_fixture(include_str!("fixtures/left-edge-swipe-right.ev"));

    assert!(passthrough_frames(&outputs).is_empty());
}

#[test]
fn center_touch_passthrough_preserves_frame_boundaries() {
    let outputs = run_fixture(include_str!("fixtures/center-touch-passthrough.ev"));

    assert_eq!(
        passthrough_frames(&outputs),
        vec![
            vec![
                Event::slot(1),
                Event::tracking_id(200),
                Event::x(500),
                Event::y(300),
            ],
            vec![Event::slot(1), Event::x(510), Event::y(310)],
            vec![Event::slot(1), Event::tracking_id(-1)],
        ]
    );
}

#[test]
fn mixed_edge_and_center_passthrough_preserves_only_center_frames() {
    let outputs = run_fixture(include_str!("fixtures/mixed-edge-and-center.ev"));

    assert_eq!(
        passthrough_frames(&outputs),
        vec![
            vec![
                Event::slot(1),
                Event::tracking_id(200),
                Event::x(500),
                Event::y(300),
            ],
            vec![Event::slot(1), Event::x(510), Event::y(310)],
            vec![Event::slot(1), Event::tracking_id(-1)],
        ]
    );
}

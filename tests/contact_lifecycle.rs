use std::time::Duration;

use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, Engine, Event, GestureDirection, SliderAxis, SliderSpec,
    Zone,
};

fn engine(slider: bool) -> Engine {
    Engine::with_sliders(
        Capabilities {
            slot_min: 3,
            slot_max: 4,
            x: AxisRange { min: 0, max: 1000 },
            y: AxisRange { min: 0, max: 700 },
        },
        EdgeWidths::all(0.1),
        if slider {
            vec![SliderSpec {
                zone: Zone::Left,
                axis: SliderAxis::Vertical,
                step: 0.1,
            }]
        } else {
            Vec::new()
        },
    )
}

#[test]
fn coordinates_before_or_after_tracking_id_resolve_the_same_contact() {
    let events = [Event::tracking_id(10), Event::x(500), Event::y(300)];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut engine = engine(false);
        let mut frame = vec![Event::slot(3)];
        frame.extend(order.map(|i| events[i]));
        let down = engine.process_frame(&frame).unwrap();
        assert_eq!(down.passthrough, frame, "order: {order:?}");
        let up = engine.process_frame(&[Event::tracking_id(-1)]).unwrap();
        assert_eq!(up.passthrough, [Event::slot(3), Event::tracking_id(-1)]);
        assert!(up.gestures.is_empty());
    }
}

#[test]
fn ownership_waits_for_missing_coordinates_across_frames() {
    let mut engine = engine(false);
    let first = [Event::slot(3), Event::tracking_id(10), Event::x(500)];
    assert!(engine.process_frame(&first).unwrap().passthrough.is_empty());
    let second = [Event::slot(3), Event::y(300)];
    let output = engine.process_frame(&second).unwrap();
    assert_eq!(
        output.passthrough,
        [first.as_slice(), second.as_slice()].concat()
    );
    assert!(output.gestures.is_empty());
}

#[test]
fn incomplete_contact_discards_held_input_on_lift() {
    let mut engine = engine(false);
    engine
        .process_frame(&[Event::slot(3), Event::tracking_id(10), Event::x(500)])
        .unwrap();
    let up = engine.process_frame(&[Event::tracking_id(-1)]).unwrap();
    assert!(up.passthrough.is_empty());
    assert!(up.gestures.is_empty());

    let frame = [Event::tracking_id(11), Event::y(300)];
    let down = engine.process_frame(&frame).unwrap();
    assert_eq!(down.passthrough, [Event::slot(3), frame[0], frame[1]]);
}

#[test]
fn slot_reuse_in_a_release_frame_keeps_contacts_and_output_slots_separate() {
    let mut engine = engine(false);
    engine
        .process_frame(&[Event::tracking_id(10), Event::x(20), Event::y(300)])
        .unwrap();
    engine.process_frame(&[Event::x(220)]).unwrap();
    let output = engine
        .process_frame(&[
            Event::tracking_id(-1),
            Event::tracking_id(11),
            Event::x(500),
            Event::y(300),
        ])
        .unwrap();
    assert_eq!(output.gestures.len(), 1);
    assert_eq!(output.gestures[0].tracking_id, 10);
    assert_eq!(output.gestures[0].direction, GestureDirection::Right);
    assert_eq!(
        output.passthrough,
        [
            Event::slot(3),
            Event::tracking_id(11),
            Event::x(500),
            Event::y(300),
        ]
    );

    let output = engine
        .process_frame(&[Event::tracking_id(-1), Event::tracking_id(12), Event::x(20)])
        .unwrap();
    assert_eq!(output.passthrough, [Event::slot(3), Event::tracking_id(-1)]);
    let up = engine.process_frame(&[Event::tracking_id(-1)]).unwrap();
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].tracking_id, 12);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn slider_consumption_does_not_survive_lift() {
    let mut engine = engine(true);
    engine
        .process_frame_at(
            &[Event::tracking_id(10), Event::x(20), Event::y(300)],
            Duration::ZERO,
        )
        .unwrap();
    let motion = engine
        .process_frame_at(&[Event::y(500)], Duration::from_millis(100))
        .unwrap();
    assert!(!motion.slider_steps.is_empty());
    let up = engine
        .process_frame_at(&[Event::tracking_id(-1)], Duration::from_millis(110))
        .unwrap();
    assert!(up.gestures.is_empty());

    engine
        .process_frame_at(&[Event::tracking_id(11)], Duration::from_millis(200))
        .unwrap();
    let up = engine
        .process_frame_at(&[Event::tracking_id(-1)], Duration::from_millis(300))
        .unwrap();
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].tracking_id, 11);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
    assert!(up.slider_steps.is_empty());
}

#[test]
fn idle_axis_updates_are_used_by_the_next_contact_without_starting_one() {
    let mut engine = engine(false);
    let idle = [Event::slot(4), Event::x(20), Event::y(300)];
    let output = engine.process_frame(&idle).unwrap();
    assert!(output.passthrough.is_empty());
    assert!(output.gestures.is_empty());
    let down = engine.process_frame(&[Event::tracking_id(10)]).unwrap();
    assert!(down.passthrough.is_empty());
    let up = engine.process_frame(&[Event::tracking_id(-1)]).unwrap();
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].slot, 4);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

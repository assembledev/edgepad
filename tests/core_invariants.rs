use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, Engine, Event, GestureDirection, SliderAxis,
    SliderDirection, SliderSpec, SlotError, Zone,
};

fn test_caps() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 1,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

#[test]
fn rejects_slots_outside_device_capability_range_instead_of_clamping() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.08));

    let err = engine
        .process_frame(&[Event::slot(2), Event::tracking_id(10)])
        .expect_err("slot 2 is outside 0..=1 and must not be clamped");

    assert_eq!(
        err,
        SlotError::SlotOutOfRange {
            slot: 2,
            min: 0,
            max: 1
        }
    );
}

#[test]
fn rejects_duplicate_tracking_id_without_release_for_same_slot() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.08));

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(10),
            Event::x(500),
            Event::y(300),
        ])
        .expect("first contact starts cleanly");

    let err = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(11)])
        .expect_err("a slot cannot start twice without TRACKING_ID=-1");

    assert_eq!(
        err,
        SlotError::SlotAlreadyActive {
            slot: 0,
            active_tracking_id: 10,
            new_tracking_id: 11
        }
    );
}

#[test]
fn claimed_edge_touch_does_not_emit_initial_down_to_passthrough() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");

    assert_eq!(down.passthrough, Vec::<Event>::new());
    assert!(down.gestures.is_empty());

    let move_frame = engine
        .process_frame(&[Event::slot(0), Event::x(220), Event::y(310)])
        .expect("claimed movement remains internal");
    assert_eq!(move_frame.passthrough, Vec::<Event>::new());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("release emits gesture only");

    assert_eq!(up.passthrough, Vec::<Event>::new());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Right);
}

#[test]
fn slider_edge_touch_emits_steps_during_motion_without_release_swipe() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(22),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");

    assert!(down.slider_steps.is_empty());
    assert!(down.gestures.is_empty());

    let move_frame = engine
        .process_frame(&[Event::slot(0), Event::y(510)])
        .expect("slider motion emits steps");

    assert_eq!(move_frame.slider_steps.len(), 3);
    for step in &move_frame.slider_steps {
        assert_eq!(step.zone, Zone::Left);
        assert_eq!(step.direction, SliderDirection::Down);
        assert_eq!(step.slot, 0);
        assert_eq!(step.tracking_id, 22);
    }
    assert!(move_frame.gestures.is_empty());
    assert!(move_frame.passthrough.is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("slider release is valid");

    assert!(up.slider_steps.is_empty());
    assert!(up.gestures.is_empty());
    assert!(up.passthrough.is_empty());
}

#[test]
fn slider_edge_touch_still_emits_tap_on_release() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(23),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("slider tap release is valid");

    assert!(up.slider_steps.is_empty());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn inactive_edge_width_passes_edge_touch_through() {
    let mut engine = Engine::new(
        test_caps(),
        EdgeWidths {
            left: 0.0,
            right: 0.10,
            top: 0.0,
            bottom: 0.0,
        },
    );

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(21),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");

    assert_eq!(
        down.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(21),
            Event::x(20),
            Event::y(300)
        ]
    );
    assert!(down.gestures.is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("inactive edge release passes through");

    assert_eq!(up.passthrough, vec![Event::slot(0), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn center_touch_is_passthrough_from_initial_down_to_release() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    let down = engine
        .process_frame(&[
            Event::slot(1),
            Event::tracking_id(30),
            Event::x(500),
            Event::y(300),
        ])
        .expect("center touch starts cleanly");

    assert_eq!(
        down.passthrough,
        vec![
            Event::slot(1),
            Event::tracking_id(30),
            Event::x(500),
            Event::y(300)
        ]
    );

    let up = engine
        .process_frame(&[Event::slot(1), Event::tracking_id(-1)])
        .expect("center touch release passes through");

    assert_eq!(up.passthrough, vec![Event::slot(1), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn syn_dropped_clears_state_and_requires_explicit_resync() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(40),
            Event::x(500),
            Event::y(300),
        ])
        .expect("touch starts cleanly");

    let dropped = engine
        .process_frame(&[Event::syn_dropped()])
        .expect("SYN_DROPPED is represented as resync-required output, not ignored");

    assert!(dropped.resync_required);
    assert!(dropped.passthrough.is_empty());
    assert!(dropped.gestures.is_empty());

    let restarted = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(41),
            Event::x(500),
            Event::y(300),
        ])
        .expect("state was cleared after SYN_DROPPED");

    assert!(!restarted.resync_required);
}

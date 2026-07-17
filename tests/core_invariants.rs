use std::time::Duration;

use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, Engine, EngineOptions, Event, GestureDirection,
    ResyncContact, SliderAxis, SliderDirection, SliderSpec, SlotError, Zone,
};
use edgepad::raw::BTN_LEFT;

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
fn buttonpad_press_promotes_claimed_contact_and_cancels_edge_gesture() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");
    assert!(down.passthrough.is_empty());

    assert_eq!(
        engine.update_physical_button(BTN_LEFT, true),
        vec![
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ]
    );

    let motion = engine
        .process_frame(&[Event::slot(0), Event::x(220), Event::y(310)])
        .expect("promoted contact motion passes through");
    assert_eq!(
        motion.passthrough,
        vec![Event::slot(0), Event::x(220), Event::y(310)]
    );
    assert!(motion.gestures.is_empty());

    assert!(engine.update_physical_button(BTN_LEFT, false).is_empty());
    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("promoted contact release passes through");
    assert_eq!(up.passthrough, vec![Event::slot(0), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn buttonpad_press_promotes_all_claimed_contacts_for_clickfinger() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
            Event::slot(1),
            Event::tracking_id(21),
            Event::x(980),
            Event::y(320),
        ])
        .expect("two edge contacts are valid");

    assert_eq!(
        engine.update_physical_button(BTN_LEFT, true),
        vec![
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
            Event::slot(1),
            Event::tracking_id(21),
            Event::x(980),
            Event::y(320),
        ]
    );
}

#[test]
fn button_held_on_buttonpad_forces_new_edge_contact_to_passthrough() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);
    assert!(engine.update_physical_button(BTN_LEFT, true).is_empty());

    let down = engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("edge contact while button is held is valid");
    assert_eq!(
        down.passthrough,
        vec![
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ]
    );

    assert!(engine.update_physical_button(BTN_LEFT, false).is_empty());
    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("contact remains passthrough after button release");
    assert_eq!(up.passthrough, vec![Event::slot(0), Event::tracking_id(-1)]);
    assert!(up.gestures.is_empty());
}

#[test]
fn physical_button_on_non_buttonpad_does_not_preempt_edge_ownership() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");
    assert!(engine.update_physical_button(BTN_LEFT, true).is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("separate button does not cancel edge gesture");
    assert!(up.passthrough.is_empty());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn buttonpad_tap_without_hardware_button_remains_edge_gesture() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine.set_buttonpad(true);

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(20),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left edge contact is valid");
    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("tap release is valid");

    assert!(up.passthrough.is_empty());
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
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
fn buttonpad_press_keeps_emitted_slider_steps_but_cancels_future_steps_and_gesture() {
    let mut engine = Engine::with_sliders(
        test_caps(),
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );
    engine.set_buttonpad(true);

    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(22),
            Event::x(20),
            Event::y(300),
        ])
        .expect("left slider contact is valid");
    let before_click = engine
        .process_frame(&[Event::slot(0), Event::y(510)])
        .expect("slider motion emits steps before click");
    assert_eq!(before_click.slider_steps.len(), 3);

    assert_eq!(
        engine.update_physical_button(BTN_LEFT, true),
        vec![
            Event::slot(0),
            Event::tracking_id(22),
            Event::x(20),
            Event::y(510),
        ]
    );
    let after_click = engine
        .process_frame(&[Event::slot(0), Event::y(650)])
        .expect("motion after click passes through");
    assert_eq!(after_click.passthrough, vec![Event::slot(0), Event::y(650)]);
    assert!(after_click.slider_steps.is_empty());

    let up = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("promoted slider contact releases cleanly");
    assert!(up.slider_steps.is_empty());
    assert!(up.gestures.is_empty());
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
fn short_edge_touch_does_not_emit_tap_when_timing_is_known() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::from_millis(80),
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(24),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1079),
        )
        .expect("short tap release is valid");

    assert!(up.gestures.is_empty());
    assert!(up.passthrough.is_empty());
}

#[test]
fn edge_touch_at_tap_min_duration_emits_tap() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::from_millis(80),
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(25),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1080),
        )
        .expect("tap release is valid");

    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn zero_tap_min_duration_allows_immediate_tap() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::ZERO,
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(26),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1000),
        )
        .expect("immediate tap release is valid");

    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].zone, Zone::Left);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn short_edge_swipe_still_emits_directional_gesture() {
    let mut engine = Engine::with_options(
        test_caps(),
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            tap_min_duration: Duration::from_millis(80),
            ..EngineOptions::default()
        },
    );

    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(27),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("left edge contact is valid");
    engine
        .process_frame_at(
            &[Event::slot(0), Event::x(220), Event::y(310)],
            Duration::from_millis(1030),
        )
        .expect("claimed movement remains internal");

    let up = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1040),
        )
        .expect("short swipe release is valid");

    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::Right);
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

#[test]
fn restored_contact_is_passthrough_until_release_even_when_it_is_on_an_edge() {
    let mut engine = Engine::new(test_caps(), EdgeWidths::all(0.10));
    engine
        .process_frame(&[Event::syn_dropped()])
        .expect("SYN_DROPPED should reset the engine");

    let restored = engine
        .restore_passthrough_contacts(&[ResyncContact {
            slot: 1,
            tracking_id: 42,
            x: 20,
            y: 300,
        }])
        .expect("kernel snapshot should restore the active contact");

    assert_eq!(
        restored.passthrough,
        vec![
            Event::slot(1),
            Event::tracking_id(42),
            Event::x(20),
            Event::y(300),
        ]
    );
    assert!(restored.gestures.is_empty());

    let released = engine
        .process_frame(&[Event::slot(1), Event::tracking_id(-1)])
        .expect("restored contact release should pass through");
    assert_eq!(
        released.passthrough,
        vec![Event::slot(1), Event::tracking_id(-1)]
    );
    assert!(released.gestures.is_empty());
}

#[test]
fn gesture_distance_is_invariant_across_touchpad_coordinate_ranges() {
    let direction_for = |x_max: i32, start_x: i32, end_x: i32| {
        let mut engine = Engine::new(
            Capabilities {
                x: AxisRange { min: 0, max: x_max },
                ..test_caps()
            },
            EdgeWidths::all(0.10),
        );
        engine
            .process_frame(&[
                Event::slot(0),
                Event::tracking_id(50),
                Event::x(start_x),
                Event::y(300),
            ])
            .expect("contact should start");
        engine
            .process_frame(&[Event::slot(0), Event::x(end_x), Event::y(300)])
            .expect("contact should move");
        engine
            .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
            .expect("contact should release")
            .gestures[0]
            .direction
    };

    assert_eq!(direction_for(1000, 10, 25), GestureDirection::Tap);
    assert_eq!(direction_for(2000, 20, 50), GestureDirection::Tap);
}

#[test]
fn gesture_direction_compares_normalized_axis_travel() {
    let mut engine = Engine::new(
        Capabilities {
            x: AxisRange { min: 0, max: 2000 },
            y: AxisRange { min: 0, max: 1000 },
            ..test_caps()
        },
        EdgeWidths::all(0.10),
    );
    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(51),
            Event::x(20),
            Event::y(500),
        ])
        .expect("contact should start");
    engine
        .process_frame(&[Event::slot(0), Event::x(220), Event::y(650)])
        .expect("contact should move");
    let released = engine
        .process_frame(&[Event::slot(0), Event::tracking_id(-1)])
        .expect("contact should release");

    assert_eq!(released.gestures[0].direction, GestureDirection::Down);
}

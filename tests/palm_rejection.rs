use std::time::Duration;

use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, Engine, EngineOptions, GestureDirection, SliderAxis,
    SliderSpec, Zone, ZoneSet,
};
use edgepad::raw::{route_raw_frame, RawEvent, RawFrame, ABS_MT_TOOL_TYPE, EV_ABS};

fn caps() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 4,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

fn engine(slider: bool, double_tap: bool) -> Engine {
    Engine::with_options(
        caps(),
        EdgeWidths::all(0.1),
        if slider {
            vec![SliderSpec {
                zone: Zone::Left,
                axis: SliderAxis::Vertical,
                step: 0.1,
            }]
        } else {
            vec![]
        },
        EngineOptions {
            double_tap_zones: if double_tap {
                ZoneSet::all()
            } else {
                ZoneSet::default()
            },
            ..EngineOptions::default()
        },
    )
}

fn tool(value: i32) -> RawEvent {
    RawEvent::new(EV_ABS, ABS_MT_TOOL_TYPE, value)
}

fn down(id: i32, x: i32, y: i32) -> Vec<RawEvent> {
    vec![
        RawEvent::abs_mt_tracking_id(id),
        RawEvent::abs_mt_position_x(x),
        RawEvent::abs_mt_position_y(y),
    ]
}

fn route(engine: &mut Engine, ms: u64, events: Vec<RawEvent>) -> edgepad::raw::RoutedRawFrame {
    route_raw_frame(
        engine,
        &RawFrame {
            events,
            timestamp: Some(Duration::from_millis(ms)),
        },
    )
    .unwrap()
}

#[test]
fn palm_never_emits_release_gesture() {
    let mut engine = engine(false, false);
    let mut events = down(10, 20, 300);
    events.push(tool(2));
    route(&mut engine, 1000, events);
    route(&mut engine, 1050, vec![RawEvent::abs_mt_position_x(220)]);
    let up = route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)]);
    assert!(up.gestures.is_empty(), "palm must not emit a swipe");
    assert!(
        up.passthrough.is_empty(),
        "classification must not promote an edge contact"
    );
}

#[test]
fn late_palm_suppresses_slider_steps_even_after_coordinates_in_same_frame() {
    let mut engine = engine(true, false);
    route(&mut engine, 1000, down(11, 20, 300));
    let output = route(
        &mut engine,
        1050,
        vec![RawEvent::abs_mt_position_y(500), tool(2)],
    );
    assert!(
        output.slider_steps.is_empty(),
        "palm classification applies to the complete frame"
    );
    assert!(
        route(&mut engine, 1100, vec![RawEvent::abs_mt_position_y(650)])
            .slider_steps
            .is_empty()
    );
    assert!(
        route(&mut engine, 1150, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .is_empty()
    );
}

#[test]
fn palm_cancellation_lasts_until_lift_but_resets_for_next_finger() {
    let mut engine = engine(false, false);
    route(&mut engine, 1000, down(12, 20, 300));
    route(&mut engine, 1020, vec![tool(2)]);
    route(&mut engine, 1030, vec![tool(0)]);
    assert!(
        route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .is_empty()
    );
    route(&mut engine, 1200, down(13, 20, 300));
    let up = route(&mut engine, 1300, vec![RawEvent::abs_mt_tracking_id(-1)]);
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn reused_slot_retains_palm_type_until_finger_update_in_new_frame() {
    let mut engine = engine(false, false);
    let mut events = down(14, 20, 300);
    events.push(tool(2));
    route(&mut engine, 1000, events);
    route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)]);
    route(&mut engine, 1200, down(15, 20, 300));
    assert!(
        route(&mut engine, 1300, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .is_empty()
    );
    let mut events = down(16, 20, 300);
    events.push(tool(0));
    route(&mut engine, 1400, events);
    assert_eq!(
        route(&mut engine, 1500, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .len(),
        1
    );
}

#[test]
fn palm_second_contact_does_not_complete_or_flush_first_tap() {
    let mut engine = engine(false, true);
    route(&mut engine, 1000, down(17, 20, 300));
    assert!(
        route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .is_empty()
    );
    route(&mut engine, 1200, down(18, 20, 300));
    assert!(route(&mut engine, 1220, vec![tool(2)]).gestures.is_empty());
    assert!(
        route(&mut engine, 1300, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .is_empty()
    );
    let expired = engine.advance_time(Duration::from_millis(1400));
    assert_eq!(expired.gestures.len(), 1);
    assert_eq!(expired.gestures[0].tracking_id, 17);
    assert_eq!(expired.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn center_palm_metadata_is_forwarded() {
    let mut engine = engine(false, false);
    let mut events = down(19, 500, 300);
    events.push(tool(2));
    let output = route(&mut engine, 1000, events);
    assert!(output.passthrough.contains(&tool(2)));
}

#[test]
fn palm_flag_before_contact_start_and_after_coordinates_have_same_result() {
    for before_start in [true, false] {
        let mut engine = engine(true, false);
        let mut events = down(20, 20, 300);
        if before_start {
            events.insert(0, tool(2));
        } else {
            events.push(tool(2));
        }
        assert!(route(&mut engine, 1000, events).slider_steps.is_empty());
        let moved = route(&mut engine, 1050, vec![RawEvent::abs_mt_position_y(600)]);
        assert!(moved.slider_steps.is_empty());
        assert!(
            route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)])
                .gestures
                .is_empty()
        );
    }
}

#[test]
fn palm_label_in_release_frame_cancels_swipe() {
    let mut engine = engine(false, false);
    route(&mut engine, 1000, down(21, 20, 300));
    let output = route(
        &mut engine,
        1100,
        vec![
            RawEvent::abs_mt_position_x(220),
            tool(2),
            RawEvent::abs_mt_tracking_id(-1),
        ],
    );
    assert!(output.gestures.is_empty());
}

#[test]
fn palm_on_another_slot_does_not_interrupt_finger_double_tap() {
    let mut engine = engine(false, true);
    route(&mut engine, 1000, down(22, 20, 300));
    route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)]);
    let mut events = vec![RawEvent::abs_mt_slot(1)];
    events.extend(down(23, 500, 300));
    events.push(tool(2));
    assert!(route(&mut engine, 1150, events).gestures.is_empty());
    let mut events = vec![RawEvent::abs_mt_slot(0)];
    events.extend(down(24, 20, 300));
    route(&mut engine, 1200, events);
    let up = route(&mut engine, 1300, vec![RawEvent::abs_mt_tracking_id(-1)]);
    assert_eq!(up.gestures.len(), 1);
    assert_eq!(up.gestures[0].direction, GestureDirection::DoubleTap);
    assert_eq!(up.gestures[0].tracking_id, 24);
}

#[test]
fn same_frame_slot_reuse_keeps_old_palm_and_new_finger_separate() {
    let mut engine = engine(false, false);
    route(&mut engine, 1000, down(25, 20, 300));
    let mut events = vec![tool(2), RawEvent::abs_mt_tracking_id(-1)];
    events.extend(down(26, 20, 300));
    events.push(tool(0));
    assert!(route(&mut engine, 1100, events).gestures.is_empty());
    let output = route(&mut engine, 1200, vec![RawEvent::abs_mt_tracking_id(-1)]);
    assert_eq!(output.gestures.len(), 1);
    assert_eq!(output.gestures[0].tracking_id, 26);
}

#[test]
fn new_palm_does_not_cancel_finger_released_from_same_slot_in_same_frame() {
    let mut engine = engine(false, false);
    route(&mut engine, 1000, down(27, 20, 300));
    let mut events = vec![RawEvent::abs_mt_tracking_id(-1)];
    events.extend(down(28, 20, 300));
    events.push(tool(2));
    let output = route(&mut engine, 1100, events);
    assert_eq!(output.gestures.len(), 1);
    assert_eq!(output.gestures[0].tracking_id, 27);
    assert!(
        route(&mut engine, 1200, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .is_empty()
    );
}

#[test]
fn passthrough_start_and_click_promotion_include_retained_tool_type() {
    use edgepad::raw::{BTN_LEFT, EV_KEY};
    let mut engine = engine(false, false);
    engine.set_buttonpad(true);
    let mut events = down(29, 20, 300);
    events.push(tool(2));
    route(&mut engine, 1000, events);
    let click = route(&mut engine, 1050, vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]);
    assert!(click.passthrough.contains(&tool(2)));
    assert!(click.gestures.is_empty());
    route(
        &mut engine,
        1100,
        vec![
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::new(EV_KEY, BTN_LEFT, 0),
        ],
    );
    let center = route(&mut engine, 1200, down(30, 500, 300));
    assert!(
        center.passthrough.contains(&tool(2)),
        "a newly visible contact needs the retained label"
    );
}

#[test]
fn resync_restores_tool_type_and_new_finger_clears_virtual_palm_state() {
    use edgepad::core::ResyncContact;
    use edgepad::raw::{route_resync_contacts, RawOutputComposer};
    let mut engine = engine(false, false);
    let mut composer = RawOutputComposer::new(caps());
    let output = route_resync_contacts(
        &mut engine,
        &[ResyncContact {
            slot: 0,
            tracking_id: 31,
            x: 20,
            y: 300,
            tool_type: Some(2),
        }],
    )
    .unwrap();
    assert!(composer
        .compose_frame(&output)
        .unwrap()
        .events
        .contains(&tool(2)));
    let up = route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)]);
    assert!(up.gestures.is_empty());
    composer.compose_frame(&up).unwrap();
    // The physical type changes while the following edge contact stays hidden.
    let mut events = down(32, 20, 300);
    events.push(tool(0));
    route(&mut engine, 1200, events);
    route(&mut engine, 1300, vec![RawEvent::abs_mt_tracking_id(-1)]);
    let center = route(&mut engine, 1400, down(33, 500, 300));
    assert!(composer
        .compose_frame(&center)
        .unwrap()
        .events
        .contains(&tool(0)));
    route(&mut engine, 1500, vec![RawEvent::abs_mt_tracking_id(-1)]);
}

#[test]
fn slider_steps_before_detection_remain_valid_but_no_more_are_emitted() {
    let mut engine = engine(true, false);
    route(&mut engine, 1000, down(34, 20, 300));
    assert!(
        !route(&mut engine, 1050, vec![RawEvent::abs_mt_position_y(400)])
            .slider_steps
            .is_empty()
    );
    assert!(route(
        &mut engine,
        1100,
        vec![RawEvent::abs_mt_position_y(600), tool(2)]
    )
    .slider_steps
    .is_empty());
    assert!(route(
        &mut engine,
        1150,
        vec![tool(0), RawEvent::abs_mt_position_y(200)]
    )
    .slider_steps
    .is_empty());
}

#[test]
fn syn_dropped_recovery_replaces_stale_palm_classification() {
    use edgepad::core::ResyncContact;
    use edgepad::raw::route_resync_contacts;
    let mut engine = engine(false, false);
    let mut events = down(35, 20, 300);
    events.push(tool(2));
    route(&mut engine, 1000, events);
    assert!(route(&mut engine, 1020, vec![RawEvent::syn_dropped()]).resync_required);
    let restored = route_resync_contacts(
        &mut engine,
        &[ResyncContact {
            slot: 0,
            tracking_id: 36,
            x: 20,
            y: 300,
            tool_type: Some(0),
        }],
    )
    .unwrap();
    assert!(restored.passthrough.contains(&tool(0)));
    assert!(
        route(&mut engine, 1100, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .is_empty()
    );
    route(&mut engine, 1200, down(37, 20, 300));
    assert_eq!(
        route(&mut engine, 1300, vec![RawEvent::abs_mt_tracking_id(-1)])
            .gestures
            .len(),
        1
    );
}

#[test]
fn reduced_hardware_capture_rejects_late_immediate_and_inherited_palms() {
    use edgepad::raw::parse_raw_dump_file;
    let dump = parse_raw_dump_file(include_str!("fixtures/palm-classification.raw.ev")).unwrap();
    let caps = dump.capabilities.unwrap();
    let mut guarded = Engine::new(caps, EdgeWidths::all(0.1));
    let mut unlabelled = Engine::new(caps, EdgeWidths::all(0.1));
    let mut baseline_gestures = 0;
    for frame in &dump.frames {
        assert!(route_raw_frame(&mut guarded, frame)
            .unwrap()
            .gestures
            .is_empty());
        let mut frame = frame.clone();
        frame
            .events
            .retain(|event| !(event.kind == EV_ABS && event.code == ABS_MT_TOOL_TYPE));
        baseline_gestures += route_raw_frame(&mut unlabelled, &frame)
            .unwrap()
            .gestures
            .len();
    }
    assert!(
        baseline_gestures > 0,
        "the real contact movements exercise release recognition"
    );
}

#[test]
fn disabling_palm_rejection_matches_input_without_tool_labels() {
    use edgepad::raw::parse_raw_dump_file;
    let dump = parse_raw_dump_file(include_str!("fixtures/palm-classification.raw.ev")).unwrap();
    let caps = dump.capabilities.unwrap();
    for sliders in [
        vec![],
        vec![SliderSpec {
            zone: Zone::Right,
            axis: SliderAxis::Vertical,
            step: 0.01,
        }],
    ] {
        let mut disabled = Engine::with_options(
            caps,
            EdgeWidths::all(0.1),
            sliders.clone(),
            EngineOptions {
                palm_rejection: false,
                ..EngineOptions::default()
            },
        );
        let mut legacy = Engine::with_sliders(caps, EdgeWidths::all(0.1), sliders);
        for frame in &dump.frames {
            let output = route_raw_frame(&mut disabled, frame).unwrap();
            let mut unlabelled = frame.clone();
            unlabelled
                .events
                .retain(|event| !(event.kind == EV_ABS && event.code == ABS_MT_TOOL_TYPE));
            let expected = route_raw_frame(&mut legacy, &unlabelled).unwrap();
            assert_eq!(output, expected);
        }
    }
}

#[test]
fn disabled_palm_rejection_ignores_resync_labels() {
    use edgepad::core::ResyncContact;
    use edgepad::raw::route_resync_contacts;
    let mut engine = Engine::with_options(
        caps(),
        EdgeWidths::all(0.1),
        vec![],
        EngineOptions {
            palm_rejection: false,
            ..EngineOptions::default()
        },
    );
    let restored = route_resync_contacts(
        &mut engine,
        &[ResyncContact {
            slot: 0,
            tracking_id: 40,
            x: 20,
            y: 300,
            tool_type: Some(2),
        }],
    )
    .unwrap();
    assert!(!restored
        .passthrough
        .iter()
        .any(|event| event.code == ABS_MT_TOOL_TYPE && event.kind == EV_ABS));
    route(&mut engine, 1000, vec![RawEvent::abs_mt_tracking_id(-1)]);
    let mut events = down(41, 20, 300);
    events.push(tool(2));
    route(&mut engine, 1100, events);
    let up = route(&mut engine, 1200, vec![RawEvent::abs_mt_tracking_id(-1)]);
    assert_eq!(up.gestures.len(), 1);
}

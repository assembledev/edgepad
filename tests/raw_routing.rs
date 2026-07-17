use std::time::Duration;

use edgepad::core::{
    AxisRange, Capabilities, EdgeWidths, Engine, GestureDirection, ResyncContact, Zone,
};
use edgepad::raw::{
    route_raw_frame, route_resync_contacts, RawEvent, RawFrame, BTN_LEFT, BTN_TOUCH, EV_KEY,
};

fn test_engine() -> Engine {
    Engine::new(
        Capabilities {
            slot_min: 0,
            slot_max: 1,
            x: AxisRange { min: 0, max: 1000 },
            y: AxisRange { min: 0, max: 700 },
        },
        EdgeWidths::all(0.10),
    )
}

#[test]
fn route_raw_frame_returns_only_recognizer_raw_events_for_center_touch() {
    let mut engine = test_engine();
    let frame = RawFrame::new(vec![
        RawEvent::btn_touch(true),
        RawEvent::abs_mt_slot(1),
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::abs_x(640),
        RawEvent::abs_mt_position_x(500),
        RawEvent::abs_y(320),
        RawEvent::abs_mt_position_y(300),
    ]);

    let routed = route_raw_frame(&mut engine, &frame).expect("center frame should route");

    assert_eq!(
        routed.passthrough,
        vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]
    );
    assert!(routed.gestures.is_empty());
    assert!(!routed.resync_required);
}

#[test]
fn route_raw_frame_drops_claimed_edge_recognizer_raw_events() {
    let mut engine = test_engine();
    let frame = RawFrame::new(vec![
        RawEvent::btn_touch(true),
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(100),
        RawEvent::abs_mt_position_x(20),
        RawEvent::abs_mt_position_y(300),
    ]);

    let routed = route_raw_frame(&mut engine, &frame).expect("edge frame should route");

    assert!(routed.passthrough.is_empty());
    assert!(routed.gestures.is_empty());
    assert!(!routed.resync_required);
}

#[test]
fn route_raw_frame_preserves_physical_button_but_not_touch_state_key() {
    let mut engine = test_engine();
    let frame = RawFrame::new(vec![
        RawEvent::btn_touch(true),
        RawEvent::new(EV_KEY, BTN_LEFT, 1),
    ]);

    let routed = route_raw_frame(&mut engine, &frame).expect("button frame should route");

    assert!(routed.passthrough.is_empty());
    assert_eq!(
        routed.physical_buttons,
        vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]
    );
    assert!(!routed
        .physical_buttons
        .iter()
        .any(|event| event.code == BTN_TOUCH));
}

#[test]
fn buttonpad_press_promotes_claimed_edge_contact_before_physical_button() {
    let mut engine = test_engine();
    engine.set_buttonpad(true);

    let down = route_raw_frame(
        &mut engine,
        &RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]),
    )
    .expect("edge down should route");
    assert!(down.passthrough.is_empty());

    let press = route_raw_frame(
        &mut engine,
        &RawFrame::new(vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]),
    )
    .expect("physical button press should route");
    assert_eq!(
        press.passthrough,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]
    );
    assert_eq!(
        press.physical_buttons,
        vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]
    );
    assert!(press.gestures.is_empty());

    let release = route_raw_frame(
        &mut engine,
        &RawFrame::new(vec![
            RawEvent::new(EV_KEY, BTN_LEFT, 0),
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
        ]),
    )
    .expect("button and promoted contact release should route");
    assert_eq!(
        release.passthrough,
        vec![RawEvent::abs_mt_slot(0), RawEvent::abs_mt_tracking_id(-1),]
    );
    assert_eq!(
        release.physical_buttons,
        vec![RawEvent::new(EV_KEY, BTN_LEFT, 0)]
    );
    assert!(release.gestures.is_empty());
}

#[test]
fn buttonpad_contact_and_press_in_same_frame_start_as_passthrough() {
    let mut engine = test_engine();
    engine.set_buttonpad(true);

    let routed = route_raw_frame(
        &mut engine,
        &RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
            RawEvent::new(EV_KEY, BTN_LEFT, 1),
        ]),
    )
    .expect("same-frame edge contact and physical press should route");

    assert_eq!(
        routed.passthrough,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]
    );
    assert_eq!(
        routed.physical_buttons,
        vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]
    );
    assert!(routed.gestures.is_empty());
}

#[test]
fn route_raw_frame_keeps_only_center_slot_raw_events_in_mixed_frame() {
    let mut engine = test_engine();
    let frame = RawFrame::new(vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(100),
        RawEvent::abs_mt_position_x(20),
        RawEvent::abs_mt_position_y(300),
        RawEvent::abs_mt_slot(1),
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::abs_mt_position_x(500),
        RawEvent::abs_mt_position_y(300),
    ]);

    let routed = route_raw_frame(&mut engine, &frame).expect("mixed frame should route");

    assert_eq!(
        routed.passthrough,
        vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]
    );
    assert!(routed.gestures.is_empty());
    assert!(!routed.resync_required);
}

#[test]
fn route_raw_frame_returns_gesture_on_claimed_edge_release_without_passthrough() {
    let mut engine = test_engine();

    route_raw_frame(
        &mut engine,
        &RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]),
    )
    .expect("edge down should route");
    route_raw_frame(
        &mut engine,
        &RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_position_x(220),
            RawEvent::abs_mt_position_y(310),
        ]),
    )
    .expect("edge move should route");

    let routed = route_raw_frame(
        &mut engine,
        &RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
        ]),
    )
    .expect("edge release should route");

    assert!(routed.passthrough.is_empty());
    assert_eq!(routed.gestures.len(), 1);
    assert_eq!(routed.gestures[0].zone, Zone::Left);
    assert_eq!(routed.gestures[0].direction, GestureDirection::Right);
    assert!(!routed.resync_required);
}

#[test]
fn route_raw_frame_suppresses_short_timed_tap() {
    let mut engine = test_engine();

    route_raw_frame(
        &mut engine,
        &RawFrame::new_at(
            vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(101),
                RawEvent::abs_mt_position_x(20),
                RawEvent::abs_mt_position_y(300),
            ],
            Duration::from_millis(1000),
        ),
    )
    .expect("edge down should route");

    let routed = route_raw_frame(
        &mut engine,
        &RawFrame::new_at(
            vec![RawEvent::abs_mt_slot(0), RawEvent::abs_mt_tracking_id(-1)],
            Duration::from_millis(1079),
        ),
    )
    .expect("edge release should route");

    assert!(routed.passthrough.is_empty());
    assert!(routed.gestures.is_empty());
}

#[test]
fn route_raw_frame_reports_syn_dropped_without_emitting_raw_passthrough() {
    let mut engine = test_engine();
    let frame = RawFrame::new(vec![RawEvent::syn_dropped()]);

    let routed = route_raw_frame(&mut engine, &frame).expect("SYN_DROPPED should route");

    assert!(routed.passthrough.is_empty());
    assert!(routed.gestures.is_empty());
    assert!(routed.resync_required);
}

#[test]
fn route_resync_contacts_rebuilds_kernel_snapshot_as_passthrough() {
    let mut engine = test_engine();
    let routed = route_resync_contacts(
        &mut engine,
        &[ResyncContact {
            slot: 1,
            tracking_id: 200,
            x: 20,
            y: 300,
        }],
    )
    .expect("resync snapshot should route");

    assert_eq!(
        routed.passthrough,
        vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]
    );
    assert!(routed.gestures.is_empty());
    assert!(!routed.resync_required);
}

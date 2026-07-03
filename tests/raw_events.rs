use edgepad::core::Event;
use edgepad::raw::{
    extract_core_events, RawEvent, RawFrame, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_SLOT,
    ABS_MT_TRACKING_ID, ABS_X, ABS_Y, BTN_TOUCH, EV_ABS, EV_KEY, EV_SYN, SYN_DROPPED, SYN_REPORT,
};

#[test]
fn raw_event_constructors_use_linux_input_codes() {
    assert_eq!(
        RawEvent::abs_mt_slot(1),
        RawEvent::new(EV_ABS, ABS_MT_SLOT, 1)
    );
    assert_eq!(
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::new(EV_ABS, ABS_MT_TRACKING_ID, 200)
    );
    assert_eq!(
        RawEvent::abs_mt_position_x(500),
        RawEvent::new(EV_ABS, ABS_MT_POSITION_X, 500)
    );
    assert_eq!(
        RawEvent::abs_mt_position_y(300),
        RawEvent::new(EV_ABS, ABS_MT_POSITION_Y, 300)
    );
}

#[test]
fn raw_event_model_can_represent_non_recognizer_touchpad_events() {
    assert_eq!(
        RawEvent::btn_touch(true),
        RawEvent::new(EV_KEY, BTN_TOUCH, 1)
    );
    assert_eq!(
        RawEvent::btn_touch(false),
        RawEvent::new(EV_KEY, BTN_TOUCH, 0)
    );
    assert_eq!(RawEvent::abs_x(640), RawEvent::new(EV_ABS, ABS_X, 640));
    assert_eq!(RawEvent::abs_y(320), RawEvent::new(EV_ABS, ABS_Y, 320));
}

#[test]
fn raw_event_syn_constructors_represent_frame_boundaries_and_resync() {
    assert_eq!(RawEvent::syn_report(), RawEvent::new(EV_SYN, SYN_REPORT, 0));
    assert_eq!(
        RawEvent::syn_dropped(),
        RawEvent::new(EV_SYN, SYN_DROPPED, 0)
    );
}

#[test]
fn raw_frame_preserves_event_order_without_interpreting_events() {
    let frame = RawFrame::new(vec![
        RawEvent::abs_mt_slot(1),
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::btn_touch(true),
        RawEvent::abs_x(640),
        RawEvent::abs_y(320),
    ]);

    assert_eq!(
        frame.events,
        vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::btn_touch(true),
            RawEvent::abs_x(640),
            RawEvent::abs_y(320),
        ]
    );
}

#[test]
fn extract_core_events_keeps_recognizer_events_in_order() {
    let frame = RawFrame::new(vec![
        RawEvent::btn_touch(true),
        RawEvent::abs_mt_slot(1),
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::abs_x(640),
        RawEvent::abs_mt_position_x(500),
        RawEvent::abs_y(320),
        RawEvent::abs_mt_position_y(300),
    ]);

    assert_eq!(
        extract_core_events(&frame),
        vec![
            Event::slot(1),
            Event::tracking_id(200),
            Event::x(500),
            Event::y(300),
        ]
    );
}

#[test]
fn extract_core_events_ignores_syn_report_frame_boundary() {
    let frame = RawFrame::new(vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::syn_report(),
        RawEvent::abs_mt_position_x(123),
    ]);

    assert_eq!(
        extract_core_events(&frame),
        vec![Event::slot(0), Event::x(123)]
    );
}

#[test]
fn extract_core_events_preserves_syn_dropped_as_resync_signal() {
    let frame = RawFrame::new(vec![RawEvent::syn_dropped()]);

    assert_eq!(extract_core_events(&frame), vec![Event::syn_dropped()]);
}

#[test]
fn extract_core_events_ignores_unknown_or_non_multitouch_raw_events() {
    let frame = RawFrame::new(vec![
        RawEvent::btn_touch(true),
        RawEvent::abs_x(640),
        RawEvent::abs_y(320),
        RawEvent::new(EV_KEY, 0xffff, 1),
        RawEvent::new(EV_ABS, 0xffff, 42),
    ]);

    assert!(extract_core_events(&frame).is_empty());
}

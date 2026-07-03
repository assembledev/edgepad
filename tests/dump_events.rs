use edgepad::core::{AxisRange, Capabilities};
use edgepad::dump::{
    fixture_line_for_event, raw_line_for_event, write_capture_header, write_fixture_event,
    write_fixture_events_with_limit, write_raw_events_with_limit,
};
use evdev::{AbsoluteAxisCode, EventType, InputEvent, KeyCode, SynchronizationCode};

#[test]
fn write_capture_header_includes_device_capabilities_metadata() {
    let mut out = Vec::new();

    write_capture_header(
        &mut out,
        std::path::Path::new("/dev/input/event5"),
        Some(Capabilities {
            slot_min: 0,
            slot_max: 4,
            x: AxisRange { min: 10, max: 1210 },
            y: AxisRange { min: 20, max: 820 },
        }),
    )
    .expect("header should be written");

    assert_eq!(
        String::from_utf8(out).expect("header should be utf8"),
        "# edgepad .ev dump\n# device: /dev/input/event5\n# slots: 0..=4\n# x: 10..=1210\n# y: 20..=820\n\n"
    );
}

#[test]
fn fixture_line_for_event_keeps_only_replay_relevant_multitouch_events() {
    assert_eq!(
        fixture_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_SLOT.0,
            0,
        )),
        Some("ABS_MT_SLOT 0".to_string())
    );
    assert_eq!(
        fixture_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_TRACKING_ID.0,
            123,
        )),
        Some("ABS_MT_TRACKING_ID 123".to_string())
    );
    assert_eq!(
        fixture_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_POSITION_X.0,
            20,
        )),
        Some("ABS_MT_POSITION_X 20".to_string())
    );
    assert_eq!(
        fixture_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_POSITION_Y.0,
            300,
        )),
        Some("ABS_MT_POSITION_Y 300".to_string())
    );
    assert_eq!(
        fixture_line_for_event(InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        )),
        Some("SYN_REPORT".to_string())
    );
    assert_eq!(
        fixture_line_for_event(InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_DROPPED.0,
            0,
        )),
        Some("SYN_DROPPED".to_string())
    );

    assert_eq!(
        fixture_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_X.0,
            999,
        )),
        None
    );
}

#[test]
fn write_fixture_event_adds_blank_line_after_sync_boundaries() {
    let mut out = Vec::new();

    assert!(write_fixture_event(
        &mut out,
        InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_MT_SLOT.0, 0),
    )
    .expect("write should succeed"));
    assert!(write_fixture_event(
        &mut out,
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        ),
    )
    .expect("write should succeed"));
    assert!(write_fixture_event(
        &mut out,
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_DROPPED.0,
            0,
        ),
    )
    .expect("write should succeed"));

    assert_eq!(
        String::from_utf8(out).expect("fixture output should be utf8"),
        "ABS_MT_SLOT 0\nSYN_REPORT\n\nSYN_DROPPED\n\n"
    );
}

#[test]
fn write_fixture_events_with_limit_stops_after_requested_sync_boundaries() {
    let mut out = Vec::new();
    let events = vec![
        InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_MT_SLOT.0, 0),
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        ),
        InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_POSITION_X.0,
            42,
        ),
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_DROPPED.0,
            0,
        ),
        InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_POSITION_Y.0,
            99,
        ),
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        ),
    ];
    let mut remaining_frames = Some(2);

    let result = write_fixture_events_with_limit(&mut out, events, &mut remaining_frames)
        .expect("events should write");

    assert!(result.reached_limit);
    assert_eq!(result.events_written, 4);
    assert_eq!(result.frame_boundaries_written, 2);
    assert_eq!(remaining_frames, Some(0));
    assert_eq!(
        String::from_utf8(out).expect("fixture output should be utf8"),
        "ABS_MT_SLOT 0\nSYN_REPORT\n\nABS_MT_POSITION_X 42\nSYN_DROPPED\n\n"
    );
}

#[test]
fn raw_line_for_event_writes_touchpad_relevant_events_with_type_names() {
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_SLOT.0,
            1,
        )),
        "EV_ABS ABS_MT_SLOT 1"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_TRACKING_ID.0,
            200,
        )),
        "EV_ABS ABS_MT_TRACKING_ID 200"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_POSITION_X.0,
            500,
        )),
        "EV_ABS ABS_MT_POSITION_X 500"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_MT_POSITION_Y.0,
            300,
        )),
        "EV_ABS ABS_MT_POSITION_Y 300"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_X.0,
            640,
        )),
        "EV_ABS ABS_X 640"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Y.0,
            320,
        )),
        "EV_ABS ABS_Y 320"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOUCH.0, 1)),
        "EV_KEY BTN_TOUCH 1"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        )),
        "EV_SYN SYN_REPORT 0"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_DROPPED.0,
            0,
        )),
        "EV_SYN SYN_DROPPED 0"
    );
}

#[test]
fn raw_line_for_event_falls_back_to_numeric_codes_for_unknown_events() {
    assert_eq!(
        raw_line_for_event(InputEvent::new(EventType::KEY.0, 0xffff, 1)),
        "EV_KEY 65535 1"
    );
    assert_eq!(
        raw_line_for_event(InputEvent::new(0xffff, 0xfffe, 123)),
        "EV_65535 65534 123"
    );
}

#[test]
fn write_raw_events_with_limit_keeps_all_events_and_stops_after_sync_boundaries() {
    let mut out = Vec::new();
    let events = vec![
        InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOUCH.0, 1),
        InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 640),
        InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_MT_SLOT.0, 1),
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        ),
        InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOUCH.0, 0),
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_DROPPED.0,
            0,
        ),
        InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 320),
    ];
    let mut remaining_frames = Some(2);

    let result = write_raw_events_with_limit(&mut out, events, &mut remaining_frames)
        .expect("events should write");

    assert!(result.reached_limit);
    assert_eq!(result.events_written, 6);
    assert_eq!(result.frame_boundaries_written, 2);
    assert_eq!(remaining_frames, Some(0));
    assert_eq!(
        String::from_utf8(out).expect("raw output should be utf8"),
        "EV_KEY BTN_TOUCH 1\nEV_ABS ABS_X 640\nEV_ABS ABS_MT_SLOT 1\nEV_SYN SYN_REPORT 0\n\nEV_KEY BTN_TOUCH 0\nEV_SYN SYN_DROPPED 0\n\n"
    );
}

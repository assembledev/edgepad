use edgepad::core::{AxisRange, Capabilities};
use edgepad::dump::{
    fixture_line_for_event, write_capture_header, write_fixture_event,
    write_fixture_events_with_limit,
};
use evdev::{AbsoluteAxisCode, EventType, InputEvent, SynchronizationCode};

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

    let reached_limit = write_fixture_events_with_limit(&mut out, events, &mut remaining_frames)
        .expect("events should write");

    assert!(reached_limit);
    assert_eq!(remaining_frames, Some(0));
    assert_eq!(
        String::from_utf8(out).expect("fixture output should be utf8"),
        "ABS_MT_SLOT 0\nSYN_REPORT\n\nABS_MT_POSITION_X 42\nSYN_DROPPED\n\n"
    );
}

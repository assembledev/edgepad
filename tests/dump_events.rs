use edgepad::dump::{fixture_line_for_event, write_fixture_event};
use evdev::{AbsoluteAxisCode, EventType, InputEvent, SynchronizationCode};

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
            0
        ),
    )
    .expect("write should succeed"));
    assert!(write_fixture_event(
        &mut out,
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_DROPPED.0,
            0
        ),
    )
    .expect("write should succeed"));

    assert_eq!(
        String::from_utf8(out).expect("fixture output should be utf8"),
        "ABS_MT_SLOT 0\nSYN_REPORT\n\nSYN_DROPPED\n\n"
    );
}

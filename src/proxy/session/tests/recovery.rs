use super::*;
use crate::proxy::StopToken;
use crate::raw::EV_KEY;

#[test]
fn dropped_events_restore_contacts_and_buttons_before_shutdown_drains() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut config = session_config(EngineOptions::default());
    config.buttonpad = true;
    let mut h = Harness::new(config, &limit);
    h.frame(press(500), 1000, 0);
    h.handler.pending = Some(wider_edges());
    h.event(RawEvent::abs_mt_position_x(20), 1010, 10);
    assert_eq!(
        h.event(RawEvent::syn_dropped(), 1010, 10),
        SessionStep::Continue
    );
    assert_eq!(
        h.event(RawEvent::abs_mt_slot(999), 1010, 10),
        SessionStep::Continue
    );
    assert_eq!(
        h.event(RawEvent::abs_mt_tracking_id(77), 1010, 10),
        SessionStep::Continue
    );
    stop.stop();
    assert!(!h.idle(20));
    assert_eq!(h.handler.reload_polls, 0);
    let SessionStep::Resync { timestamp } = h.event(RawEvent::syn_report(), 1020, 25) else {
        panic!("a dropped stream must request a kernel snapshot");
    };
    assert_eq!(timestamp, Some(Duration::from_millis(1020)));
    assert!(!h
        .session
        .restore_snapshot(
            ResyncSnapshot {
                contacts: vec![ResyncContact {
                    slot: 0,
                    tracking_id: 50,
                    x: 500,
                    y: 300,
                    tool_type: None
                }],
                slots: vec![SlotSnapshot {
                    slot: 0,
                    x: 500,
                    y: 300,
                    tool_type: None
                }],
                pressed_buttons: vec![BTN_LEFT],
            },
            timestamp,
            h.start + Duration::from_millis(25),
            &mut h.sink,
            &mut h.handler,
        )
        .unwrap());
    assert_eq!(h.handler.reload_polls, 0);
    assert!(h
        .sink
        .frames()
        .last()
        .unwrap()
        .events
        .contains(&RawEvent::new(EV_KEY, BTN_LEFT, 1)));
    let mut events = release();
    events.push(RawEvent::new(EV_KEY, BTN_LEFT, 0));
    assert_eq!(h.frame(events, 1100, 100), SessionStep::Finished);
    assert!(h.handler.gestures.is_empty());
    assert!(h.handler.pending.is_some());
    let last = &h.sink.frames().last().unwrap().events;
    assert!(last.contains(&RawEvent::abs_mt_tracking_id(-1)));
    assert!(last.contains(&RawEvent::new(EV_KEY, BTN_LEFT, 0)));
    let stats = h.session.into_stats();
    assert!(stats.resync_required);
    assert_eq!(stats.input_frame_boundaries, 4);
    assert_eq!(stats.idle_drain_frame_boundaries, 2);
    assert!(!stats.idle_drain_timed_out);
}

#[test]
fn empty_resync_snapshot_allows_a_waiting_reload_at_the_restored_boundary() {
    let limit = ProxyRunLimit::UntilStopped {
        stop: StopToken::new(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut h = Harness::new(session_config(EngineOptions::default()), &limit);
    h.handler.pending = Some(wider_edges());
    h.event(RawEvent::syn_dropped(), 1000, 0);
    assert!(!h.idle(10));
    assert_eq!(h.handler.reload_polls, 0);
    let SessionStep::Resync { timestamp } = h.event(RawEvent::syn_report(), 1020, 20) else {
        panic!("a dropped stream must request a kernel snapshot");
    };
    assert!(!h
        .session
        .restore_snapshot(
            ResyncSnapshot {
                contacts: Vec::new(),
                slots: Vec::new(),
                pressed_buttons: Vec::new()
            },
            timestamp,
            h.start + Duration::from_millis(20),
            &mut h.sink,
            &mut h.handler,
        )
        .unwrap());
    assert_eq!(h.handler.order, ["reload"]);
    h.frame(press(150), 1100, 100);
    h.frame(release(), 1160, 160);
    assert_eq!(h.handler.gestures.len(), 1);
    assert_eq!(h.handler.gestures[0].zone, Zone::Left);
    let stats = h.session.into_stats();
    assert_eq!(stats.recognition_reloads, 1);
    assert_eq!(stats.input_frame_boundaries, 4);
}

#[test]
fn malformed_frame_errors_reach_the_session_caller_at_the_frame_boundary() {
    let limit = ProxyRunLimit::UntilStopped {
        stop: StopToken::new(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut h = Harness::new(session_config(EngineOptions::default()), &limit);
    assert_eq!(
        h.event(RawEvent::abs_mt_slot(999), 1000, 0),
        SessionStep::Continue
    );
    let err = h
        .session
        .on_event(
            ProxyInputEvent {
                raw: RawEvent::syn_report(),
                timestamp: Some(Duration::from_millis(1000)),
            },
            h.start,
            &mut h.sink,
            &mut h.handler,
        )
        .expect_err("invalid slots must fail when the complete frame is processed");
    assert!(err.contains("SlotOutOfRange"), "{err}");
    assert!(h.handler.gestures.is_empty());
}

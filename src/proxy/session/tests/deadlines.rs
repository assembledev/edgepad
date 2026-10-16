use super::*;
use crate::proxy::{StopAfterFrameLimit, StopToken};

#[test]
fn frame_budget_waits_for_a_pending_tap_beyond_the_physical_drain_timeout() {
    let limit = ProxyRunLimit::Frames {
        frame_boundaries: 2,
        stop_after_limit: StopAfterFrameLimit::WhenIdle,
    };
    let mut h = Harness::new(session_config(tap_options(1500)), &limit);
    assert_eq!(h.frame(press(20), 1000, 0), SessionStep::Continue);
    assert_eq!(h.frame(release(), 1060, 60), SessionStep::Continue);
    assert_eq!(h.timeout(60), Some(Duration::from_millis(1500)));
    assert!(!h.idle(1060));
    assert!(!h.idle(1559));
    assert!(h.idle(1560));
    assert_eq!(h.handler.gestures.len(), 1);
    assert_eq!(h.handler.gestures[0].direction, GestureDirection::Tap);
    let stats = h.session.into_stats();
    assert_eq!(stats.input_frame_boundaries, 2);
    assert_eq!(stats.raw_frames, 2);
    assert_eq!(stats.gesture_count(), 1);
    assert!(!stats.idle_drain_timed_out);
}

#[test]
fn stopping_the_daemon_waits_for_pending_recognition_without_retaining_history() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(50),
    };
    let mut h = Harness::new(session_config(tap_options(300)), &limit);
    h.frame(press(20), 1000, 0);
    h.frame(release(), 1060, 60);
    stop.stop();
    assert!(!h.idle(70));
    assert!(!h.idle(120));
    assert!(h.handler.gestures.is_empty());
    assert!(h.idle(360));
    assert_eq!(h.handler.gestures.len(), 1);
    let stats = h.session.into_stats();
    assert_eq!(stats.gesture_count(), 1);
    assert!(stats.gestures.is_empty());
    assert!(!stats.idle_drain_timed_out);
}

#[test]
fn a_pending_tap_dispatches_before_reload_changes_bindings_and_recognition() {
    let limit = ProxyRunLimit::UntilStopped {
        stop: StopToken::new(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut h = Harness::new(session_config(tap_options(300)), &limit);
    h.frame(press(20), 1000, 0);
    h.handler.pending = Some(wider_edges());
    h.frame(release(), 1060, 60);
    assert!(!h.idle(359));
    assert_eq!(h.handler.reload_polls, 0);
    assert!(h.handler.pending.is_some());
    assert!(!h.idle(360));
    assert_eq!(h.handler.order, ["gesture", "reload"]);

    h.frame(press(150), 1500, 500);
    h.frame(release(), 1560, 560);
    assert_eq!(h.handler.gestures.len(), 2);
    assert!(h
        .handler
        .gestures
        .iter()
        .all(|gesture| gesture.zone == Zone::Left));
    assert_eq!(h.session.into_stats().recognition_reloads, 1);
}

#[test]
fn reload_cannot_split_an_incomplete_frame_or_an_active_contact() {
    let limit = ProxyRunLimit::UntilStopped {
        stop: StopToken::new(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut h = Harness::new(session_config(EngineOptions::default()), &limit);
    h.handler.pending = Some(wider_edges());
    for raw in press(20) {
        assert_eq!(h.event(raw, 1000, 0), SessionStep::Continue);
    }
    assert!(!h.idle(5));
    assert_eq!(h.handler.reload_polls, 0);
    assert_eq!(
        h.event(RawEvent::syn_report(), 1000, 10),
        SessionStep::Continue
    );
    assert_eq!(h.handler.reload_polls, 0);
    h.frame(release(), 1060, 60);
    assert_eq!(h.handler.order, ["gesture", "reload"]);
    assert_eq!(h.session.into_stats().recognition_reloads, 1);
}

#[test]
fn a_stuck_edge_contact_has_a_bounded_shutdown_even_when_output_is_empty() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut h = Harness::new(session_config(EngineOptions::default()), &limit);
    h.frame(press(20), 1000, 0);
    assert!(h.sink.frames().is_empty());
    stop.stop();
    assert!(!h.idle(10));
    assert_eq!(h.timeout(10), Some(Duration::from_millis(250)));
    assert!(!h.idle(259));
    assert!(h.idle(260));
    assert!(h.handler.gestures.is_empty());
    let stats = h.session.into_stats();
    assert!(stats.idle_drain_timed_out);
    assert_eq!(stats.input_frame_boundaries, 1);
}

#[test]
fn immediate_frame_budget_cleans_up_an_active_passthrough_contact() {
    let limit = ProxyRunLimit::Frames {
        frame_boundaries: 1,
        stop_after_limit: StopAfterFrameLimit::Immediately,
    };
    let mut h = Harness::new(session_config(EngineOptions::default()), &limit);
    assert_eq!(h.frame(press(500), 1000, 0), SessionStep::Finished);
    assert_eq!(h.sink.frames().len(), 2);
    let cleanup = &h.sink.frames()[1].events;
    assert!(cleanup.contains(&RawEvent::abs_mt_tracking_id(-1)));
    assert!(cleanup.contains(&RawEvent::btn_touch(false)));
    let stats = h.session.into_stats();
    assert_eq!(stats.cleanup_output_frames, 1);
    assert_eq!(stats.input_frame_boundaries, 1);
}

#[test]
fn continuous_input_cannot_extend_the_shutdown_drain_deadline() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut h = Harness::new(session_config(EngineOptions::default()), &limit);
    h.frame(press(500), 1000, 0);
    stop.stop();
    for wall_ms in [10, 110, 210] {
        assert_eq!(
            h.frame(
                vec![RawEvent::abs_mt_position_y(301)],
                1000 + wall_ms,
                wall_ms
            ),
            SessionStep::Continue
        );
    }
    assert_eq!(
        h.frame(vec![RawEvent::abs_mt_position_y(302)], 1260, 260),
        SessionStep::Finished
    );
    let cleanup = &h.sink.frames().last().unwrap().events;
    assert!(cleanup.contains(&RawEvent::abs_mt_tracking_id(-1)));
    assert!(cleanup.contains(&RawEvent::btn_touch(false)));
    let stats = h.session.into_stats();
    assert!(stats.idle_drain_timed_out);
    assert_eq!(stats.cleanup_output_frames, 1);
    assert_eq!(stats.input_frame_boundaries, 5);
}

#[test]
fn a_release_at_the_drain_deadline_waits_for_tap_recognition() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(50),
    };
    let mut h = Harness::new(session_config(tap_options(300)), &limit);
    h.frame(press(20), 1000, 0);
    stop.stop();
    assert_eq!(h.frame(Vec::new(), 1010, 10), SessionStep::Continue);
    assert_eq!(h.frame(release(), 1060, 60), SessionStep::Continue);
    assert_eq!(h.timeout(60), Some(Duration::from_millis(100)));
    assert!(!h.idle(359));
    assert!(h.idle(360));
    assert_eq!(h.handler.gestures.len(), 1);
    assert!(!h.session.into_stats().idle_drain_timed_out);
}

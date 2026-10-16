use super::super::{input::PhysicalTouchState, output::*};
use super::*;
use crate::core::{EdgeWidths, Engine, EngineOptions, Event, GestureDirection, Zone, ZoneSet};
use crate::proxy::test_support::test_capabilities;
use crate::proxy::{ProxyRunLimit, ProxyRuntimeStats, StopAfterFrameLimit, StopToken};
use crate::raw::{
    route_recognition_deadline, RawEvent, RawFrame, RawOutputComposer, RecordingRawOutputSink,
};
use std::time::{Duration, Instant};

#[test]
fn recognition_deadline_wakes_and_releases_pending_single_tap_without_input() {
    let capabilities = test_capabilities();
    let options = EngineOptions {
        double_tap_zones: ZoneSet::from_zones([Zone::Left]),
        single_tap_zones: ZoneSet::from_zones([Zone::Left]),
        ..EngineOptions::default()
    };
    let mut engine = Engine::with_options(capabilities, EdgeWidths::all(0.10), Vec::new(), options);
    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(1),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("tap should start");
    let released = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1060),
        )
        .expect("tap should release");
    assert!(released.gestures.is_empty());

    let wall_start = Instant::now();
    let mut timer = RecognitionDeadline::default();
    timer.sync(
        engine.next_deadline(),
        Some(Duration::from_millis(1060)),
        wall_start,
    );
    assert_eq!(
        timer.poll_timeout(wall_start),
        Some(Duration::from_millis(300))
    );
    assert!(timer
        .take_due(wall_start + Duration::from_millis(299))
        .is_none());
    let input_deadline = timer
        .take_due(wall_start + Duration::from_millis(300))
        .expect("timer should wake at the engine deadline");
    let routed = route_recognition_deadline(&mut engine, input_deadline);
    assert_eq!(routed.gestures.len(), 1);
    assert_eq!(routed.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn bounded_proxy_can_finish_on_recognition_deadline_without_an_extra_input_frame() {
    let capabilities = test_capabilities();
    let mut engine = Engine::with_options(
        capabilities,
        EdgeWidths::all(0.10),
        Vec::new(),
        EngineOptions {
            double_tap_timeout: Duration::from_millis(1500),
            double_tap_zones: ZoneSet::from_zones([Zone::Left]),
            single_tap_zones: ZoneSet::from_zones([Zone::Left]),
            ..EngineOptions::default()
        },
    );
    engine
        .process_frame_at(
            &[
                Event::slot(0),
                Event::tracking_id(1),
                Event::x(20),
                Event::y(300),
            ],
            Duration::from_millis(1000),
        )
        .expect("tap should start");
    engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1060),
        )
        .expect("tap should become pending");

    let limit = ProxyRunLimit::Frames {
        frame_boundaries: 1,
        stop_after_limit: StopAfterFrameLimit::WhenIdle,
    };
    let mut stopper = ProxyLoopStopper::new(&limit);
    let wall_start = Instant::now();
    let mut timer = RecognitionDeadline::default();
    timer.sync(
        engine.next_deadline(),
        Some(Duration::from_millis(1060)),
        wall_start,
    );

    assert!(!stopper.observe_frame_boundary(true));
    assert!(stopper.is_draining());
    assert!(sync_physical_drain_deadline(None, &stopper, false, wall_start).is_none());
    assert_eq!(
        timer.poll_timeout(wall_start),
        Some(Duration::from_millis(1500))
    );

    let input_deadline = timer
        .take_due(wall_start + Duration::from_millis(1500))
        .expect("recognition deadline should remain armed beyond the safety drain interval");
    let routed = route_recognition_deadline(&mut engine, input_deadline);
    assert_eq!(routed.gestures.len(), 1);
    assert_eq!(routed.gestures[0].direction, GestureDirection::Tap);
    assert!(stopper.observe_idle_poll(false));
}

#[test]
fn physical_contact_drain_still_has_a_bounded_safety_deadline() {
    let limit = ProxyRunLimit::Frames {
        frame_boundaries: 1,
        stop_after_limit: StopAfterFrameLimit::WhenIdle,
    };
    let mut stopper = ProxyLoopStopper::new(&limit);
    let now = Instant::now();

    assert!(!stopper.observe_frame_boundary(true));
    assert_eq!(
        sync_physical_drain_deadline(None, &stopper, true, now),
        Some(now + UINPUT_IDLE_DRAIN_TIMEOUT)
    );
}

#[test]
fn frame_limit_stop_waits_for_idle_after_budget_when_configured() {
    let limit = ProxyRunLimit::Frames {
        frame_boundaries: 2,
        stop_after_limit: StopAfterFrameLimit::WhenIdle,
    };
    let mut stopper = ProxyLoopStopper::new(&limit);

    assert!(!stopper.observe_frame_boundary(true));
    assert!(!stopper.observe_frame_boundary(true));
    assert_eq!(stopper.extra_frame_boundaries(), 0);
    assert!(!stopper.observe_frame_boundary(true));
    assert_eq!(stopper.extra_frame_boundaries(), 1);
    assert!(stopper.observe_frame_boundary(false));
    assert_eq!(stopper.extra_frame_boundaries(), 2);
}

#[test]
fn frame_limit_stop_keeps_exact_budget_for_dry_run() {
    let limit = ProxyRunLimit::Frames {
        frame_boundaries: 2,
        stop_after_limit: StopAfterFrameLimit::Immediately,
    };
    let mut stopper = ProxyLoopStopper::new(&limit);

    assert!(!stopper.observe_frame_boundary(true));
    assert!(stopper.observe_frame_boundary(true));
    assert_eq!(stopper.extra_frame_boundaries(), 0);
}

#[test]
fn until_stopped_finishes_at_next_idle_boundary_after_stop_token() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut stopper = ProxyLoopStopper::new(&limit);

    assert!(!stopper.observe_frame_boundary(false));
    stop.stop();

    assert!(stopper.observe_frame_boundary(false));
    assert_eq!(stopper.extra_frame_boundaries(), 0);
}

#[test]
fn until_stopped_drains_after_stop_token_when_touch_is_active() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut stopper = ProxyLoopStopper::new(&limit);

    assert!(!stopper.observe_frame_boundary(true));
    stop.stop();
    assert!(!stopper.observe_frame_boundary(true));
    assert!(stopper.is_draining());
    assert_eq!(stopper.drain_timeout(), Some(Duration::from_millis(250)));
    assert!(!stopper.observe_frame_boundary(true));
    assert_eq!(stopper.extra_frame_boundaries(), 1);

    assert!(stopper.observe_frame_boundary(false));
    assert_eq!(stopper.extra_frame_boundaries(), 2);
}

#[test]
fn until_stopped_idle_poll_wakes_stopped_idle_loop() {
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut stopper = ProxyLoopStopper::new(&limit);

    assert_eq!(stopper.poll_timeout(), Some(STOP_POLL_INTERVAL));
    assert!(!stopper.observe_idle_poll(false));
    stop.stop();

    assert!(stopper.observe_idle_poll(false));
}

#[test]
fn until_stopped_finishes_after_synthetic_frame_sequence_reaches_idle() {
    let capabilities = test_capabilities();
    let stop = StopToken::new();
    let limit = ProxyRunLimit::UntilStopped {
        stop: stop.clone(),
        idle_drain_timeout: Duration::from_millis(250),
    };
    let mut stopper = ProxyLoopStopper::new(&limit);
    let mut touch_state = PhysicalTouchState::new(capabilities);
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
    let mut composer = RawOutputComposer::new(capabilities);
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();

    let start = RawFrame::new(vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::abs_mt_position_x(520),
        RawEvent::abs_mt_position_y(320),
    ]);
    touch_state.observe_frame(&start);
    process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &start)
        .expect("center start should process");
    assert!(!stopper.observe_frame_boundary(touch_state.is_touch_down()));

    stop.stop();
    let release = RawFrame::new(vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(-1),
    ]);
    touch_state.observe_frame(&release);
    process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &release)
        .expect("center release should process");
    assert!(stopper.observe_frame_boundary(touch_state.is_touch_down()));
    finish_proxy_output(&mut composer, &mut sink, &mut stats)
        .expect("finished idle composer should not fail");

    assert_eq!(stats.cleanup_output_frames, 0);
    assert_eq!(sink.frames().len(), 2);
}

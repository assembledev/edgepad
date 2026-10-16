use super::*;
use crate::core::{
    EdgeWidths, Engine, GestureDirection, SliderAxis, SliderDirection, SliderSpec, Zone,
};
use crate::proxy::test_support::{test_capabilities, RecordingGestureHandler};
use crate::proxy::{GestureCountKey, ProxyRuntimeStats, SliderStepCountKey};
use crate::raw::{RawEvent, RawFrame, RawOutputComposer, RecordingRawOutputSink, BTN_LEFT, EV_KEY};
mod history;

#[test]
fn proxy_dry_run_frame_stats_match_raw_replay_output() {
    let capabilities = test_capabilities();
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
    let mut composer = RawOutputComposer::new(capabilities);
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();
    let frame = RawFrame::new(vec![
        RawEvent::btn_touch(true),
        RawEvent::abs_x(20),
        RawEvent::abs_y(300),
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(100),
        RawEvent::abs_mt_position_x(20),
        RawEvent::abs_mt_position_y(300),
        RawEvent::abs_mt_slot(1),
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::abs_mt_position_x(520),
        RawEvent::abs_mt_position_y(320),
    ]);

    process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &frame)
        .expect("mixed frame should process");

    assert_eq!(stats.raw_frames, 1);
    assert_eq!(stats.raw_events, 11);
    assert_eq!(stats.recognizer_events, 8);
    assert_eq!(stats.recognizer_passthrough_events, 4);
    assert_eq!(stats.claimed_edge_frames, 1);
    assert_eq!(stats.passthrough_frames, 1);
    assert_eq!(stats.empty_output_frames, 0);
    assert_eq!(stats.composed_frames, 1);
    assert_eq!(stats.composed_events, 8);
    assert_eq!(stats.gestures.len(), 0);
    assert!(stats.gesture_counts.is_empty());
    assert!(!stats.resync_required);
}

#[test]
fn proxy_finish_output_releases_active_passthrough_contact_before_exit() {
    let capabilities = test_capabilities();
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
    let mut composer = RawOutputComposer::new(capabilities);
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();
    let frame = RawFrame::new(vec![
        RawEvent::abs_mt_tracking_id(200),
        RawEvent::abs_mt_position_x(520),
        RawEvent::abs_mt_position_y(320),
    ]);

    process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &frame)
        .expect("center frame should process");
    finish_proxy_output(&mut composer, &mut sink, &mut stats)
        .expect("finish should emit a synthetic release frame");

    assert_eq!(stats.composed_frames, 2);
    assert_eq!(stats.composed_events, 11);
    assert_eq!(stats.cleanup_output_frames, 1);
    assert_eq!(stats.cleanup_output_events, 3);
    assert_eq!(sink.frames().len(), 2);
    assert_eq!(
        sink.frames()[1].events,
        vec![
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ]
    );
}

#[test]
fn resync_restores_held_buttonpad_button_before_new_edge_contacts() {
    let capabilities = test_capabilities();
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
    engine.set_buttonpad(true);
    let mut composer = RawOutputComposer::new(capabilities);
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();
    let mut handler = RecordingGestureHandler::default();

    process_proxy_resync_contacts(
        &[],
        &[BTN_LEFT],
        &mut engine,
        &mut composer,
        &mut sink,
        &mut stats,
        &mut handler,
    )
    .expect("resync should restore held physical button");

    process_proxy_frame_with_handler(
        &mut engine,
        &mut composer,
        &mut sink,
        &mut stats,
        &RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]),
        &mut handler,
    )
    .expect("new edge contact while restored button is held should process");

    assert_eq!(sink.frames().len(), 2);
    assert_eq!(
        sink.frames()[0].events,
        vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]
    );
    assert_eq!(
        sink.frames()[1].events,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
            RawEvent::abs_x(20),
            RawEvent::abs_y(300),
        ]
    );
    assert!(handler.gestures.is_empty());
}

#[test]
fn proxy_dry_run_stats_count_gestures_by_zone_and_direction() {
    let capabilities = test_capabilities();
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
    let mut composer = RawOutputComposer::new(capabilities);
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();
    let frames = [
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(300),
            RawEvent::abs_mt_position_x(980),
            RawEvent::abs_mt_position_y(400),
        ]),
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_position_x(980),
            RawEvent::abs_mt_position_y(620),
        ]),
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
        ]),
    ];

    for frame in &frames {
        process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, frame)
            .expect("edge frames should process");
    }

    assert_eq!(stats.raw_frames, 3);
    assert_eq!(stats.claimed_edge_frames, 3);
    assert_eq!(stats.passthrough_frames, 0);
    assert_eq!(stats.empty_output_frames, 3);
    assert_eq!(stats.composed_frames, 0);
    assert_eq!(stats.gestures.len(), 1);
    assert_eq!(
        stats.gesture_counts.get(&GestureCountKey {
            zone: Zone::Right,
            direction: GestureDirection::Down,
        }),
        Some(&1)
    );
}

#[test]
fn proxy_handler_receives_recognized_gestures_live() {
    let capabilities = test_capabilities();
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
    let mut composer = RawOutputComposer::new(capabilities);
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();
    let mut handler = RecordingGestureHandler::default();
    let frames = [
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(300),
            RawEvent::abs_mt_position_x(980),
            RawEvent::abs_mt_position_y(400),
        ]),
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_position_x(980),
            RawEvent::abs_mt_position_y(620),
        ]),
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
        ]),
    ];

    for frame in &frames {
        process_proxy_frame_with_handler(
            &mut engine,
            &mut composer,
            &mut sink,
            &mut stats,
            frame,
            &mut handler,
        )
        .expect("edge frames should process");
    }

    assert_eq!(handler.gestures, stats.gestures);
    assert_eq!(handler.gestures.len(), 1);
    assert_eq!(handler.gestures[0].zone, Zone::Right);
    assert_eq!(handler.gestures[0].direction, GestureDirection::Down);
}

#[test]
fn proxy_handler_receives_slider_steps_live() {
    let capabilities = test_capabilities();
    let mut engine = Engine::with_sliders(
        capabilities,
        EdgeWidths::all(0.10),
        vec![SliderSpec {
            zone: Zone::Right,
            axis: SliderAxis::Vertical,
            step: 0.09,
        }],
    );
    let mut composer = RawOutputComposer::new(capabilities);
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();
    let mut handler = RecordingGestureHandler::default();
    let frames = [
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(300),
            RawEvent::abs_mt_position_x(980),
            RawEvent::abs_mt_position_y(400),
        ]),
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_position_y(610),
        ]),
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
        ]),
    ];

    for frame in &frames {
        process_proxy_frame_with_handler(
            &mut engine,
            &mut composer,
            &mut sink,
            &mut stats,
            frame,
            &mut handler,
        )
        .expect("slider frames should process");
    }

    assert_eq!(handler.slider_steps, stats.slider_steps);
    assert_eq!(handler.slider_steps.len(), 3);
    assert!(stats.gestures.is_empty());
    assert_eq!(
        stats.slider_step_counts.get(&SliderStepCountKey {
            zone: Zone::Right,
            direction: SliderDirection::Down,
        }),
        Some(&3)
    );
}

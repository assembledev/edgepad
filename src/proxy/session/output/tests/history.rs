use super::super::process_proxy_routed_frame;
use crate::proxy::test_support::{test_capabilities, RecordingGestureHandler};
use crate::proxy::*;
use crate::raw::{RawOutputComposer, RecordingRawOutputSink};
use std::time::Duration;

#[test]
fn daemon_statistics_do_not_retain_event_history() {
    #[derive(Default)]
    struct CountingHandler {
        gestures: usize,
        slider_steps: usize,
    }

    impl GestureHandler for CountingHandler {
        fn handle_gesture(&mut self, _gesture: Gesture) {
            self.gestures += 1;
        }

        fn handle_slider_step(&mut self, _step: SliderStep) {
            self.slider_steps += 1;
        }
    }

    let limit = ProxyRunLimit::UntilStopped {
        stop: StopToken::new(),
        idle_drain_timeout: Duration::from_secs(1),
    };
    let mut stats = ProxyRuntimeStats::for_run(&limit);
    let mut composer = RawOutputComposer::new(test_capabilities());
    let mut sink = RecordingRawOutputSink::default();
    let mut handler = CountingHandler::default();
    let gesture = Gesture {
        zone: Zone::Left,
        direction: GestureDirection::Right,
        slot: 0,
        tracking_id: 1,
    };
    let step = SliderStep {
        zone: Zone::Right,
        direction: SliderDirection::Down,
        slot: 1,
        tracking_id: 2,
    };

    for _ in 0..10_000 {
        process_proxy_routed_frame(
            0,
            &mut composer,
            &mut sink,
            &mut stats,
            crate::raw::RoutedRawFrame {
                gestures: vec![gesture],
                slider_steps: vec![step],
                ..Default::default()
            },
            &mut handler,
        )
        .expect("recognized events should still dispatch");
    }

    assert!(stats.gestures.is_empty());
    assert!(stats.slider_steps.is_empty());
    assert_eq!(stats.gestures.capacity(), 0);
    assert_eq!(stats.slider_steps.capacity(), 0);
    assert_eq!(stats.gesture_counts.len(), 1);
    assert_eq!(stats.slider_step_counts.len(), 1);
    assert_eq!(stats.gesture_counts[&gesture_count_key(gesture)], 10_000);
    assert_eq!(
        stats.slider_step_counts[&slider_step_count_key(step)],
        10_000
    );
    assert_eq!(stats.gesture_count(), 10_000);
    assert_eq!(stats.slider_step_count(), 10_000);
    assert_eq!(handler.gestures, 10_000);
    assert_eq!(handler.slider_steps, 10_000);
}

#[test]
fn bounded_proxy_statistics_preserve_ordered_event_history() {
    for stop_after_limit in [
        StopAfterFrameLimit::Immediately,
        StopAfterFrameLimit::WhenIdle,
    ] {
        let limit = ProxyRunLimit::Frames {
            frame_boundaries: 2,
            stop_after_limit,
        };
        let mut stats = ProxyRuntimeStats::for_run(&limit);
        let mut composer = RawOutputComposer::new(test_capabilities());
        let mut sink = RecordingRawOutputSink::default();
        let mut handler = RecordingGestureHandler::default();

        for tracking_id in [10, 11] {
            process_proxy_routed_frame(
                0,
                &mut composer,
                &mut sink,
                &mut stats,
                crate::raw::RoutedRawFrame {
                    gestures: vec![Gesture {
                        zone: Zone::Left,
                        direction: GestureDirection::Right,
                        slot: 0,
                        tracking_id,
                    }],
                    slider_steps: vec![SliderStep {
                        zone: Zone::Right,
                        direction: SliderDirection::Down,
                        slot: 1,
                        tracking_id,
                    }],
                    ..Default::default()
                },
                &mut handler,
            )
            .expect("diagnostic events should process");
        }

        assert_eq!(stats.gesture_count(), 2);
        assert_eq!(stats.slider_step_count(), 2);
        assert_eq!(stats.gestures, handler.gestures);
        assert_eq!(stats.slider_steps, handler.slider_steps);
        assert_eq!(
            stats
                .gestures
                .iter()
                .map(|gesture| gesture.tracking_id)
                .collect::<Vec<_>>(),
            vec![10, 11]
        );
        assert_eq!(
            stats
                .slider_steps
                .iter()
                .map(|step| step.tracking_id)
                .collect::<Vec<_>>(),
            vec![10, 11]
        );
    }
}

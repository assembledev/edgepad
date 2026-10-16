mod deadlines;
mod recovery;

use super::*;
use crate::core::{
    EdgeWidths, Engine, EngineOptions, Event, GestureDirection, SlotSnapshot, Zone, ZoneSet,
};
use crate::proxy::test_support::{test_capabilities, ReloadingGestureHandler};
use crate::proxy::ProxyRecognitionConfig;
use crate::raw::BTN_LEFT;
use std::time::Duration;

#[test]
fn recognition_reload_waits_for_idle_and_preserves_button_state() {
    let capabilities = test_capabilities();
    let config = SessionConfig {
        capabilities,
        edge_widths: EdgeWidths::all(0.10),
        engine_options: EngineOptions::default(),
        slider_specs: Vec::new(),
        initial_slot_snapshots: Vec::new(),
        buttonpad: true,
    };
    let mut engine = Engine::new(capabilities, config.edge_widths);
    engine
        .seed_slot_snapshots(&[SlotSnapshot {
            slot: 0,
            x: 20,
            y: 300,
            tool_type: None,
        }])
        .expect("slot state should seed");
    engine.set_buttonpad(true);
    engine.update_physical_button(BTN_LEFT, true);
    let mut handler = ReloadingGestureHandler {
        pending: Some(ProxyRecognitionConfig {
            edge_widths: EdgeWidths::all(0.20),
            engine_options: EngineOptions {
                tap_min_duration: Duration::from_millis(120),
                swipe_min_distance: 0.05,
                ..EngineOptions::default()
            },
            slider_specs: Vec::new(),
        }),
        reload_polls: 0,
    };

    assert!(!apply_pending_recognition_reload(
        &mut engine,
        true,
        &mut handler
    ));
    assert_eq!(handler.reload_polls, 0);
    assert!(handler.pending.is_some());

    assert!(apply_pending_recognition_reload(
        &mut engine,
        false,
        &mut handler
    ));
    assert_eq!(handler.reload_polls, 1);
    assert!(handler.pending.is_none());
    assert_eq!(engine.pressed_physical_buttons(), vec![BTN_LEFT]);

    engine.update_physical_button(BTN_LEFT, false);
    engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(9)],
            Duration::from_millis(1000),
        )
        .expect("reload should preserve retained Type-B axes");
    let released = engine
        .process_frame_at(
            &[Event::slot(0), Event::tracking_id(-1)],
            Duration::from_millis(1130),
        )
        .expect("retained-position contact should release");
    assert_eq!(released.gestures[0].zone, Zone::Left);
    assert_eq!(released.gestures[0].direction, GestureDirection::Tap);
}

#[test]
fn recognition_reload_waits_for_pending_tap_deadline() {
    let capabilities = test_capabilities();
    let config = SessionConfig {
        capabilities,
        edge_widths: EdgeWidths::all(0.10),
        engine_options: EngineOptions::default(),
        slider_specs: Vec::new(),
        initial_slot_snapshots: Vec::new(),
        buttonpad: false,
    };
    let mut engine = Engine::with_options(
        capabilities,
        config.edge_widths,
        Vec::new(),
        EngineOptions {
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
        .expect("tap should release");
    let mut handler = ReloadingGestureHandler {
        pending: Some(ProxyRecognitionConfig {
            edge_widths: EdgeWidths::all(0.20),
            engine_options: EngineOptions::default(),
            slider_specs: Vec::new(),
        }),
        reload_polls: 0,
    };

    assert!(!apply_pending_recognition_reload(
        &mut engine,
        false,
        &mut handler
    ));
    assert_eq!(handler.reload_polls, 0);

    engine.advance_time(Duration::from_millis(1360));
    assert!(apply_pending_recognition_reload(
        &mut engine,
        false,
        &mut handler
    ));
    assert_eq!(handler.reload_polls, 1);
}

#[test]
fn startup_snapshot_preserves_inactive_slot_palm_type_without_a_new_event() {
    let mut engine = Engine::new(test_capabilities(), EdgeWidths::all(0.10));
    engine
        .seed_slot_snapshots(&[SlotSnapshot {
            slot: 0,
            x: 20,
            y: 300,
            tool_type: Some(2),
        }])
        .unwrap();
    engine.process_frame(&[Event::tracking_id(700)]).unwrap();
    engine.process_frame(&[Event::x(220)]).unwrap();
    let output = engine.process_frame(&[Event::tracking_id(-1)]).unwrap();
    assert!(output.gestures.is_empty());
}

#[test]
fn disabled_palm_rejection_ignores_seeded_slot_type() {
    let mut engine = Engine::with_options(
        test_capabilities(),
        EdgeWidths::all(0.1),
        vec![],
        EngineOptions {
            palm_rejection: false,
            ..EngineOptions::default()
        },
    );
    engine
        .seed_slot_snapshots(&[SlotSnapshot {
            slot: 0,
            x: 20,
            y: 300,
            tool_type: Some(2),
        }])
        .unwrap();
    engine
        .process_frame_at(&[Event::tracking_id(702)], Duration::from_millis(1000))
        .unwrap();
    let output = engine
        .process_frame_at(&[Event::tracking_id(-1)], Duration::from_millis(1100))
        .unwrap();
    assert_eq!(output.gestures.len(), 1);
}

#[derive(Default)]
struct SessionHandler {
    pending: Option<ProxyRecognitionConfig>,
    gestures: Vec<crate::core::Gesture>,
    order: Vec<&'static str>,
    reload_polls: usize,
}

impl GestureHandler for SessionHandler {
    fn handle_gesture(&mut self, gesture: crate::core::Gesture) {
        self.gestures.push(gesture);
        self.order.push("gesture");
    }

    fn take_recognition_config_reload(&mut self) -> Option<ProxyRecognitionConfig> {
        self.reload_polls += 1;
        let pending = self.pending.take();
        if pending.is_some() {
            self.order.push("reload");
        }
        pending
    }
}

struct Harness<'a> {
    session: ProxySession<'a>,
    sink: crate::raw::RecordingRawOutputSink,
    handler: SessionHandler,
    start: Instant,
}

impl<'a> Harness<'a> {
    fn new(config: SessionConfig, limit: &'a ProxyRunLimit) -> Self {
        Self {
            session: ProxySession::new(config, limit).expect("session should initialize"),
            sink: crate::raw::RecordingRawOutputSink::default(),
            handler: SessionHandler::default(),
            start: Instant::now(),
        }
    }

    fn event(&mut self, raw: RawEvent, input_ms: u64, wall_ms: u64) -> SessionStep {
        self.session
            .on_event(
                ProxyInputEvent {
                    raw,
                    timestamp: Some(Duration::from_millis(input_ms)),
                },
                self.start + Duration::from_millis(wall_ms),
                &mut self.sink,
                &mut self.handler,
            )
            .expect("event should process")
    }

    fn frame(&mut self, events: Vec<RawEvent>, input_ms: u64, wall_ms: u64) -> SessionStep {
        for raw in events.into_iter().chain([RawEvent::syn_report()]) {
            let step = self.event(raw, input_ms, wall_ms);
            if step != SessionStep::Continue {
                return step;
            }
        }
        SessionStep::Continue
    }

    fn idle(&mut self, wall_ms: u64) -> bool {
        self.session
            .on_idle_poll(
                self.start + Duration::from_millis(wall_ms),
                &mut self.sink,
                &mut self.handler,
            )
            .expect("idle poll should process")
    }

    fn timeout(&self, wall_ms: u64) -> Option<Duration> {
        self.session
            .poll_timeout(self.start + Duration::from_millis(wall_ms))
    }
}

fn session_config(options: EngineOptions) -> SessionConfig {
    SessionConfig {
        capabilities: test_capabilities(),
        edge_widths: EdgeWidths::all(0.1),
        engine_options: options,
        slider_specs: Vec::new(),
        initial_slot_snapshots: Vec::new(),
        buttonpad: false,
    }
}

fn tap_options(timeout_ms: u64) -> EngineOptions {
    EngineOptions {
        double_tap_timeout: Duration::from_millis(timeout_ms),
        double_tap_zones: ZoneSet::from_zones([Zone::Left]),
        single_tap_zones: ZoneSet::from_zones([Zone::Left]),
        ..EngineOptions::default()
    }
}

fn press(x: i32) -> Vec<RawEvent> {
    vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(1),
        RawEvent::abs_mt_position_x(x),
        RawEvent::abs_mt_position_y(300),
        RawEvent::btn_touch(true),
    ]
}

fn release() -> Vec<RawEvent> {
    vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(-1),
        RawEvent::btn_touch(false),
    ]
}

fn wider_edges() -> ProxyRecognitionConfig {
    ProxyRecognitionConfig {
        edge_widths: EdgeWidths::all(0.2),
        engine_options: EngineOptions::default(),
        slider_specs: Vec::new(),
    }
}

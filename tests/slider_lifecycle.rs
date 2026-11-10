use edgepad::config::EdgepadConfig;
use edgepad::core::{
    AxisRange, Capabilities, Engine, Event, SliderEndKind, SliderEvent, MT_TOOL_PALM,
};
use edgepad::raw::BTN_LEFT;

const CONFIG: &str = r#"
[[sliders]]
zone = "bottom"
step = 0.1
left = ["previous"]
right = ["next"]
on_release = ["commit"]
on_cancel = ["cancel"]
"#;

fn engine() -> Engine {
    let config = EdgepadConfig::parse(CONFIG).unwrap();
    Engine::with_options(
        Capabilities {
            slot_min: 0,
            slot_max: 1,
            x: AxisRange { min: 0, max: 1000 },
            y: AxisRange { min: 0, max: 1000 },
        },
        config.active_edge_widths(),
        config.slider_specs(),
        config.engine_options(),
    )
}

fn touch(engine: &mut Engine) {
    engine
        .process_frame(&[
            Event::slot(0),
            Event::tracking_id(42),
            Event::x(200),
            Event::y(990),
        ])
        .unwrap();
}

fn start_drag(engine: &mut Engine) {
    touch(engine);
    assert!(!engine
        .process_frame(&[Event::x(450)])
        .unwrap()
        .slider_events
        .is_empty());
}

fn assert_end(events: &[SliderEvent], kind: SliderEndKind) {
    assert_eq!(events.len(), 1);
    let SliderEvent::End(end) = events[0] else {
        panic!("expected an ending")
    };
    assert_eq!(end.kind, kind);
    assert_eq!(end.slot, 0);
    assert_eq!(end.tracking_id, 42);
}

#[test]
fn lift_runs_after_final_movement_in_the_same_frame() {
    let mut engine = engine();
    start_drag(&mut engine);
    let output = engine
        .process_frame(&[Event::x(550), Event::tracking_id(-1)])
        .unwrap();
    assert!(matches!(output.slider_events[0], SliderEvent::Step(_)));
    assert_end(&output.slider_events[1..], SliderEndKind::Release);
    assert!(output.gestures.is_empty());
    assert!(engine
        .process_frame(&[Event::tracking_id(-1)])
        .unwrap()
        .slider_events
        .is_empty());
}

#[test]
fn touch_below_threshold_has_no_lifecycle_action() {
    let mut engine = engine();
    touch(&mut engine);
    let output = engine
        .process_frame(&[Event::x(210), Event::tracking_id(-1)])
        .unwrap();
    assert!(output.slider_events.is_empty());
}

#[test]
fn palm_cancels_before_movement_and_never_releases() {
    let mut engine = engine();
    start_drag(&mut engine);
    let output = engine
        .process_frame(&[Event::x(700), Event::tool_type(MT_TOOL_PALM)])
        .unwrap();
    assert_end(&output.slider_events, SliderEndKind::Cancel);
    assert!(engine
        .process_frame(&[Event::tracking_id(-1)])
        .unwrap()
        .slider_events
        .is_empty());
}

#[test]
fn dropped_input_and_direct_resync_cancel_once() {
    let mut engine = engine();
    start_drag(&mut engine);
    let output = engine.process_frame(&[Event::syn_dropped()]).unwrap();
    assert_end(&output.slider_events, SliderEndKind::Cancel);
    assert!(engine
        .restore_passthrough_contacts(&[])
        .unwrap()
        .slider_events
        .is_empty());
    start_drag(&mut engine);
    assert_end(
        &engine
            .restore_passthrough_contacts(&[])
            .unwrap()
            .slider_events,
        SliderEndKind::Cancel,
    );
}

#[test]
fn buttonpad_promotion_cancels_before_passthrough_release() {
    let mut engine = engine();
    engine.set_buttonpad(true);
    start_drag(&mut engine);
    assert!(!engine.update_physical_button(BTN_LEFT, true).is_empty());
    let output = engine
        .process_frame(&[Event::x(700), Event::tracking_id(-1)])
        .unwrap();
    assert_end(&output.slider_events, SliderEndKind::Cancel);
    assert!(!output.passthrough.is_empty());
}

#[test]
fn hook_commands_are_optional_and_validated() {
    let config = EdgepadConfig::parse(CONFIG).unwrap();
    assert_eq!(
        config.sliders[0].on_release.as_ref().unwrap().argv,
        ["commit"]
    );
    assert_eq!(
        config.sliders[0].on_cancel.as_ref().unwrap().argv,
        ["cancel"]
    );
    let legacy = CONFIG
        .lines()
        .filter(|line| !line.starts_with("on_"))
        .collect::<Vec<_>>()
        .join("\n");
    let config = EdgepadConfig::parse(&legacy).unwrap();
    assert!(config.sliders[0].on_release.is_none());
    assert!(config.sliders[0].on_cancel.is_none());
    for name in ["on_release", "on_cancel"] {
        let error = EdgepadConfig::parse(&format!("{legacy}\n{name} = []")).unwrap_err();
        assert!(error.contains(name), "{error}");
    }
}

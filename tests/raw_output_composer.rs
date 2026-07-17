use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine};
use edgepad::raw::{
    route_raw_frame, RawEvent, RawFrame, RawOutputComposer, RoutedRawFrame, BTN_LEFT, EV_KEY,
};

fn test_capabilities() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 4,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

fn test_engine() -> Engine {
    Engine::new(test_capabilities(), EdgeWidths::all(0.10))
}

fn route_and_compose(
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    frame: RawFrame,
) -> Vec<RawEvent> {
    let routed = route_raw_frame(engine, &frame).expect("raw frame should route");
    composer
        .compose_frame(&routed)
        .expect("routed frame should compose")
        .events
}

#[test]
fn compose_frame_synthesizes_touch_and_position_from_passthrough_slot_not_raw_globals() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let output = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::btn_touch(true),
            RawEvent::abs_x(20),
            RawEvent::abs_y(600),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
            RawEvent::abs_x(500),
            RawEvent::abs_y(300),
        ]
    );
}

#[test]
fn compose_frame_does_not_leak_touch_or_abs_xy_for_claimed_edge_slot() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let output = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::btn_touch(true),
            RawEvent::abs_x(20),
            RawEvent::abs_y(300),
            RawEvent::abs_mt_tracking_id(101),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    assert!(output.is_empty());
}

#[test]
fn compose_frame_uses_center_slot_for_legacy_state_when_raw_abs_xy_belong_to_edge_slot() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let output = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
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
        ]),
    );

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(520),
            RawEvent::abs_mt_position_y(320),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
            RawEvent::abs_x(520),
            RawEvent::abs_y(320),
        ]
    );
}

#[test]
fn compose_frame_releases_legacy_state_when_last_passthrough_contact_ends() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    let output = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![RawEvent::abs_mt_tracking_id(-1)]),
    );

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ]
    );
}

#[test]
fn passthrough_release_carries_slot_context_after_claimed_slot_switch() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    let output = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(-1),
        ]),
    );

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ]
    );
}

#[test]
fn passthrough_start_carries_slot_context_after_claimed_slot_switch() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );
    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(-1),
        ]),
    );

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]),
    );
    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![RawEvent::abs_mt_tracking_id(-1)]),
    );

    let output = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_tracking_id(300),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(300),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
            RawEvent::abs_x(500),
            RawEvent::abs_y(300),
        ]
    );
}

#[test]
fn compose_frame_updates_tool_count_when_second_passthrough_contact_starts() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    let output = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(620),
            RawEvent::abs_mt_position_y(330),
        ]),
    );

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(620),
            RawEvent::abs_mt_position_y(330),
            RawEvent::btn_tool_finger(false),
            RawEvent::btn_tool_doubletap(true),
        ]
    );
}

#[test]
fn compose_frame_releases_each_active_passthrough_slot_on_resync() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );
    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(620),
            RawEvent::abs_mt_position_y(330),
        ]),
    );

    let output = composer
        .compose_frame(&RoutedRawFrame {
            passthrough: vec![],
            physical_buttons: vec![],
            gestures: vec![],
            slider_steps: vec![],
            resync_required: true,
        })
        .expect("resync should compose")
        .events;

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_doubletap(false),
        ]
    );
}

#[test]
fn compose_frame_forwards_physical_button_and_finish_releases_it() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let press = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]),
    );
    assert_eq!(press, vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]);

    let cleanup = composer
        .finish()
        .expect("finish should release a held physical button")
        .events;
    assert_eq!(cleanup, vec![RawEvent::new(EV_KEY, BTN_LEFT, 0)]);
    assert!(composer
        .finish()
        .expect("button cleanup should be idempotent")
        .events
        .is_empty());
}

#[test]
fn finish_releases_active_passthrough_contact_when_capture_stops_mid_contact() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());

    let _ = route_and_compose(
        &mut engine,
        &mut composer,
        RawFrame::new(vec![
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    );

    let output = composer
        .finish()
        .expect("finish should synthesize release for active passthrough contact")
        .events;

    assert_eq!(
        output,
        vec![
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ]
    );

    let second_finish = composer
        .finish()
        .expect("finish should be idempotent after cleanup")
        .events;
    assert!(second_finish.is_empty());
}

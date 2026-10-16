use super::*;
use crate::proxy::test_support::test_capabilities;
use crate::raw::{RawEvent, RawFrame};

#[test]
fn physical_touch_state_tracks_touch_lifecycle_from_raw_frames() {
    let capabilities = test_capabilities();
    let mut touch_state = PhysicalTouchState::new(capabilities);

    touch_state.observe_frame(&RawFrame::new(vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(10),
        RawEvent::btn_touch(true),
    ]));
    assert!(touch_state.is_touch_down());

    touch_state.observe_frame(&RawFrame::new(vec![
        RawEvent::abs_mt_slot(0),
        RawEvent::abs_mt_tracking_id(-1),
        RawEvent::btn_touch(false),
    ]));
    assert!(!touch_state.is_touch_down());
}

#[test]
fn syn_dropped_discards_partial_frame_and_ignores_events_until_syn_report() {
    let mut frames = FrameAssembler::default();
    let event = |raw| ProxyInputEvent {
        raw,
        timestamp: None,
    };
    assert!(matches!(
        frames.push(event(RawEvent::abs_mt_position_x(500))),
        FrameInput::Pending
    ));
    assert!(matches!(
        frames.push(event(RawEvent::syn_dropped())),
        FrameInput::Dropped(_)
    ));
    assert!(!frames.is_idle());
    assert!(matches!(
        frames.push(event(RawEvent::abs_mt_tracking_id(77))),
        FrameInput::Pending
    ));
    assert!(matches!(
        frames.push(event(RawEvent::syn_report())),
        FrameInput::Resync(None)
    ));
    assert!(frames.is_idle());
    assert!(matches!(
        frames.push(event(RawEvent::syn_report())),
        FrameInput::Boundary(None)
    ));
}

#[test]
fn physical_touch_state_stays_busy_until_resync_snapshot_arrives() {
    let mut state = PhysicalTouchState::new(test_capabilities());
    state.mark_desynchronized();
    assert!(state.is_touch_down());

    state.restore_contacts(&[]);
    assert!(!state.is_touch_down());
}

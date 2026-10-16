use super::*;
use edgepad::raw::{parse_raw_dump_file, BTN_LEFT, EV_KEY};

#[test]
fn raw_capture_recovers_from_a_dropped_click_drag_and_cleans_up_the_next_contact() {
    let capture = parse_raw_dump_file(
        r#"
# slots: 0..=4
# x: 0..=1000
# y: 0..=700
EV_ABS ABS_MT_SLOT 0
EV_ABS ABS_MT_TRACKING_ID 7
EV_ABS ABS_MT_POSITION_X 20
EV_ABS ABS_MT_POSITION_Y 300
EV_SYN SYN_REPORT 0 1000
EV_KEY BTN_LEFT 1
EV_ABS ABS_MT_POSITION_X 500
EV_ABS ABS_X 20
EV_SYN SYN_REPORT 0 2000
EV_SYN SYN_DROPPED 0 3000
EV_SYN SYN_REPORT 0 3000
EV_ABS ABS_MT_SLOT 1
EV_ABS ABS_MT_TRACKING_ID 20
EV_ABS ABS_MT_POSITION_X 500
EV_ABS ABS_MT_POSITION_Y 300
EV_SYN SYN_REPORT 0 4000
"#,
    )
    .unwrap();
    let caps = capture.capabilities.unwrap();
    let mut engine = Engine::new(caps, EdgeWidths::all(0.1));
    engine.set_buttonpad(true);
    let mut composer = RawOutputComposer::new(caps);
    let mut sink = RecordingRawOutputSink::default();
    let mut resync_seen = false;
    for frame in capture.frames {
        let routed = route_raw_frame(&mut engine, &frame).unwrap();
        assert!(routed.gestures.is_empty());
        resync_seen |= routed.resync_required;
        write_raw_output_frame(&mut composer, &routed, &mut sink).unwrap();
    }
    let cleanup = composer.finish().unwrap();
    sink.write_frame(&cleanup).unwrap();

    assert!(resync_seen);
    let frames = sink.frames();
    assert_eq!(frames.len(), 4);
    let click = &frames[0].events;
    assert_eq!(click.last(), Some(&RawEvent::new(EV_KEY, BTN_LEFT, 1)));
    assert!(click.contains(&RawEvent::abs_mt_tracking_id(7)));
    assert!(click.contains(&RawEvent::abs_mt_position_x(500)));
    assert!(click.contains(&RawEvent::abs_x(500)));
    assert!(!click.contains(&RawEvent::abs_x(20)));

    let dropped = &frames[1].events;
    assert!(dropped.contains(&RawEvent::abs_mt_tracking_id(-1)));
    assert!(dropped.contains(&RawEvent::btn_touch(false)));
    assert!(dropped.contains(&RawEvent::new(EV_KEY, BTN_LEFT, 0)));

    let resumed = &frames[2].events;
    assert!(resumed.contains(&RawEvent::abs_mt_tracking_id(20)));
    assert!(resumed.contains(&RawEvent::btn_touch(true)));
    assert!(resumed.contains(&RawEvent::abs_x(500)));
    assert!(resumed.contains(&RawEvent::abs_y(300)));
    let final_release = &frames[3].events;
    assert!(final_release.contains(&RawEvent::abs_mt_tracking_id(-1)));
    assert!(final_release.contains(&RawEvent::btn_touch(false)));
    assert!(composer.finish().unwrap().events.is_empty());
}

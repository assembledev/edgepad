use super::*;

#[test]
fn frame_writer_distinguishes_emit_failure_from_failed_commit() {
    let frame = RawFrame::new(vec![
        RawEvent::abs_mt_tracking_id(100),
        RawEvent::abs_mt_position_x(500),
    ]);
    let mut emit_failure = FailingSink {
        fail_on_emit: 1,
        ..FailingSink::default()
    };
    assert_eq!(
        emit_failure.write_frame(&frame),
        Err(RawFrameWriteError::Emit(TestSinkError("emit failed"))),
    );
    assert_eq!(emit_failure.emitted, [RawEvent::abs_mt_tracking_id(100)]);
    assert!(!emit_failure.sync_called);

    let mut sync_failure = FailingSink {
        fail_on_emit: usize::MAX,
        fail_on_sync: true,
        ..FailingSink::default()
    };
    assert_eq!(
        sync_failure.write_frame(&frame),
        Err(RawFrameWriteError::Sync(TestSinkError("sync failed"))),
    );
    assert_eq!(sync_failure.emitted, frame.events);
    assert!(sync_failure.sync_called);
}

#[test]
fn composed_frame_writer_propagates_a_failed_commit() {
    let mut composer = RawOutputComposer::new(test_capabilities());
    let mut sink = FailingSink {
        fail_on_emit: usize::MAX,
        fail_on_sync: true,
        ..FailingSink::default()
    };
    let result = write_raw_output_frame(
        &mut composer,
        &RoutedRawFrame {
            physical_buttons: vec![RawEvent::new(
                edgepad::raw::EV_KEY,
                edgepad::raw::BTN_LEFT,
                1,
            )],
            ..RoutedRawFrame::default()
        },
        &mut sink,
    );
    assert_eq!(
        result,
        Err(RawOutputError::Sink(TestSinkError("sync failed")))
    );
    assert!(sink.sync_called);
}

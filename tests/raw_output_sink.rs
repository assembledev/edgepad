use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine};
use edgepad::raw::{
    route_raw_frame, write_raw_output_frame, RawEvent, RawFrame, RawOutputComposer, RawOutputError,
    RawOutputSink, RecordingRawOutputSink, RoutedRawFrame,
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

fn route_and_write<S: RawOutputSink>(
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    frame: RawFrame,
) -> Result<(), RawOutputError<S::Error>> {
    let routed = route_raw_frame(engine, &frame).expect("raw frame should route");
    write_raw_output_frame(composer, &routed, sink)
}

#[test]
fn recording_sink_groups_non_empty_composed_output_by_sync_boundary() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());
    let mut sink = RecordingRawOutputSink::default();

    route_and_write(
        &mut engine,
        &mut composer,
        &mut sink,
        RawFrame::new(vec![
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
        ]),
    )
    .expect("center touch start should be written");

    route_and_write(
        &mut engine,
        &mut composer,
        &mut sink,
        RawFrame::new(vec![RawEvent::abs_mt_position_x(550)]),
    )
    .expect("center touch move should be written");

    route_and_write(
        &mut engine,
        &mut composer,
        &mut sink,
        RawFrame::new(vec![RawEvent::abs_mt_tracking_id(-1)]),
    )
    .expect("center touch release should be written");

    assert_eq!(
        sink.frames(),
        &[
            RawFrame::new(vec![
                RawEvent::abs_mt_tracking_id(100),
                RawEvent::abs_mt_position_x(500),
                RawEvent::abs_mt_position_y(300),
                RawEvent::btn_touch(true),
                RawEvent::btn_tool_finger(true),
                RawEvent::abs_x(500),
                RawEvent::abs_y(300),
            ]),
            RawFrame::new(vec![RawEvent::abs_mt_position_x(550), RawEvent::abs_x(550)]),
            RawFrame::new(vec![
                RawEvent::abs_mt_tracking_id(-1),
                RawEvent::btn_touch(false),
                RawEvent::btn_tool_finger(false),
            ]),
        ]
    );
}

#[test]
fn claimed_edge_contacts_do_not_create_empty_sink_frames() {
    let mut engine = test_engine();
    let mut composer = RawOutputComposer::new(test_capabilities());
    let mut sink = RecordingRawOutputSink::default();

    route_and_write(
        &mut engine,
        &mut composer,
        &mut sink,
        RawFrame::new(vec![
            RawEvent::btn_touch(true),
            RawEvent::abs_x(20),
            RawEvent::abs_y(300),
            RawEvent::abs_mt_tracking_id(101),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
        ]),
    )
    .expect("edge touch start should be swallowed");

    route_and_write(
        &mut engine,
        &mut composer,
        &mut sink,
        RawFrame::new(vec![
            RawEvent::abs_mt_position_x(30),
            RawEvent::abs_mt_position_y(180),
        ]),
    )
    .expect("edge touch move should be swallowed");

    route_and_write(
        &mut engine,
        &mut composer,
        &mut sink,
        RawFrame::new(vec![RawEvent::abs_mt_tracking_id(-1)]),
    )
    .expect("edge touch release should be swallowed");

    assert_eq!(sink.frames(), &[]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestSinkError(&'static str);

#[derive(Default)]
struct FailingSink {
    emitted: Vec<RawEvent>,
    fail_on_emit: usize,
    sync_called: bool,
}

impl RawOutputSink for FailingSink {
    type Error = TestSinkError;

    fn emit(&mut self, event: RawEvent) -> Result<(), Self::Error> {
        if self.emitted.len() == self.fail_on_emit {
            return Err(TestSinkError("emit failed"));
        }
        self.emitted.push(event);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Self::Error> {
        self.sync_called = true;
        Ok(())
    }
}

#[test]
fn sink_emit_errors_are_returned_without_syncing_partial_frames() {
    let mut composer = RawOutputComposer::new(test_capabilities());
    let mut sink = FailingSink {
        fail_on_emit: 1,
        ..FailingSink::default()
    };

    let result = write_raw_output_frame(
        &mut composer,
        &RoutedRawFrame {
            passthrough: vec![
                RawEvent::abs_mt_tracking_id(100),
                RawEvent::abs_mt_position_x(500),
                RawEvent::abs_mt_position_y(300),
            ],
            gestures: vec![],
            resync_required: false,
        },
        &mut sink,
    );

    assert_eq!(
        result,
        Err(RawOutputError::Sink(TestSinkError("emit failed")))
    );
    assert_eq!(sink.emitted, vec![RawEvent::abs_mt_tracking_id(100)]);
    assert!(!sink.sync_called);
}

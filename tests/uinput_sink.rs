use edgepad::raw::{
    RawEvent, RawOutputSink, ABS_MT_POSITION_X, ABS_MT_TRACKING_ID, ABS_X, EV_ABS, EV_SYN,
    SYN_REPORT,
};
use edgepad::uinput::{UinputEventWriter, UinputRawOutputSink, UinputRawSinkError};
use evdev::InputEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestWriterError(&'static str);

#[derive(Default)]
struct RecordingWriter {
    batches: Vec<Vec<InputEvent>>,
    fail_next_emit: bool,
}

impl UinputEventWriter for RecordingWriter {
    type Error = TestWriterError;

    fn emit_events(&mut self, events: &[InputEvent]) -> Result<(), Self::Error> {
        if self.fail_next_emit {
            self.fail_next_emit = false;
            return Err(TestWriterError("emit failed"));
        }
        self.batches.push(events.to_vec());
        Ok(())
    }
}

fn event_triples(events: &[InputEvent]) -> Vec<(u16, u16, i32)> {
    events
        .iter()
        .map(|event| (event.event_type().0, event.code(), event.value()))
        .collect()
}

#[test]
fn uinput_sink_flushes_buffered_raw_events_as_one_batch_on_sync() {
    let mut sink = UinputRawOutputSink::new(RecordingWriter::default());

    sink.emit(RawEvent::abs_mt_tracking_id(100))
        .expect("tracking id should buffer");
    sink.emit(RawEvent::abs_mt_position_x(500))
        .expect("mt x should buffer");
    sink.emit(RawEvent::abs_x(500))
        .expect("legacy x should buffer");
    sink.sync().expect("sync should flush one batch");

    let writer = sink.into_inner();
    assert_eq!(writer.batches.len(), 1);
    assert_eq!(
        event_triples(&writer.batches[0]),
        vec![
            (EV_ABS, ABS_MT_TRACKING_ID, 100),
            (EV_ABS, ABS_MT_POSITION_X, 500),
            (EV_ABS, ABS_X, 500),
        ]
    );
    assert!(
        !event_triples(&writer.batches[0])
            .iter()
            .any(|(kind, code, _)| *kind == EV_SYN && *code == SYN_REPORT),
        "evdev VirtualDevice::emit appends SYN_REPORT itself; sink must not duplicate it"
    );
}

#[test]
fn uinput_sink_ignores_empty_sync_without_calling_writer() {
    let mut sink = UinputRawOutputSink::new(RecordingWriter::default());

    sink.sync().expect("empty sync should be a no-op");

    let writer = sink.into_inner();
    assert!(writer.batches.is_empty());
}

#[test]
fn uinput_sink_propagates_writer_errors() {
    let mut sink = UinputRawOutputSink::new(RecordingWriter {
        fail_next_emit: true,
        ..RecordingWriter::default()
    });

    sink.emit(RawEvent::abs_mt_tracking_id(100))
        .expect("buffering should not touch writer");

    assert_eq!(
        sink.sync(),
        Err(UinputRawSinkError::Emit(TestWriterError("emit failed")))
    );
}

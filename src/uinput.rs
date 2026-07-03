use crate::raw::{RawEvent, RawOutputSink};
use evdev::InputEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UinputRawSinkError<E> {
    Emit(E),
}

pub trait UinputEventWriter {
    type Error;

    fn emit_events(&mut self, events: &[InputEvent]) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone)]
pub struct UinputRawOutputSink<W> {
    writer: W,
    current: Vec<InputEvent>,
}

impl<W> UinputRawOutputSink<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            current: Vec::new(),
        }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W> RawOutputSink for UinputRawOutputSink<W>
where
    W: UinputEventWriter,
{
    type Error = UinputRawSinkError<W::Error>;

    fn emit(&mut self, event: RawEvent) -> Result<(), Self::Error> {
        self.current
            .push(InputEvent::new(event.kind, event.code, event.value));
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Self::Error> {
        if self.current.is_empty() {
            return Ok(());
        }

        self.writer
            .emit_events(&self.current)
            .map_err(UinputRawSinkError::Emit)?;
        self.current.clear();
        Ok(())
    }
}

impl UinputEventWriter for evdev::uinput::VirtualDevice {
    type Error = std::io::Error;

    fn emit_events(&mut self, events: &[InputEvent]) -> Result<(), Self::Error> {
        self.emit(events)
    }
}

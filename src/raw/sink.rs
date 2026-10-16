use crate::core::SlotError;

use super::events::{RawEvent, RawFrame};
use super::output::RawOutputComposer;
use super::routing::RoutedRawFrame;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawOutputError<E> {
    Compose(SlotError),
    Sink(E),
}

/// Frame output boundary shared by the recording sink and uinput.
/// `emit` receives frame contents; `sync` commits the frame. Callers do not emit
/// `SYN_REPORT` themselves, since the live writer supplies it.
pub trait RawOutputSink {
    type Error;

    fn emit(&mut self, event: RawEvent) -> Result<(), Self::Error>;
    fn sync(&mut self) -> Result<(), Self::Error>;

    /// Commit a nonempty frame once. An emit error stops before `sync`.
    fn write_frame(&mut self, frame: &RawFrame) -> Result<(), RawFrameWriteError<Self::Error>> {
        if frame.events.is_empty() {
            return Ok(());
        }
        for event in frame.events.iter().copied() {
            self.emit(event).map_err(RawFrameWriteError::Emit)?;
        }
        self.sync().map_err(RawFrameWriteError::Sync)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordingRawOutputSink {
    frames: Vec<RawFrame>,
    current: Vec<RawEvent>,
}

impl RecordingRawOutputSink {
    pub fn frames(&self) -> &[RawFrame] {
        &self.frames
    }

    pub fn into_frames(self) -> Vec<RawFrame> {
        self.frames
    }
}

impl RawOutputSink for RecordingRawOutputSink {
    type Error = std::convert::Infallible;

    fn emit(&mut self, event: RawEvent) -> Result<(), Self::Error> {
        self.current.push(event);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Self::Error> {
        if !self.current.is_empty() {
            self.frames
                .push(RawFrame::new(std::mem::take(&mut self.current)));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawFrameWriteError<E> {
    Emit(E),
    Sync(E),
}

impl<E: std::fmt::Debug> std::fmt::Display for RawFrameWriteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Emit(error) => write!(f, "emit failed: {error:?}"),
            Self::Sync(error) => write!(f, "sync failed: {error:?}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RawFrameWriteError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Emit(error) | Self::Sync(error) => Some(error),
        }
    }
}

pub fn write_raw_output_frame<S>(
    composer: &mut RawOutputComposer,
    routed: &RoutedRawFrame,
    sink: &mut S,
) -> Result<(), RawOutputError<S::Error>>
where
    S: RawOutputSink,
{
    let frame = composer
        .compose_frame(routed)
        .map_err(RawOutputError::Compose)?;
    sink.write_frame(&frame).map_err(|error| match error {
        RawFrameWriteError::Emit(error) | RawFrameWriteError::Sync(error) => {
            RawOutputError::Sink(error)
        }
    })
}

//! Adapt evdev events to the recognizer, then compose the virtual touchpad stream.
//!
//! Routing decides which contacts survive. Composition rebuilds pointer-emulation
//! state from those contacts; copying the physical device's global state would
//! let an edge-owned finger affect the pointer. Both stages run in raw replay.

mod events;
mod output;
mod parse;
mod routing;
mod sink;

pub use events::*;
pub use output::RawOutputComposer;
pub use parse::{parse_raw_dump_file, parse_raw_frames, RawDumpFile, RawParseError};
pub use routing::{
    extract_core_events, route_raw_frame, route_recognition_deadline, route_resync_contacts,
    RoutedRawFrame,
};
pub use sink::{
    write_raw_output_frame, RawFrameWriteError, RawOutputError, RawOutputSink,
    RecordingRawOutputSink,
};

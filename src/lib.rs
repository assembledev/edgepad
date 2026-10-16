//! Touchpad edge recognition and input forwarding.
//!
//! Start with [`core::Engine`] for gesture behavior. It consumes complete input
//! frames and decides which contacts belong to edge gestures or normal input.
//! It does not open devices, read clocks, or execute commands.
//!
//! The live path is [`proxy`] -> [`raw::route_raw_frame`] -> [`core::Engine`] ->
//! [`raw::RawOutputComposer`] -> [`uinput`]. The proxy also sends recognized
//! gestures to [`actions`]; [`config`] supplies bindings and recognition settings.
//! [`replay`] and raw replay exercise recognition and routing without hardware.
//!
//! Regression tests are grouped by boundary: `tests/core_invariants.rs` covers
//! recognition, `tests/raw_output_composer.rs` covers virtual input, and
//! `tests/replay_fixtures.rs` runs recorded or handwritten event sequences.

mod capture_metadata;
mod diagnostics;

pub mod actions;
pub mod config;
pub mod device;
pub mod doctor;
pub mod dump;
pub mod notify;
pub mod proxy;
pub mod raw;
pub mod replay;
pub mod status;
pub mod uinput;

/// Gesture recognition over Linux Type-B multitouch slots.
///
/// A slot retains axis values between contacts; a tracking ID identifies the
/// contact currently occupying it. Keep slot state and contact state distinct.
pub mod core;

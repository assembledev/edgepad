//! Recognizer state and construction.
//!
//! Read frame.rs first: it resolves tool types for a whole frame, then routes
//! events and finalizes new contacts. routing.rs decides ownership; contact.rs
//! classifies releases; sliders.rs emits during motion; taps.rs can defer a
//! released tap until an input-time deadline.
//!
//! Split Engine impls share the private state below. Keep cross-module helpers
//! scoped to core so the file layout does not become part of the public API.

use std::collections::BTreeSet;

mod contact;
mod frame;
mod routing;
mod sliders;
mod slot;
mod taps;
mod types;

pub(crate) use types::SlotSnapshot;
pub use types::{
    AxisRange, Capabilities, EdgeWidths, EngineOptions, Event, FrameOutput, Gesture,
    GestureDirection, ResyncContact, SliderAxis, SliderDirection, SliderSpec, SliderStep,
    SlotError, Zone, ZoneSet, DEFAULT_DOUBLE_TAP_MAX_DISTANCE, DEFAULT_DOUBLE_TAP_TIMEOUT_MS,
    DEFAULT_PALM_REJECTION, DEFAULT_SWIPE_MIN_DISTANCE, DEFAULT_TAP_MAX_DURATION_MS,
    DEFAULT_TAP_MIN_DURATION_MS, MT_TOOL_PALM,
};

use contact::{Contact, ContactPhase, Ownership};
use sliders::validate_slider_specs;
use slot::SlotState;
use taps::PendingTap;

/// Stateful recognizer shared by live input and replay.
///
/// Feed complete frames with [`Self::process_frame_at`]. The caller must also
/// service [`Self::next_deadline`] through [`Self::advance_time`] when no input
/// arrives, or a pending single tap can wait indefinitely.
#[derive(Debug, Clone)]
pub struct Engine {
    caps: Capabilities,
    edges: EdgeWidths,
    options: EngineOptions,
    sliders: Vec<SliderSpec>,
    buttonpad: bool,
    pressed_physical_buttons: BTreeSet<u16>,
    current_slot: i32,
    slots: Vec<SlotState>,
    pending_tap: Option<PendingTap>,
}

impl Engine {
    pub fn new(caps: Capabilities, edges: EdgeWidths) -> Self {
        assert!(
            caps.slot_min <= caps.slot_max,
            "invalid slot range: {}..={} ",
            caps.slot_min,
            caps.slot_max
        );
        let slot_count = (caps.slot_max - caps.slot_min + 1) as usize;
        Self {
            current_slot: caps.slot_min,
            caps,
            edges,
            options: EngineOptions::default(),
            sliders: Vec::new(),
            buttonpad: false,
            pressed_physical_buttons: BTreeSet::new(),
            slots: vec![SlotState::default(); slot_count],
            pending_tap: None,
        }
    }

    pub fn with_sliders(caps: Capabilities, edges: EdgeWidths, sliders: Vec<SliderSpec>) -> Self {
        validate_slider_specs(&sliders);
        let mut engine = Self::new(caps, edges);
        engine.sliders = sliders;
        engine
    }

    pub fn with_options(
        caps: Capabilities,
        edges: EdgeWidths,
        sliders: Vec<SliderSpec>,
        options: EngineOptions,
    ) -> Self {
        validate_engine_options(options);
        let mut engine = Self::with_sliders(caps, edges, sliders);
        engine.options = options;
        engine
    }

    pub(crate) fn reconfigure_recognition(
        &mut self,
        edges: EdgeWidths,
        sliders: Vec<SliderSpec>,
        options: EngineOptions,
    ) {
        // Preserve retained slot axes and button state across reload. Only recognition
        // policy changes, after contacts and deferred taps have drained.
        assert!(
            self.slots.iter().all(|slot| slot.contact.is_none()) && self.pending_tap.is_none(),
            "recognition reconfiguration requires an idle engine"
        );
        validate_slider_specs(&sliders);
        validate_engine_options(options);
        self.edges = edges;
        self.sliders = sliders;
        self.options = options;
    }

    pub(crate) fn seed_slot_snapshots(
        &mut self,
        positions: &[SlotSnapshot],
    ) -> Result<(), SlotError> {
        // Inactive slots also retain kernel axes/tool type. Seed them without
        // inventing contacts so an unchanged first touch can omit those values.
        for position in positions {
            let tool_type = position.tool_type.filter(|_| self.options.palm_rejection);
            let slot = self.slot_mut(position.slot)?;
            slot.memory.x = Some(position.x);
            slot.memory.y = Some(position.y);
            slot.memory.tool_type = tool_type;
        }
        Ok(())
    }

    /// Restore a kernel snapshot after lost events. Existing contacts pass
    /// through until lift because their missing history cannot prove a gesture.
    pub fn restore_passthrough_contacts(
        &mut self,
        contacts: &[ResyncContact],
    ) -> Result<FrameOutput, SlotError> {
        self.reset_for_resync();
        let mut output = FrameOutput::empty();

        for contact in contacts {
            self.ensure_slot(contact.slot)?;
            self.current_slot = contact.slot;
            let tool_type = contact.tool_type.filter(|_| self.options.palm_rejection);
            let slot = self.slot_mut(contact.slot)?;
            slot.restore_contact(contact, tool_type);

            output.passthrough.extend([
                Event::slot(contact.slot),
                Event::tracking_id(contact.tracking_id),
                Event::x(contact.x),
                Event::y(contact.y),
            ]);
            if let Some(tool_type) = tool_type {
                output.passthrough.push(Event::tool_type(tool_type));
            }
        }

        Ok(output)
    }

    fn ensure_slot(&self, slot: i32) -> Result<(), SlotError> {
        if slot < self.caps.slot_min || slot > self.caps.slot_max {
            return Err(SlotError::SlotOutOfRange {
                slot,
                min: self.caps.slot_min,
                max: self.caps.slot_max,
            });
        }
        Ok(())
    }

    fn reset_for_resync(&mut self) {
        // Lost input invalidates even retained slot memory. The live proxy replaces
        // it from a kernel snapshot before continuing.
        for slot in &mut self.slots {
            *slot = SlotState::default();
        }
        self.pressed_physical_buttons.clear();
        self.current_slot = self.caps.slot_min;
        self.pending_tap = None;
    }

    fn slot_index(&self, slot: i32) -> Result<usize, SlotError> {
        self.ensure_slot(slot)?;
        Ok((slot - self.caps.slot_min) as usize)
    }

    fn slot(&self, slot: i32) -> Result<&SlotState, SlotError> {
        let index = self.slot_index(slot)?;
        Ok(&self.slots[index])
    }

    fn slot_mut(&mut self, slot: i32) -> Result<&mut SlotState, SlotError> {
        let index = self.slot_index(slot)?;
        Ok(&mut self.slots[index])
    }
}

fn validate_engine_options(options: EngineOptions) {
    assert!(
        options.tap_min_duration < options.tap_max_duration,
        "tap minimum duration must be below maximum duration"
    );
    assert!(
        !options.double_tap_timeout.is_zero(),
        "double-tap timeout must be positive"
    );
    assert!(
        options.double_tap_max_distance.is_finite()
            && options.double_tap_max_distance > 0.0
            && options.double_tap_max_distance <= 1.0,
        "double-tap distance must be > 0 and <= 1"
    );
}

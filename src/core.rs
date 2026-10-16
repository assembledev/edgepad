//! Recognizer state and construction. Private modules own frame processing,
//! contact handling, routing, sliders, and tap arbitration.

use std::collections::BTreeSet;
use std::time::Duration;

mod contact;
mod frame;
mod routing;
mod sliders;
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

use sliders::validate_slider_specs;
use taps::PendingTap;

// Ownership normally lasts until lift: moving across an edge must not steal
// an existing pointer contact. A physical click on a buttonpad can promote
// an edge contact to passthrough so click-drag still works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ownership {
    // Buffer events until the initial position is known at a frame boundary.
    Unknown,
    Claimed(Zone),
    Passthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContactPhase {
    TapCandidate,
    Motion,
    ConsumedBySlider,
}

#[derive(Debug, Clone)]
struct SlotState {
    active: bool,
    tracking_id: Option<i32>,
    ownership: Ownership,
    contact_phase: ContactPhase,
    start_x: Option<i32>,
    start_y: Option<i32>,
    current_x: Option<i32>,
    current_y: Option<i32>,
    retained_x: Option<i32>,
    retained_y: Option<i32>,
    tool_type: Option<i32>,
    gesture_rejected: bool,
    began_in_frame: bool,
    started_at: Option<Duration>,
    tap_sequence_eligible: bool,
    tap_sequence_match: bool,
    slider_anchor: Option<f32>,
    held_events: Vec<Event>,
}

impl Default for SlotState {
    fn default() -> Self {
        Self {
            active: false,
            tracking_id: None,
            ownership: Ownership::Unknown,
            contact_phase: ContactPhase::TapCandidate,
            start_x: None,
            start_y: None,
            current_x: None,
            current_y: None,
            retained_x: None,
            retained_y: None,
            tool_type: None,
            gesture_rejected: false,
            began_in_frame: false,
            started_at: None,
            tap_sequence_eligible: true,
            tap_sequence_match: false,
            slider_anchor: None,
            held_events: Vec::new(),
        }
    }
}

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
        assert!(
            self.slots.iter().all(|slot| !slot.active) && self.pending_tap.is_none(),
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
        for position in positions {
            let tool_type = position.tool_type.filter(|_| self.options.palm_rejection);
            let slot = self.slot_mut(position.slot)?;
            slot.retained_x = Some(position.x);
            slot.retained_y = Some(position.y);
            slot.tool_type = tool_type;
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
            slot.active = true;
            slot.tracking_id = Some(contact.tracking_id);
            slot.ownership = Ownership::Passthrough;
            slot.start_x = Some(contact.x);
            slot.start_y = Some(contact.y);
            slot.current_x = Some(contact.x);
            slot.current_y = Some(contact.y);
            slot.retained_x = Some(contact.x);
            slot.retained_y = Some(contact.y);
            slot.tool_type = tool_type;
            if tool_type == Some(MT_TOOL_PALM) {
                slot.reject_gestures();
            }

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

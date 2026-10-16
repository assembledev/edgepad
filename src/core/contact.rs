//! Active contact state and release classification.

use std::time::Duration;

use super::{
    Capabilities, Engine, EngineOptions, Event, FrameOutput, Gesture, GestureDirection,
    ResyncContact, SlotError, Zone, MT_TOOL_PALM,
};

// Ownership lasts until lift. A buttonpad press can promote an edge contact
// to passthrough so it can participate in click-drag or clickfinger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ownership {
    Unknown,
    Claimed(Zone),
    Passthrough,
}

// Returning to the start cannot undo motion or a slider step and produce a tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContactPhase {
    TapCandidate,
    Motion,
    ConsumedBySlider,
}

#[derive(Debug, Clone)]
pub(super) struct Contact {
    pub(super) tracking_id: i32,
    pub(super) ownership: Ownership,
    pub(super) contact_phase: ContactPhase,
    pub(super) start_x: Option<i32>,
    pub(super) start_y: Option<i32>,
    pub(super) current_x: Option<i32>,
    pub(super) current_y: Option<i32>,
    pub(super) gesture_rejected: bool,
    pub(super) began_in_frame: bool,
    pub(super) started_at: Option<Duration>,
    // Eligibility permits double-tap pairing; a match identifies the second contact.
    pub(super) tap_sequence_eligible: bool,
    pub(super) tap_sequence_match: bool,
    pub(super) slider_anchor: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ClassifiedContact {
    pub(super) gesture: Gesture,
    pub(super) sequence_match: bool,
    pub(super) sequence_eligible: bool,
    pub(super) x: Option<i32>,
    pub(super) y: Option<i32>,
}

impl Contact {
    pub(super) fn new(
        tracking_id: i32,
        started_at: Option<Duration>,
        sequence_eligible: bool,
    ) -> Self {
        Self {
            tracking_id,
            ownership: Ownership::Unknown,
            contact_phase: ContactPhase::TapCandidate,
            start_x: None,
            start_y: None,
            current_x: None,
            current_y: None,
            gesture_rejected: false,
            began_in_frame: true,
            started_at,
            tap_sequence_eligible: sequence_eligible,
            tap_sequence_match: false,
            slider_anchor: None,
        }
    }

    pub(super) fn restored(snapshot: &ResyncContact, tool_type: Option<i32>) -> Self {
        let mut contact = Self {
            ownership: Ownership::Passthrough,
            start_x: Some(snapshot.x),
            start_y: Some(snapshot.y),
            current_x: Some(snapshot.x),
            current_y: Some(snapshot.y),
            began_in_frame: false,
            ..Self::new(snapshot.tracking_id, None, true)
        };
        if tool_type == Some(MT_TOOL_PALM) {
            contact.reject_gestures();
        }
        contact
    }

    pub(super) fn reject_gestures(&mut self) {
        // A palm label suppresses recognition until lift, but passthrough contacts
        // still need their coordinates, tool type, and release forwarded.
        self.gesture_rejected = true;
        self.tap_sequence_eligible = false;
        self.tap_sequence_match = false;
        self.slider_anchor = None;
    }

    pub(super) fn observe_position(&mut self, capabilities: Capabilities, swipe_min_distance: f32) {
        if self.gesture_rejected || self.contact_phase != ContactPhase::TapCandidate {
            return;
        }

        let moved_x = match (self.start_x, self.current_x) {
            (Some(start), Some(current)) => {
                capabilities.x.normalize_delta(start, current).abs() >= swipe_min_distance
            }
            _ => false,
        };
        let moved_y = match (self.start_y, self.current_y) {
            (Some(start), Some(current)) => {
                capabilities.y.normalize_delta(start, current).abs() >= swipe_min_distance
            }
            _ => false,
        };

        if moved_x || moved_y {
            self.contact_phase = ContactPhase::Motion;
        }
    }

    pub(super) fn consume_by_slider(&mut self) {
        self.contact_phase = ContactPhase::ConsumedBySlider;
    }
}

impl Engine {
    pub(super) fn finalize_contacts_started_in_frame(
        &mut self,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let previous_slot = self.current_slot;
        for index in 0..self.slots.len() {
            let slot_number = self.caps.slot_min + index as i32;
            if self.slots[index].finalize_new_contact() {
                self.current_slot = slot_number;
                self.decide_ownership_if_ready(output)?;
                self.emit_slider_steps_if_ready(output)?;
            }
        }
        self.current_slot = previous_slot;
        Ok(())
    }

    pub(super) fn release_current_slot(
        &mut self,
        event: Event,
        timestamp: Option<Duration>,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let slot = self.current_slot;
        let Some(contact) = self.slot_mut(slot)?.take_contact() else {
            self.slot_mut(slot)?.held_events.push(event);
            return Ok(());
        };
        if contact.gesture_rejected {
            if contact.ownership == Ownership::Passthrough {
                self.push_passthrough_event_for_current_slot(event, output);
            }
            return Ok(());
        }
        match contact.ownership {
            Ownership::Claimed(zone) => {
                let releases_slider_zone = self.slider_for_zone(zone).is_some();
                let classified =
                    classify_gesture(self.caps, slot, zone, &contact, self.options, timestamp).map(
                        |gesture| ClassifiedContact {
                            gesture,
                            sequence_match: contact.tap_sequence_match,
                            sequence_eligible: contact.tap_sequence_eligible,
                            x: contact.current_x.or(contact.start_x),
                            y: contact.current_y.or(contact.start_y),
                        },
                    );

                if let Some(classified) = classified {
                    // Sliders own directional motion for their zone. Release can
                    // still be a tap when no slider step consumed the contact.
                    let allowed_for_slider = !releases_slider_zone
                        || matches!(
                            classified.gesture.direction,
                            GestureDirection::Tap | GestureDirection::DoubleTap
                        );
                    if allowed_for_slider {
                        self.handle_classified_gesture(classified, timestamp, output);
                    } else {
                        self.flush_pending_tap(output);
                    }
                } else if self.pending_tap.is_some() {
                    self.flush_pending_tap(output);
                }
            }
            Ownership::Passthrough => {
                self.push_passthrough_event_for_current_slot(event, output);
            }
            Ownership::Unknown => {}
        }
        Ok(())
    }
}

fn classify_gesture(
    capabilities: Capabilities,
    slot: i32,
    zone: Zone,
    state: &Contact,
    options: EngineOptions,
    released_at: Option<Duration>,
) -> Option<Gesture> {
    if state.gesture_rejected {
        return None;
    }
    let start_x = state.start_x?;
    let start_y = state.start_y?;
    let current_x = state.current_x.unwrap_or(start_x);
    let current_y = state.current_y.unwrap_or(start_y);
    let dx = capabilities.x.normalize_delta(start_x, current_x);
    let dy = capabilities.y.normalize_delta(start_y, current_y);

    let final_direction = direction_for_displacement(dx, dy, options.swipe_min_distance);
    let direction = match state.contact_phase {
        ContactPhase::TapCandidate => match final_direction {
            Some(direction) => direction,
            None => {
                if !tap_duration_is_valid(
                    state.started_at,
                    released_at,
                    options.tap_min_duration,
                    options.tap_max_duration,
                ) {
                    return None;
                }
                GestureDirection::Tap
            }
        },
        ContactPhase::Motion => final_direction?,
        ContactPhase::ConsumedBySlider => return None,
    };

    Some(Gesture {
        zone,
        direction,
        slot,
        tracking_id: state.tracking_id,
    })
}

fn direction_for_displacement(
    dx: f32,
    dy: f32,
    swipe_min_distance: f32,
) -> Option<GestureDirection> {
    if dx.abs() < swipe_min_distance && dy.abs() < swipe_min_distance {
        return None;
    }

    Some(if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            GestureDirection::Right
        } else {
            GestureDirection::Left
        }
    } else if dy >= 0.0 {
        GestureDirection::Down
    } else {
        GestureDirection::Up
    })
}

fn tap_duration_is_valid(
    started_at: Option<Duration>,
    released_at: Option<Duration>,
    min_duration: Duration,
    max_duration: Duration,
) -> bool {
    match (started_at, released_at) {
        (Some(started_at), Some(released_at)) => released_at
            .checked_sub(started_at)
            .is_some_and(|duration| duration >= min_duration && duration < max_duration),
        _ => true,
    }
}

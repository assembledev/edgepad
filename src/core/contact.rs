//! Contact completion and gesture classification, including retained slot axes.

use std::time::Duration;

use super::{
    Capabilities, ContactPhase, Engine, EngineOptions, Event, FrameOutput, Gesture,
    GestureDirection, Ownership, SlotError, SlotState, Zone,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct ClassifiedContact {
    pub(super) gesture: Gesture,
    pub(super) sequence_match: bool,
    pub(super) sequence_eligible: bool,
    pub(super) x: Option<i32>,
    pub(super) y: Option<i32>,
}

impl SlotState {
    pub(super) fn finish_contact(&mut self) {
        // Type-B slots retain axis state after TRACKING_ID -1. Linux may
        // suppress unchanged positions or tool type when the next contact
        // begins. Retain those axes while clearing contact-owned state.
        let retained_x = self.retained_x;
        let retained_y = self.retained_y;
        let tool_type = self.tool_type;
        *self = Self {
            retained_x,
            retained_y,
            tool_type,
            ..Self::default()
        };
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
            let should_finalize = {
                let slot = &mut self.slots[index];
                if !slot.active || !slot.began_in_frame {
                    false
                } else {
                    // Resolve any omitted axes only after the whole SYN
                    // frame. Doing it at TRACKING_ID would briefly classify
                    // a moved contact from the preceding contact's position.
                    slot.current_x = slot.current_x.or(slot.retained_x);
                    slot.current_y = slot.current_y.or(slot.retained_y);
                    slot.start_x = slot.start_x.or(slot.current_x);
                    slot.start_y = slot.start_y.or(slot.current_y);
                    slot.began_in_frame = false;
                    true
                }
            };
            if should_finalize {
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
        if self.slot(slot)?.gesture_rejected {
            if self.slot(slot)?.ownership == Ownership::Passthrough {
                self.push_passthrough_event_for_current_slot(event, output);
            }
            self.slot_mut(slot)?.finish_contact();
            return Ok(());
        }
        match self.slot(slot)?.ownership {
            Ownership::Claimed(zone) => {
                let releases_slider_zone = self.slider_for_zone(zone).is_some();
                let options = self.options;
                let capabilities = self.caps;
                let classified = {
                    let slot_state = self.slot(slot)?;
                    classify_gesture(capabilities, slot, zone, slot_state, options, timestamp).map(
                        |gesture| ClassifiedContact {
                            gesture,
                            sequence_match: slot_state.tap_sequence_match,
                            sequence_eligible: slot_state.tap_sequence_eligible,
                            x: slot_state.current_x.or(slot_state.start_x),
                            y: slot_state.current_y.or(slot_state.start_y),
                        },
                    )
                };
                if self.slot(slot)?.active {
                    self.slot_mut(slot)?.finish_contact();
                }

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
                if self.slot(slot)?.active {
                    self.slot_mut(slot)?.finish_contact();
                }
            }
            Ownership::Unknown => {
                let slot_state = self.slot_mut(slot)?;
                slot_state.held_events.push(event);
                if slot_state.active {
                    slot_state.finish_contact();
                }
            }
        }
        Ok(())
    }
}

fn classify_gesture(
    capabilities: Capabilities,
    slot: i32,
    zone: Zone,
    state: &SlotState,
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
        tracking_id: state.tracking_id?,
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

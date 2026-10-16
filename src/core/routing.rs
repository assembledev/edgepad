//! Contact ownership, passthrough slot routing, and buttonpad promotion.

use super::sliders::slider_position;
use super::{Engine, Event, FrameOutput, Ownership, SlotError, Zone};

impl Engine {
    pub fn set_buttonpad(&mut self, buttonpad: bool) {
        self.buttonpad = buttonpad;
        if !buttonpad {
            self.pressed_physical_buttons.clear();
        }
    }

    pub fn update_physical_button(&mut self, code: u16, pressed: bool) -> Vec<Event> {
        if !self.buttonpad {
            return Vec::new();
        }

        if !pressed {
            self.pressed_physical_buttons.remove(&code);
            return Vec::new();
        }

        self.pressed_physical_buttons.insert(code);
        self.promote_claimed_contacts()
    }

    pub fn restore_pressed_physical_buttons(&mut self, codes: &[u16]) {
        self.pressed_physical_buttons.clear();
        if self.buttonpad {
            self.pressed_physical_buttons.extend(codes.iter().copied());
        }
    }

    #[cfg(test)]
    pub(crate) fn pressed_physical_buttons(&self) -> Vec<u16> {
        self.pressed_physical_buttons.iter().copied().collect()
    }

    pub(super) fn route_event_for_current_slot(
        &mut self,
        event: Event,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let slot_state = self.slot_mut(self.current_slot)?;
        match slot_state.ownership {
            Ownership::Passthrough => self.push_passthrough_event_for_current_slot(event, output),
            Ownership::Unknown => slot_state.held_events.push(event),
            Ownership::Claimed(_) => {}
        }
        Ok(())
    }

    pub(super) fn push_passthrough_event_for_current_slot(
        &self,
        event: Event,
        output: &mut FrameOutput,
    ) {
        self.push_passthrough_event_for_slot(self.current_slot, event, output);
    }

    fn push_passthrough_event_for_slot(&self, slot: i32, event: Event, output: &mut FrameOutput) {
        match event {
            Event::Slot(_) | Event::SynDropped => output.passthrough.push(event),
            Event::TrackingId(_) | Event::X(_) | Event::Y(_) | Event::ToolType(_) => {
                // Filtering edge contacts also filters slot switches. The
                // output slot can therefore differ from the physical slot;
                // emitting a bare release here can leave the wrong finger down.
                if last_passthrough_slot(&output.passthrough) != Some(slot) {
                    output.passthrough.push(Event::slot(slot));
                }
                output.passthrough.push(event);
            }
        }
    }

    pub(super) fn decide_ownership_if_ready(
        &mut self,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let slot = self.current_slot;
        let zone = if self.buttonpad && !self.pressed_physical_buttons.is_empty() {
            None
        } else {
            self.zone_for_current_slot()?
        };
        let Some(zone) = zone else {
            let (held_events, became_passthrough) = {
                let slot_state = self.slot_mut(slot)?;
                if slot_state.active
                    && matches!(slot_state.ownership, Ownership::Unknown)
                    && slot_state.start_x.is_some()
                    && slot_state.start_y.is_some()
                {
                    slot_state.ownership = Ownership::Passthrough;
                    (std::mem::take(&mut slot_state.held_events), true)
                } else {
                    (Vec::new(), false)
                }
            };
            if became_passthrough && !self.slot(slot)?.gesture_rejected {
                self.flush_pending_tap(output);
            }
            for event in held_events {
                self.push_passthrough_event_for_slot(slot, event, output);
            }
            if became_passthrough {
                if let Some(tool_type) = self.slot(slot)?.tool_type {
                    self.push_passthrough_event_for_slot(slot, Event::tool_type(tool_type), output);
                }
            }
            return Ok(());
        };

        let slider_anchor = {
            let slot_state = self.slot(slot)?;
            if slot_state.active
                && matches!(slot_state.ownership, Ownership::Unknown)
                && slot_state.start_x.is_some()
                && slot_state.start_y.is_some()
            {
                self.slider_for_zone(zone)
                    .and_then(|spec| slider_position(self.caps, spec.axis, slot_state))
            } else {
                None
            }
        };

        let rejected = self.slot(slot)?.gesture_rejected;
        let sequence_match = !rejected && self.current_contact_matches_pending_tap(slot, zone)?;
        // Palms neither complete nor interrupt an unrelated finger's pending tap.
        if self.pending_tap.is_some() && !sequence_match && !rejected {
            self.flush_pending_tap(output);
        }

        let slot_state = self.slot_mut(slot)?;
        if slot_state.active
            && matches!(slot_state.ownership, Ownership::Unknown)
            && slot_state.start_x.is_some()
            && slot_state.start_y.is_some()
        {
            slot_state.ownership = Ownership::Claimed(zone);
            slot_state.slider_anchor = slider_anchor;
            slot_state.tap_sequence_match = sequence_match;
            slot_state.held_events.clear();
        }
        Ok(())
    }

    fn promote_claimed_contacts(&mut self) -> Vec<Event> {
        // These contacts' original downs were hidden from the virtual device.
        // Synthesize complete downs at their current positions so the compositor
        // can use them for click-drag and clickfinger.
        let mut events = Vec::new();
        for index in 0..self.slots.len() {
            let slot_number = self.caps.slot_min + index as i32;
            let snapshot = {
                let slot = &mut self.slots[index];
                if !slot.active || !matches!(slot.ownership, Ownership::Claimed(_)) {
                    None
                } else {
                    match (slot.tracking_id, slot.current_x, slot.current_y) {
                        (Some(tracking_id), Some(x), Some(y)) => {
                            slot.ownership = Ownership::Passthrough;
                            slot.slider_anchor = None;
                            Some((tracking_id, x, y, slot.tool_type))
                        }
                        _ => None,
                    }
                }
            };

            if let Some((tracking_id, x, y, tool_type)) = snapshot {
                events.extend([
                    Event::slot(slot_number),
                    Event::tracking_id(tracking_id),
                    Event::x(x),
                    Event::y(y),
                ]);
                if let Some(tool_type) = tool_type {
                    events.push(Event::tool_type(tool_type));
                }
            }
        }
        events
    }

    fn zone_for_current_slot(&self) -> Result<Option<Zone>, SlotError> {
        let slot_state = self.slot(self.current_slot)?;
        if !slot_state.active || slot_state.start_x.is_none() || slot_state.start_y.is_none() {
            return Ok(None);
        }
        let x = self.caps.x.normalize(slot_state.start_x.unwrap());
        let y = self.caps.y.normalize(slot_state.start_y.unwrap());

        // This order resolves corner overlaps and is part of gesture behavior.
        let zone = if x < self.edges.left {
            Some(Zone::Left)
        } else if x > 1.0 - self.edges.right {
            Some(Zone::Right)
        } else if y < self.edges.top {
            Some(Zone::Top)
        } else if y > 1.0 - self.edges.bottom {
            Some(Zone::Bottom)
        } else {
            None
        };
        Ok(zone)
    }
}

fn last_passthrough_slot(events: &[Event]) -> Option<i32> {
    events.iter().rev().find_map(|event| match event {
        Event::Slot(slot) => Some(*slot),
        _ => None,
    })
}

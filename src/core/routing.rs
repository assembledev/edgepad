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
        match slot_state.contact.as_ref().map(|contact| contact.ownership) {
            Some(Ownership::Passthrough) => {
                self.push_passthrough_event_for_current_slot(event, output)
            }
            None | Some(Ownership::Unknown) => slot_state.held_events.push(event),
            Some(Ownership::Claimed(_)) => {}
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
        let Some(contact) = &self.slot(slot)?.contact else {
            return Ok(());
        };
        if contact.ownership != Ownership::Unknown {
            return Ok(());
        }
        let (Some(start_x), Some(start_y)) = (contact.start_x, contact.start_y) else {
            return Ok(());
        };
        let zone = if self.buttonpad && !self.pressed_physical_buttons.is_empty() {
            None
        } else {
            self.zone_for_position(start_x, start_y)
        };
        let rejected = contact.gesture_rejected;
        let Some(zone) = zone else {
            let state = self.slot_mut(slot)?;
            state
                .contact
                .as_mut()
                .expect("contact is being classified")
                .ownership = Ownership::Passthrough;
            let held_events = std::mem::take(&mut state.held_events);
            let tool_type = state.memory.tool_type;
            if !rejected {
                self.flush_pending_tap(output);
            }
            for event in held_events {
                self.push_passthrough_event_for_slot(slot, event, output);
            }
            if let Some(tool_type) = tool_type {
                self.push_passthrough_event_for_slot(slot, Event::tool_type(tool_type), output);
            }
            return Ok(());
        };

        let slider_anchor = self
            .slider_for_zone(zone)
            .and_then(|spec| slider_position(self.caps, spec.axis, contact));
        let sequence_match = !rejected && self.contact_matches_pending_tap(contact, zone);
        // Palms neither complete nor interrupt an unrelated finger's pending tap.
        if self.pending_tap.is_some() && !sequence_match && !rejected {
            self.flush_pending_tap(output);
        }

        let state = self.slot_mut(slot)?;
        let contact = state.contact.as_mut().expect("contact is being classified");
        contact.ownership = Ownership::Claimed(zone);
        contact.slider_anchor = slider_anchor;
        contact.tap_sequence_match = sequence_match;
        state.held_events.clear();
        Ok(())
    }

    fn promote_claimed_contacts(&mut self) -> Vec<Event> {
        // These contacts' original downs were hidden from the virtual device.
        // Synthesize complete downs at their current positions so the compositor
        // can use them for click-drag and clickfinger.
        let mut events = Vec::new();
        for index in 0..self.slots.len() {
            let slot_number = self.caps.slot_min + index as i32;
            let slot = &mut self.slots[index];
            let Some(contact) = &mut slot.contact else {
                continue;
            };
            if !matches!(contact.ownership, Ownership::Claimed(_)) {
                continue;
            }
            let (Some(x), Some(y)) = (contact.current_x, contact.current_y) else {
                continue;
            };
            contact.ownership = Ownership::Passthrough;
            contact.slider_anchor = None;
            let tracking_id = contact.tracking_id;
            let tool_type = slot.memory.tool_type;

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
        events
    }

    fn zone_for_position(&self, start_x: i32, start_y: i32) -> Option<Zone> {
        let x = self.caps.x.normalize(start_x);
        let y = self.caps.y.normalize(start_y);

        // This order resolves corner overlaps and is part of gesture behavior.
        if x < self.edges.left {
            Some(Zone::Left)
        } else if x > 1.0 - self.edges.right {
            Some(Zone::Right)
        } else if y < self.edges.top {
            Some(Zone::Top)
        } else if y > 1.0 - self.edges.bottom {
            Some(Zone::Bottom)
        } else {
            None
        }
    }
}

fn last_passthrough_slot(events: &[Event]) -> Option<i32> {
    events.iter().rev().find_map(|event| match event {
        Event::Slot(slot) => Some(*slot),
        _ => None,
    })
}

//! Type-B slot memory and contact lifetime.

use super::{Contact, Event, ResyncContact, SlotError};

// Linux retains these values after lift and may omit them on the next down.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SlotMemory {
    pub(super) x: Option<i32>,
    pub(super) y: Option<i32>,
    pub(super) tool_type: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SlotState {
    pub(super) memory: SlotMemory,
    pub(super) contact: Option<Contact>,
    // Slot/axis events can precede TRACKING_ID. Keep their order until the
    // contact's starting position determines whether to forward its down.
    pub(super) held_events: Vec<Event>,
}

impl SlotState {
    pub(super) fn begin_contact(&mut self, slot: i32, contact: Contact) -> Result<(), SlotError> {
        if let Some(active) = &self.contact {
            return Err(SlotError::SlotAlreadyActive {
                slot,
                active_tracking_id: active.tracking_id,
                new_tracking_id: contact.tracking_id,
            });
        }
        self.held_events
            .push(Event::tracking_id(contact.tracking_id));
        self.contact = Some(contact);
        Ok(())
    }

    pub(super) fn take_contact(&mut self) -> Option<Contact> {
        let contact = self.contact.take()?;
        self.held_events.clear();
        Some(contact)
    }

    pub(super) fn finalize_new_contact(&mut self) -> bool {
        let Some(contact) = &mut self.contact else {
            return false;
        };
        if !contact.began_in_frame {
            return false;
        }
        // Wait for the whole frame before falling back to retained coordinates:
        // a later axis event may move this contact away from the previous one.
        contact.current_x = contact.current_x.or(self.memory.x);
        contact.current_y = contact.current_y.or(self.memory.y);
        contact.start_x = contact.start_x.or(contact.current_x);
        contact.start_y = contact.start_y.or(contact.current_y);
        contact.began_in_frame = false;
        true
    }

    pub(super) fn restore_contact(&mut self, snapshot: &ResyncContact, tool_type: Option<i32>) {
        self.memory = SlotMemory {
            x: Some(snapshot.x),
            y: Some(snapshot.y),
            tool_type,
        };
        self.contact = Some(Contact::restored(snapshot, tool_type));
        self.held_events.clear();
    }
}

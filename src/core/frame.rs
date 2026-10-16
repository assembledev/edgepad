//! Complete-frame processing and order-independent palm classification.

use std::collections::BTreeMap;
use std::time::Duration;

use super::{Contact, ContactPhase, Engine, Event, FrameOutput, SlotError, MT_TOOL_PALM};

#[derive(Debug, Clone, Copy, Default)]
struct FrameContactTool {
    tool_type: Option<i32>,
    // Preserve an explicit palm report even if a later event says "finger".
    palm_reported: bool,
}

impl FrameContactTool {
    fn is_palm(self) -> bool {
        self.palm_reported || self.tool_type == Some(MT_TOOL_PALM)
    }
}

impl Engine {
    /// Process an untimed frame. Tap-duration checks are skipped and double
    /// taps cannot form; use `process_frame_at` when timestamps are available.
    pub fn process_frame(&mut self, frame: &[Event]) -> Result<FrameOutput, SlotError> {
        self.process_frame_with_time(frame, None)
    }

    /// Process one complete frame using its input timestamp. Use the same
    /// monotonic time domain for all frames and calls to `advance_time`.
    pub fn process_frame_at(
        &mut self,
        frame: &[Event],
        timestamp: Duration,
    ) -> Result<FrameOutput, SlotError> {
        self.process_frame_with_time(frame, Some(timestamp))
    }

    // Resolve each contact's classification before recognition. Coordinates
    // can precede TOOL_TYPE within a SYN frame, and a reused slot can inherit
    // a palm value that a later finger update overrides in its first frame.
    fn frame_contact_tools(
        &self,
        frame: &[Event],
    ) -> Result<BTreeMap<(i32, i32), FrameContactTool>, SlotError> {
        let mut tools = self
            .slots
            .iter()
            .map(|slot| slot.memory.tool_type)
            .collect::<Vec<_>>();
        let mut tracking_ids = self
            .slots
            .iter()
            .map(|slot| slot.contact.as_ref().map(|contact| contact.tracking_id))
            .collect::<Vec<_>>();
        // A slot can be lifted and reused within this frame. Key by both slot
        // and tracking ID so the next contact's tool label cannot reject the old one.
        let mut contacts = BTreeMap::new();
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some(contact) = &slot.contact {
                contacts.insert(
                    (self.caps.slot_min + index as i32, contact.tracking_id),
                    FrameContactTool {
                        tool_type: slot.memory.tool_type,
                        palm_reported: contact.gesture_rejected,
                    },
                );
            }
        }
        let mut current_slot = self.current_slot;
        for event in frame {
            match *event {
                Event::Slot(slot) => {
                    self.ensure_slot(slot)?;
                    current_slot = slot;
                }
                Event::TrackingId(id) => {
                    let index = (current_slot - self.caps.slot_min) as usize;
                    tracking_ids[index] = (id >= 0).then_some(id);
                    if id >= 0 {
                        contacts.insert(
                            (current_slot, id),
                            FrameContactTool {
                                tool_type: tools[index],
                                palm_reported: false,
                            },
                        );
                    }
                }
                Event::ToolType(tool_type) => {
                    let index = (current_slot - self.caps.slot_min) as usize;
                    tools[index] = Some(tool_type);
                    if let Some(id) = tracking_ids[index] {
                        let contact = contacts
                            .get_mut(&(current_slot, id))
                            .expect("active contact has tool state");
                        contact.tool_type = Some(tool_type);
                        contact.palm_reported |= tool_type == MT_TOOL_PALM;
                    }
                }
                Event::SynDropped => {
                    tools.fill(None);
                    tracking_ids.fill(None);
                    contacts.clear();
                    current_slot = self.caps.slot_min;
                }
                Event::X(_) | Event::Y(_) => {}
            }
        }
        Ok(contacts)
    }

    fn process_frame_with_time(
        &mut self,
        frame: &[Event],
        timestamp: Option<Duration>,
    ) -> Result<FrameOutput, SlotError> {
        let frame_tools = if self.options.palm_rejection {
            self.frame_contact_tools(frame)?
        } else {
            BTreeMap::new()
        };
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Some(contact) = &mut slot.contact {
                if frame_tools
                    .get(&(self.caps.slot_min + index as i32, contact.tracking_id))
                    .is_some_and(|tool| tool.is_palm())
                {
                    contact.reject_gestures();
                }
            }
        }
        let mut output = FrameOutput::empty();
        if let Some(timestamp) = timestamp {
            self.expire_pending_tap(timestamp, &mut output);
        }

        for event in frame.iter().copied() {
            match event {
                Event::SynDropped => {
                    self.reset_for_resync();
                    output.resync_required = true;
                }
                Event::Slot(slot) => {
                    self.ensure_slot(slot)?;
                    self.current_slot = slot;
                    self.route_event_for_current_slot(event, &mut output)?;
                }
                Event::TrackingId(tracking_id) if tracking_id >= 0 => {
                    let slot = self.current_slot;
                    let tool = frame_tools
                        .get(&(slot, tracking_id))
                        .copied()
                        .unwrap_or_default();
                    let another_contact_active = self
                        .slots
                        .iter()
                        .filter_map(|slot| slot.contact.as_ref())
                        .any(|contact| !contact.gesture_rejected);
                    let mut contact = Contact::new(
                        tracking_id,
                        timestamp,
                        !another_contact_active && !tool.is_palm(),
                    );
                    if tool.is_palm() {
                        contact.reject_gestures();
                    }
                    let slot_state = self.slot_mut(slot)?;
                    slot_state.begin_contact(slot, contact)?;
                    slot_state.memory.tool_type = tool.tool_type;
                    // Double taps are sequential. Overlapping fingers invalidate
                    // pairing; palms must not interrupt a legitimate finger sequence.
                    if another_contact_active && !tool.is_palm() {
                        for contact in self
                            .slots
                            .iter_mut()
                            .filter_map(|slot| slot.contact.as_mut())
                        {
                            contact.tap_sequence_eligible = false;
                            contact.tap_sequence_match = false;
                        }
                        self.flush_pending_tap(&mut output);
                    }
                }
                Event::TrackingId(_) => {
                    self.release_current_slot(event, timestamp, &mut output)?;
                }
                Event::ToolType(_) if !self.options.palm_rejection => continue,
                Event::ToolType(tool_type) => {
                    self.slot_mut(self.current_slot)?.memory.tool_type = Some(tool_type);
                    self.route_event_for_current_slot(event, &mut output)?;
                }
                Event::X(x) => {
                    let capabilities = self.caps;
                    let swipe_min_distance = self.options.swipe_min_distance;
                    let sequence_invalidated = {
                        let slot_state = self.slot_mut(self.current_slot)?;
                        slot_state.memory.x = Some(x);
                        if let Some(contact) = &mut slot_state.contact {
                            contact.current_x = Some(x);
                            if contact.start_x.is_none() {
                                contact.start_x = Some(x);
                            }
                            contact.observe_position(capabilities, swipe_min_distance);
                            contact.tap_sequence_match
                                && contact.contact_phase != ContactPhase::TapCandidate
                        } else {
                            false
                        }
                    };
                    if sequence_invalidated {
                        self.flush_pending_tap(&mut output);
                    }
                    self.route_event_for_current_slot(event, &mut output)?;
                }
                Event::Y(y) => {
                    let capabilities = self.caps;
                    let swipe_min_distance = self.options.swipe_min_distance;
                    let sequence_invalidated = {
                        let slot_state = self.slot_mut(self.current_slot)?;
                        slot_state.memory.y = Some(y);
                        if let Some(contact) = &mut slot_state.contact {
                            contact.current_y = Some(y);
                            if contact.start_y.is_none() {
                                contact.start_y = Some(y);
                            }
                            contact.observe_position(capabilities, swipe_min_distance);
                            contact.tap_sequence_match
                                && contact.contact_phase != ContactPhase::TapCandidate
                        } else {
                            false
                        }
                    };
                    if sequence_invalidated {
                        self.flush_pending_tap(&mut output);
                    }
                    self.route_event_for_current_slot(event, &mut output)?;
                }
            }

            if !self
                .slot(self.current_slot)?
                .contact
                .as_ref()
                .is_some_and(|contact| contact.began_in_frame)
            {
                self.decide_ownership_if_ready(&mut output)?;
                self.emit_slider_steps_if_ready(&mut output)?;
            }
        }

        self.finalize_contacts_started_in_frame(&mut output)?;

        Ok(output)
    }
}

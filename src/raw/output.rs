use std::collections::BTreeSet;

use crate::core::{Capabilities, SlotError};

use super::events::*;
use super::routing::RoutedRawFrame;

mod pointer;
use pointer::PointerEmulation;

/// Tracks the contacts and buttons visible to the virtual device, independently
/// of physical state. `finish` releases that output state before shutdown.
#[derive(Debug, Clone)]
pub struct RawOutputComposer {
    caps: Capabilities,
    current_slot: i32,
    slots: Vec<RawOutputSlotState>,
    pointer: PointerEmulation,
    pressed_physical_buttons: BTreeSet<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RawOutputSlotState {
    active: bool,
    x: Option<i32>,
    y: Option<i32>,
}

impl RawOutputComposer {
    pub fn new(caps: Capabilities) -> Self {
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
            slots: vec![RawOutputSlotState::default(); slot_count],
            pointer: PointerEmulation::default(),
            pressed_physical_buttons: BTreeSet::new(),
        }
    }

    pub fn compose_frame(&mut self, routed: &RoutedRawFrame) -> Result<RawFrame, SlotError> {
        if routed.resync_required {
            return self.finish();
        }

        let mut events = Vec::new();
        for event in routed.passthrough.iter().copied() {
            self.apply_passthrough_event(event)?;
            events.push(event);
        }
        self.append_legacy_state_events(&mut events);
        // libinput needs the promoted contact's position before the click event
        // to recognize a click-drag that started inside an edge zone.
        for event in routed.physical_buttons.iter().copied() {
            self.apply_physical_button_event(event);
            events.push(event);
        }

        Ok(RawFrame::new(events))
    }

    pub fn finish(&mut self) -> Result<RawFrame, SlotError> {
        let events = self.release_all_tracked_contacts();
        self.reset();
        Ok(RawFrame::new(events))
    }

    fn apply_passthrough_event(&mut self, event: RawEvent) -> Result<(), SlotError> {
        match (event.kind, event.code) {
            (EV_ABS, ABS_MT_SLOT) => {
                self.ensure_slot(event.value)?;
                self.current_slot = event.value;
            }
            (EV_ABS, ABS_MT_TRACKING_ID) if event.value >= 0 => {
                let slot = self.slot_mut(self.current_slot)?;
                slot.active = true;
                slot.x = None;
                slot.y = None;
            }
            (EV_ABS, ABS_MT_TRACKING_ID) => {
                let slot = self.slot_mut(self.current_slot)?;
                slot.active = false;
                slot.x = None;
                slot.y = None;
            }
            (EV_ABS, ABS_MT_POSITION_X) => {
                self.slot_mut(self.current_slot)?.x = Some(event.value);
            }
            (EV_ABS, ABS_MT_POSITION_Y) => {
                self.slot_mut(self.current_slot)?.y = Some(event.value);
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_physical_button_event(&mut self, event: RawEvent) {
        if event.kind != EV_KEY || !is_pointer_button_code(event.code) {
            return;
        }
        if event.value == 0 {
            self.pressed_physical_buttons.remove(&event.code);
        } else {
            self.pressed_physical_buttons.insert(event.code);
        }
    }

    fn append_legacy_state_events(&mut self, events: &mut Vec<RawEvent>) {
        self.pointer.append_changes(
            self.active_slot_count(),
            self.representative_position(),
            events,
        );
    }

    fn active_slot_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.active).count()
    }

    fn representative_position(&self) -> Option<(i32, i32)> {
        self.slots
            .iter()
            .find(|slot| slot.active && slot.x.is_some() && slot.y.is_some())
            .and_then(|slot| Some((slot.x?, slot.y?)))
    }

    fn release_all_tracked_contacts(&mut self) -> Vec<RawEvent> {
        let mut events = Vec::new();
        let mut emitted_current_slot = self.current_slot;
        for index in 0..self.slots.len() {
            if !self.slots[index].active {
                continue;
            }
            let slot = self.caps.slot_min + index as i32;
            if slot != emitted_current_slot {
                events.push(RawEvent::abs_mt_slot(slot));
                emitted_current_slot = slot;
            }
            events.push(RawEvent::abs_mt_tracking_id(-1));
        }
        self.clear_slot_state();
        self.append_legacy_state_events(&mut events);
        for code in std::mem::take(&mut self.pressed_physical_buttons) {
            events.push(RawEvent::new(EV_KEY, code, 0));
        }
        events
    }

    fn reset(&mut self) {
        self.clear_slot_state();
        self.current_slot = self.caps.slot_min;
        self.pointer = PointerEmulation::default();
        self.pressed_physical_buttons.clear();
    }

    fn clear_slot_state(&mut self) {
        for slot in &mut self.slots {
            *slot = RawOutputSlotState::default();
        }
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

    fn slot_index(&self, slot: i32) -> Result<usize, SlotError> {
        self.ensure_slot(slot)?;
        Ok((slot - self.caps.slot_min) as usize)
    }

    fn slot_mut(&mut self, slot: i32) -> Result<&mut RawOutputSlotState, SlotError> {
        let index = self.slot_index(slot)?;
        Ok(&mut self.slots[index])
    }
}

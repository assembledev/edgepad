use super::ProxyInputEvent;
use crate::core::{Capabilities, ResyncContact};
use crate::raw::{
    RawEvent, RawFrame, ABS_MT_SLOT, ABS_MT_TRACKING_ID, BTN_TOUCH, EV_ABS, EV_KEY, EV_SYN,
    SYN_DROPPED, SYN_REPORT,
};
use std::time::Duration;

#[derive(Debug, Default)]
pub(super) struct PendingRawFrame {
    events: Vec<RawEvent>,
    timestamp: Option<Duration>,
}

impl PendingRawFrame {
    fn push(&mut self, event: ProxyInputEvent) {
        self.events.push(event.raw);
        if let Some(timestamp) = event.timestamp {
            self.timestamp = Some(timestamp);
        }
    }

    fn take_frame(&mut self, frame_timestamp: Option<Duration>) -> Option<RawFrame> {
        if self.events.is_empty() {
            self.timestamp = None;
            return None;
        }

        let timestamp = frame_timestamp.or(self.timestamp);
        self.timestamp = None;
        Some(raw_frame_with_timestamp(
            std::mem::take(&mut self.events),
            timestamp,
        ))
    }

    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

pub(super) enum FrameInput {
    Pending,
    Boundary(Option<RawFrame>),
    Dropped(RawFrame),
    Resync(Option<std::time::Duration>),
}

pub(super) enum FrameAssembler {
    Collecting(PendingRawFrame),
    Discarding,
}

impl Default for FrameAssembler {
    fn default() -> Self {
        Self::Collecting(PendingRawFrame::default())
    }
}

impl FrameAssembler {
    pub(super) fn push(&mut self, event: ProxyInputEvent) -> FrameInput {
        let raw = event.raw;
        match self {
            Self::Discarding if (raw.kind, raw.code) == (EV_SYN, SYN_REPORT) => {
                *self = Self::default();
                FrameInput::Resync(event.timestamp)
            }
            Self::Discarding => FrameInput::Pending,
            Self::Collecting(pending) => match (raw.kind, raw.code) {
                (EV_SYN, SYN_DROPPED) => {
                    *self = Self::Discarding;
                    FrameInput::Dropped(raw_frame_with_timestamp(vec![raw], event.timestamp))
                }
                (EV_SYN, SYN_REPORT) => FrameInput::Boundary(pending.take_frame(event.timestamp)),
                _ => {
                    pending.push(event);
                    FrameInput::Pending
                }
            },
        }
    }

    pub(super) fn is_idle(&self) -> bool {
        matches!(self, Self::Collecting(pending) if pending.is_empty())
    }
}

fn raw_frame_with_timestamp(events: Vec<RawEvent>, timestamp: Option<Duration>) -> RawFrame {
    match timestamp {
        Some(timestamp) => RawFrame::new_at(events, timestamp),
        None => RawFrame::new(events),
    }
}

// Track all physical contacts, including edge-owned ones hidden from output.
// Include those fingers in the shutdown drain; virtual output can look idle.
#[derive(Debug, Clone)]
pub(super) struct PhysicalTouchState {
    current_slot: i32,
    active_slots: Vec<bool>,
    btn_touch_down: bool,
    desynchronized: bool,
    capabilities: Capabilities,
}

impl PhysicalTouchState {
    pub(super) fn new(capabilities: Capabilities) -> Self {
        let slot_count = (capabilities.slot_max - capabilities.slot_min + 1) as usize;
        Self {
            current_slot: capabilities.slot_min,
            active_slots: vec![false; slot_count],
            btn_touch_down: false,
            desynchronized: false,
            capabilities,
        }
    }

    pub(super) fn observe_frame(&mut self, frame: &RawFrame) {
        for event in &frame.events {
            match (event.kind, event.code) {
                (EV_ABS, ABS_MT_SLOT) => {
                    if self.slot_index(event.value).is_some() {
                        self.current_slot = event.value;
                    }
                }
                (EV_ABS, ABS_MT_TRACKING_ID) if event.value >= 0 => {
                    if let Some(index) = self.slot_index(self.current_slot) {
                        self.active_slots[index] = true;
                    }
                }
                (EV_ABS, ABS_MT_TRACKING_ID) => {
                    if let Some(index) = self.slot_index(self.current_slot) {
                        self.active_slots[index] = false;
                    }
                }
                (EV_KEY, BTN_TOUCH) => self.btn_touch_down = event.value != 0,
                _ => {}
            }
        }
    }

    pub(super) fn is_touch_down(&self) -> bool {
        self.desynchronized || self.btn_touch_down || self.active_slots.iter().any(|active| *active)
    }

    pub(super) fn mark_desynchronized(&mut self) {
        self.active_slots.fill(false);
        self.btn_touch_down = false;
        self.desynchronized = true;
    }

    pub(super) fn restore_contacts(&mut self, contacts: &[ResyncContact]) {
        self.current_slot = self.capabilities.slot_min;
        self.active_slots.fill(false);
        self.btn_touch_down = !contacts.is_empty();
        self.desynchronized = false;
        for contact in contacts {
            if let Some(index) = self.slot_index(contact.slot) {
                self.active_slots[index] = true;
                self.current_slot = contact.slot;
            }
        }
    }

    fn slot_index(&self, slot: i32) -> Option<usize> {
        (slot >= self.capabilities.slot_min && slot <= self.capabilities.slot_max)
            .then_some((slot - self.capabilities.slot_min) as usize)
    }
}

#[cfg(test)]
mod tests;

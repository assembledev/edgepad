//! Single/double-tap arbitration and input-time deadlines.

use std::time::Duration;

use super::contact::ClassifiedContact;
use super::{Engine, FrameOutput, Gesture, GestureDirection, SlotError, Zone};

#[derive(Debug, Clone, Copy)]
pub(super) struct PendingTap {
    gesture: Gesture,
    released_at: Duration,
    x: i32,
    y: i32,
    emit_single: bool,
}

impl Engine {
    pub fn next_deadline(&self) -> Option<Duration> {
        let pending = self.pending_tap?;
        if let Some(second_tap_deadline) = self.matching_contact_deadline() {
            return Some(second_tap_deadline);
        }
        Some(
            pending
                .released_at
                .saturating_add(self.options.double_tap_timeout),
        )
    }

    pub fn advance_time(&mut self, timestamp: Duration) -> FrameOutput {
        let mut output = FrameOutput::empty();
        self.expire_pending_tap(timestamp, &mut output);
        output
    }

    /// Whether tap arbitration is finished, not whether all fingers are up.
    /// The proxy separately checks physical contacts before reload or shutdown.
    pub fn is_recognition_idle(&self) -> bool {
        self.pending_tap.is_none()
    }

    pub fn interrupt_tap_sequence(&mut self) -> Vec<Gesture> {
        // A physical interaction can arrive after a matching second
        // contact is already down. Keep separate-button edge ownership
        // intact, but do not let that contact seed another temporal pair.
        for slot in self.slots.iter_mut().filter(|slot| slot.active) {
            slot.tap_sequence_eligible = false;
            slot.tap_sequence_match = false;
        }
        let mut output = FrameOutput::empty();
        self.flush_pending_tap(&mut output);
        output.gestures
    }

    pub(super) fn handle_classified_gesture(
        &mut self,
        classified: ClassifiedContact,
        released_at: Option<Duration>,
        output: &mut FrameOutput,
    ) {
        let ClassifiedContact {
            gesture,
            sequence_match,
            sequence_eligible,
            x,
            y,
        } = classified;
        if gesture.direction != GestureDirection::Tap {
            self.flush_pending_tap(output);
            output.gestures.push(gesture);
            return;
        }

        if sequence_match && self.pending_tap.take().is_some() {
            output.gestures.push(Gesture {
                direction: GestureDirection::DoubleTap,
                ..gesture
            });
            return;
        }

        if !sequence_eligible {
            self.flush_pending_tap(output);
            if self.options.single_tap_zones.contains(gesture.zone) {
                output.gestures.push(gesture);
            }
            return;
        }

        let can_start_sequence = self.options.double_tap_zones.contains(gesture.zone)
            && released_at.is_some()
            && x.is_some()
            && y.is_some();
        if !can_start_sequence {
            output.gestures.push(gesture);
            return;
        }

        self.flush_pending_tap(output);
        self.pending_tap = Some(PendingTap {
            gesture,
            released_at: released_at.unwrap(),
            x: x.unwrap(),
            y: y.unwrap(),
            emit_single: self.options.single_tap_zones.contains(gesture.zone),
        });
    }

    pub(super) fn current_contact_matches_pending_tap(
        &self,
        slot: i32,
        zone: Zone,
    ) -> Result<bool, SlotError> {
        let Some(pending) = self.pending_tap else {
            return Ok(false);
        };
        let state = self.slot(slot)?;
        if !state.tap_sequence_eligible || pending.gesture.zone != zone {
            return Ok(false);
        }
        let (Some(started_at), Some(x), Some(y)) = (state.started_at, state.start_x, state.start_y)
        else {
            return Ok(false);
        };
        let Some(gap) = started_at.checked_sub(pending.released_at) else {
            return Ok(false);
        };
        if gap >= self.options.double_tap_timeout {
            return Ok(false);
        }

        let dx = self.caps.x.normalize_delta(pending.x, x);
        let dy = self.caps.y.normalize_delta(pending.y, y);
        let max_distance = self.options.double_tap_max_distance;
        Ok(dx * dx + dy * dy <= max_distance * max_distance)
    }

    // The gap limit controls when the second tap starts. Once it is down,
    // allow its own tap-duration window before releasing the first tap.
    fn matching_contact_deadline(&self) -> Option<Duration> {
        self.slots
            .iter()
            .find(|slot| slot.active && slot.tap_sequence_match)
            .and_then(|slot| slot.started_at)
            .map(|started_at| started_at.saturating_add(self.options.tap_max_duration))
    }

    pub(super) fn expire_pending_tap(&mut self, timestamp: Duration, output: &mut FrameOutput) {
        let Some(pending) = self.pending_tap else {
            return;
        };

        let deadline = if let Some(second_tap_deadline) = self.matching_contact_deadline() {
            second_tap_deadline
        } else {
            pending
                .released_at
                .saturating_add(self.options.double_tap_timeout)
        };
        if timestamp >= deadline {
            self.flush_pending_tap(output);
        }
    }

    pub(super) fn flush_pending_tap(&mut self, output: &mut FrameOutput) {
        for slot in &mut self.slots {
            slot.tap_sequence_match = false;
        }
        if let Some(pending) = self.pending_tap.take() {
            if pending.emit_single {
                output.gestures.push(pending.gesture);
            }
        }
    }
}

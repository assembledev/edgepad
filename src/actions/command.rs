use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::core::{Gesture, GestureDirection, SliderDirection, SliderStep, Zone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionCommandStatus {
    pub success: bool,
}

impl ActionCommandStatus {
    pub fn success() -> Self {
        Self { success: true }
    }

    pub fn failure() -> Self {
        Self { success: false }
    }
}

pub trait ActionCommandRunner: Send + 'static {
    /// Check `shutdown` while waiting. Cleanup has a bounded wait and can
    /// leave a runner detached if it ignores cancellation.
    fn run(
        &mut self,
        argv: &[String],
        shutdown: &ActionShutdownToken,
    ) -> Result<ActionCommandStatus, String>;
}

#[derive(Debug, Clone, Default)]
pub struct ActionShutdownToken {
    requested: Arc<AtomicBool>,
}

impl ActionShutdownToken {
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub(super) fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }
}

#[derive(Debug)]
pub(super) struct ActionCommand {
    pub source: ActionSource,
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ActionSource {
    Gesture(Gesture),
    SliderStep(SliderStep),
}

impl std::fmt::Display for ActionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (zone, direction, slot, tracking_id) = match *self {
            Self::Gesture(gesture) => (
                gesture.zone,
                direction_name(gesture.direction),
                gesture.slot,
                gesture.tracking_id,
            ),
            Self::SliderStep(step) => (
                step.zone,
                slider_direction_name(step.direction),
                step.slot,
                step.tracking_id,
            ),
        };
        write!(
            f,
            "zone={} direction={direction} slot={slot} tracking_id={tracking_id}",
            zone_name(zone)
        )
    }
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

fn direction_name(direction: GestureDirection) -> &'static str {
    match direction {
        GestureDirection::Up => "up",
        GestureDirection::Down => "down",
        GestureDirection::Left => "left",
        GestureDirection::Right => "right",
        GestureDirection::Tap => "tap",
        GestureDirection::DoubleTap => "double-tap",
    }
}

fn slider_direction_name(direction: SliderDirection) -> &'static str {
    match direction {
        SliderDirection::Up => "up",
        SliderDirection::Down => "down",
        SliderDirection::Left => "left",
        SliderDirection::Right => "right",
    }
}

#[cfg(test)]
mod tests;

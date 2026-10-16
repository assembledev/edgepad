//! Bindings store action argv; the actions module owns execution and processes.
//! Slider directions map onto negative/positive actions along the zone's axis.

use crate::core::{Gesture, GestureDirection, SliderAxis, SliderDirection, Zone};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GestureBindingConfig {
    pub zone: Zone,
    pub direction: GestureDirection,
    pub action: GestureActionConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SliderBindingConfig {
    pub zone: Zone,
    pub axis: SliderAxis,
    pub step: f32,
    pub negative: CommandActionConfig,
    pub positive: CommandActionConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandActionConfig {
    pub argv: Vec<String>,
}

impl CommandActionConfig {
    pub fn new<I, S>(argv: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let argv = argv.into_iter().map(Into::into).collect::<Vec<_>>();
        if argv.is_empty() {
            return Err("command action requires at least a program".to_string());
        }
        Ok(Self { argv })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GestureActionConfig {
    Log,
    Command { argv: Vec<String> },
}

impl SliderBindingConfig {
    pub fn action_for(&self, direction: SliderDirection) -> &CommandActionConfig {
        match direction {
            SliderDirection::Up | SliderDirection::Left => &self.negative,
            SliderDirection::Down | SliderDirection::Right => &self.positive,
        }
    }

    pub fn direction_is_valid(&self, direction: SliderDirection) -> bool {
        match self.axis {
            SliderAxis::Vertical => {
                matches!(direction, SliderDirection::Up | SliderDirection::Down)
            }
            SliderAxis::Horizontal => {
                matches!(direction, SliderDirection::Left | SliderDirection::Right)
            }
        }
    }
}

impl GestureBindingConfig {
    pub fn matches(&self, gesture: &Gesture) -> bool {
        self.zone == gesture.zone && self.direction == gesture.direction
    }
}

impl GestureActionConfig {
    pub fn command<I, S>(argv: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let argv = argv.into_iter().map(Into::into).collect::<Vec<_>>();
        if argv.is_empty() {
            return Err("command action requires at least a program".to_string());
        }
        Ok(Self::Command { argv })
    }
}

#[cfg(test)]
mod tests;

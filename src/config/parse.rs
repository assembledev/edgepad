//! Lower TOML into validated configuration without accessing input devices.
//! Independently valid bindings can still compete for the same movement, so
//! duplicate entries and slider/gesture conflicts are checked together here.

use std::collections::BTreeSet;

use crate::core::{GestureDirection, SliderAxis, Zone};

use super::validation::{
    validate_double_tap_max_distance, validate_double_tap_timeout_ms, validate_edge_width,
    validate_optional_edge_width, validate_slider_step, validate_swipe_min_distance,
    validate_tap_max_duration_ms, validate_tap_min_duration_ms,
};
use super::{
    CommandActionConfig, DeviceConfig, EdgeWidthOverrides, EdgepadConfig, GestureActionConfig,
    GestureBindingConfig, SliderBindingConfig, DEFAULT_SLIDER_STEP,
};

mod schema;
use schema::{
    RawEdgepadConfig, RawGestureActionConfig, RawGestureBindingConfig, RawSliderBindingConfig,
};

impl EdgepadConfig {
    pub fn parse(input: &str) -> Result<Self, String> {
        let raw = toml::from_str::<RawEdgepadConfig>(input)
            .map_err(|err| format!("invalid TOML config: {err}"))?;
        let mut config = Self::default();
        let mut gesture_keys = BTreeSet::new();
        let mut slider_zones = BTreeSet::new();

        if let Some(device) = raw.device {
            config.device = DeviceConfig::parse(&device)
                .map_err(|err| format!("invalid device config: {err}"))?;
        }
        if let Some(palm_rejection) = raw.palm_rejection {
            config.palm_rejection = palm_rejection;
        }
        if let Some(edge_width) = raw.edge_width {
            config.edge_width = validate_edge_width(edge_width, "edge_width")?;
        }
        config.edge_width_overrides = EdgeWidthOverrides {
            left: validate_optional_edge_width(raw.left_edge_width, "left_edge_width")?,
            right: validate_optional_edge_width(raw.right_edge_width, "right_edge_width")?,
            top: validate_optional_edge_width(raw.top_edge_width, "top_edge_width")?,
            bottom: validate_optional_edge_width(raw.bottom_edge_width, "bottom_edge_width")?,
        };
        if let Some(tap_min_duration_ms) = raw.tap_min_duration_ms {
            config.tap_min_duration_ms =
                validate_tap_min_duration_ms(tap_min_duration_ms, "tap_min_duration_ms")?;
        }
        if let Some(tap_max_duration_ms) = raw.tap_max_duration_ms {
            config.tap_max_duration_ms =
                validate_tap_max_duration_ms(tap_max_duration_ms, "tap_max_duration_ms")?;
        }
        if config.tap_min_duration_ms >= config.tap_max_duration_ms {
            return Err("tap_min_duration_ms must be < tap_max_duration_ms".to_string());
        }
        if let Some(double_tap_timeout_ms) = raw.double_tap_timeout_ms {
            config.double_tap_timeout_ms =
                validate_double_tap_timeout_ms(double_tap_timeout_ms, "double_tap_timeout_ms")?;
        }
        if let Some(double_tap_max_distance) = raw.double_tap_max_distance {
            config.double_tap_max_distance = validate_double_tap_max_distance(
                double_tap_max_distance,
                "double_tap_max_distance",
            )?;
        }
        if let Some(swipe_min_distance) = raw.swipe_min_distance {
            config.swipe_min_distance =
                validate_swipe_min_distance(swipe_min_distance, "swipe_min_distance")?;
        }

        for (index, raw_binding) in raw.gestures.into_iter().enumerate() {
            let binding = GestureBindingConfig::from_raw(index, raw_binding)?;
            let gesture_key = (binding.zone, binding.direction);
            if !gesture_keys.insert(gesture_key) {
                return Err(format!(
                    "duplicate gesture binding {}.{}",
                    zone_name(binding.zone),
                    direction_name(binding.direction)
                ));
            }
            config.gestures.push(binding);
        }

        for (index, raw_slider) in raw.sliders.into_iter().enumerate() {
            let slider = SliderBindingConfig::from_raw(index, raw_slider)?;
            if !slider_zones.insert(slider.zone) {
                return Err(format!(
                    "duplicate slider binding {}",
                    zone_name(slider.zone)
                ));
            }
            config.sliders.push(slider);
        }

        config.validate_slider_gesture_conflicts()?;

        Ok(config)
    }

    fn validate_slider_gesture_conflicts(&self) -> Result<(), String> {
        for (index, gesture) in self.gestures.iter().enumerate() {
            if matches!(
                gesture.direction,
                GestureDirection::Tap | GestureDirection::DoubleTap
            ) {
                continue;
            }
            if self
                .sliders
                .iter()
                .any(|slider| slider.zone == gesture.zone)
            {
                return Err(format!(
                    "{}.direction={} conflicts with slider {}; slider zones only allow tap or double-tap gestures",
                    gesture_label(index),
                    direction_name(gesture.direction),
                    zone_name(gesture.zone)
                ));
            }
        }
        Ok(())
    }
}

impl SliderBindingConfig {
    fn from_raw(index: usize, raw: RawSliderBindingConfig) -> Result<Self, String> {
        let label = slider_label(index);
        let zone = parse_zone(&raw.zone)
            .ok_or_else(|| format!("{}.zone must be one of: left, right, top, bottom", label))?;
        let axis = default_slider_axis(zone);
        let step = match raw.step {
            Some(step) => validate_slider_step(step, &format!("{label}.step"))?,
            None => DEFAULT_SLIDER_STEP,
        };
        let (negative, positive) = match axis {
            SliderAxis::Vertical => {
                reject_slider_action(&label, "left", raw.left)?;
                reject_slider_action(&label, "right", raw.right)?;
                (
                    parse_required_slider_action(&label, "up", raw.up)?,
                    parse_required_slider_action(&label, "down", raw.down)?,
                )
            }
            SliderAxis::Horizontal => {
                reject_slider_action(&label, "up", raw.up)?;
                reject_slider_action(&label, "down", raw.down)?;
                (
                    parse_required_slider_action(&label, "left", raw.left)?,
                    parse_required_slider_action(&label, "right", raw.right)?,
                )
            }
        };

        Ok(Self {
            zone,
            axis,
            step,
            negative,
            positive,
        })
    }
}

impl GestureBindingConfig {
    fn from_raw(index: usize, raw: RawGestureBindingConfig) -> Result<Self, String> {
        let label = gesture_label(index);
        let zone = parse_zone(&raw.zone)
            .ok_or_else(|| format!("{}.zone must be one of: left, right, top, bottom", label))?;
        let direction = parse_direction(&raw.direction).ok_or_else(|| {
            format!(
                "{}.direction must be one of: up, down, left, right, tap, double-tap",
                label
            )
        })?;
        let action = GestureActionConfig::from_raw(index, raw.action)?;

        Ok(Self {
            zone,
            direction,
            action,
        })
    }
}

impl GestureActionConfig {
    fn from_raw(index: usize, raw: RawGestureActionConfig) -> Result<Self, String> {
        match raw {
            RawGestureActionConfig::Command(argv) => {
                Self::command(argv).map_err(|err| format!("{}.action: {err}", gesture_label(index)))
            }
            RawGestureActionConfig::Log { log: true } => Ok(Self::Log),
            RawGestureActionConfig::Log { log: false } => {
                Err(format!("{}.action.log must be true", gesture_label(index)))
            }
        }
    }
}

fn parse_zone(raw: &str) -> Option<Zone> {
    match raw {
        "left" => Some(Zone::Left),
        "right" => Some(Zone::Right),
        "top" => Some(Zone::Top),
        "bottom" => Some(Zone::Bottom),
        _ => None,
    }
}

fn parse_direction(raw: &str) -> Option<GestureDirection> {
    match raw {
        "up" => Some(GestureDirection::Up),
        "down" => Some(GestureDirection::Down),
        "left" => Some(GestureDirection::Left),
        "right" => Some(GestureDirection::Right),
        "tap" => Some(GestureDirection::Tap),
        "double-tap" => Some(GestureDirection::DoubleTap),
        _ => None,
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

fn default_slider_axis(zone: Zone) -> SliderAxis {
    match zone {
        Zone::Left | Zone::Right => SliderAxis::Vertical,
        Zone::Top | Zone::Bottom => SliderAxis::Horizontal,
    }
}

fn parse_required_slider_action(
    label: &str,
    direction: &str,
    raw: Option<Vec<String>>,
) -> Result<CommandActionConfig, String> {
    let argv = raw.ok_or_else(|| format!("{label}.{direction} is required"))?;
    CommandActionConfig::new(argv).map_err(|err| format!("{label}.{direction}: {err}"))
}

fn reject_slider_action(
    label: &str,
    direction: &str,
    raw: Option<Vec<String>>,
) -> Result<(), String> {
    if raw.is_some() {
        return Err(format!(
            "{label}.{direction} is not valid for this slider zone"
        ));
    }
    Ok(())
}

fn gesture_label(index: usize) -> String {
    format!("gestures[{index}]")
}

fn slider_label(index: usize) -> String {
    format!("sliders[{index}]")
}

#[cfg(test)]
mod tests;

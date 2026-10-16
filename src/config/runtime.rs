//! Translate configured values and bindings into recognizer settings.
//! An unbound edge stays available for pointer input even if its width is set.

use std::time::Duration;

use crate::core::{EdgeWidths, EngineOptions, GestureDirection, SliderSpec, Zone, ZoneSet};

use super::{EdgeWidthOverrides, EdgepadConfig};

impl EdgepadConfig {
    pub fn active_edge_widths(&self) -> EdgeWidths {
        let configured = self.configured_edge_widths();
        EdgeWidths {
            left: self.active_zone_edge_width(Zone::Left, configured.left),
            right: self.active_zone_edge_width(Zone::Right, configured.right),
            top: self.active_zone_edge_width(Zone::Top, configured.top),
            bottom: self.active_zone_edge_width(Zone::Bottom, configured.bottom),
        }
    }

    pub fn configured_edge_widths(&self) -> EdgeWidths {
        EdgeWidths {
            left: self.edge_width_overrides.left.unwrap_or(self.edge_width),
            right: self.edge_width_overrides.right.unwrap_or(self.edge_width),
            top: self.edge_width_overrides.top.unwrap_or(self.edge_width),
            bottom: self.edge_width_overrides.bottom.unwrap_or(self.edge_width),
        }
    }

    pub fn override_edge_width(&mut self, width: f32) {
        // A global width override must win over per-edge TOML values. Clear those
        // overrides so they cannot mask the requested width.
        self.edge_width = width;
        self.edge_width_overrides = EdgeWidthOverrides::default();
    }

    fn active_zone_edge_width(&self, zone: Zone, configured_width: f32) -> f32 {
        if self.gestures.iter().any(|binding| binding.zone == zone)
            || self.sliders.iter().any(|binding| binding.zone == zone)
        {
            configured_width
        } else {
            0.0
        }
    }

    pub fn slider_specs(&self) -> Vec<SliderSpec> {
        self.sliders
            .iter()
            .map(|slider| SliderSpec {
                zone: slider.zone,
                axis: slider.axis,
                step: slider.step,
            })
            .collect()
    }

    pub fn engine_options(&self) -> EngineOptions {
        EngineOptions {
            palm_rejection: self.palm_rejection,
            tap_min_duration: Duration::from_millis(self.tap_min_duration_ms),
            tap_max_duration: Duration::from_millis(self.tap_max_duration_ms),
            swipe_min_distance: self.swipe_min_distance,
            double_tap_timeout: Duration::from_millis(self.double_tap_timeout_ms),
            double_tap_max_distance: self.double_tap_max_distance,
            // Defer single taps only where a double-tap binding needs arbitration.
            double_tap_zones: ZoneSet::from_zones(self.gestures.iter().filter_map(|binding| {
                (binding.direction == GestureDirection::DoubleTap).then_some(binding.zone)
            })),
            single_tap_zones: ZoneSet::from_zones(self.gestures.iter().filter_map(|binding| {
                (binding.direction == GestureDirection::Tap).then_some(binding.zone)
            })),
        }
    }
}

#[cfg(test)]
mod tests;

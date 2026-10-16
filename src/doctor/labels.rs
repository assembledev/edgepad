use crate::config::DeviceConfig;
use crate::core::{GestureDirection, SliderDirection, Zone};

pub(super) fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

pub(super) fn direction_name(direction: GestureDirection) -> &'static str {
    match direction {
        GestureDirection::Up => "up",
        GestureDirection::Down => "down",
        GestureDirection::Left => "left",
        GestureDirection::Right => "right",
        GestureDirection::Tap => "tap",
        GestureDirection::DoubleTap => "double-tap",
    }
}

pub(super) fn slider_direction_name(direction: SliderDirection) -> &'static str {
    match direction {
        SliderDirection::Up => "up",
        SliderDirection::Down => "down",
        SliderDirection::Left => "left",
        SliderDirection::Right => "right",
    }
}

pub(super) fn device_config_value_label(device: &DeviceConfig) -> String {
    match device {
        DeviceConfig::Auto => "auto".to_string(),
        DeviceConfig::Path(path) => format!("{}", path.display()),
    }
}

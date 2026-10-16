//! Parse and validate user configuration before it reaches the input loop.
//!
//! Only zones with bindings become active edges. Validation also rejects a
//! slider and directional gesture on the same edge, since both would consume
//! the same movement. Live device selection is separate from TOML parsing.
//!
//! parse.rs lowers the TOML schema into the public model below; runtime.rs turns
//! that model into recognizer settings. Device discovery belongs to device.rs.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

mod bindings;
mod device;
mod parse;
mod runtime;
mod validation;

pub use bindings::{
    CommandActionConfig, GestureActionConfig, GestureBindingConfig, SliderBindingConfig,
};
pub use device::{DeviceConfig, DeviceResolveError};
pub use validation::parse_edge_width;

pub const DEFAULT_EDGE_WIDTH: f32 = 0.10;
pub const DEFAULT_SLIDER_STEP: f32 = 0.04;
pub use crate::core::{
    DEFAULT_DOUBLE_TAP_MAX_DISTANCE, DEFAULT_DOUBLE_TAP_TIMEOUT_MS, DEFAULT_PALM_REJECTION,
    DEFAULT_SWIPE_MIN_DISTANCE, DEFAULT_TAP_MAX_DURATION_MS, DEFAULT_TAP_MIN_DURATION_MS,
};

#[derive(Debug, Clone, PartialEq)]
pub struct EdgepadConfig {
    pub device: DeviceConfig,
    pub edge_width: f32,
    pub palm_rejection: bool,
    pub edge_width_overrides: EdgeWidthOverrides,
    pub tap_min_duration_ms: u64,
    pub tap_max_duration_ms: u64,
    pub double_tap_timeout_ms: u64,
    pub double_tap_max_distance: f32,
    pub swipe_min_distance: f32,
    pub gestures: Vec<GestureBindingConfig>,
    pub sliders: Vec<SliderBindingConfig>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EdgeWidthOverrides {
    pub left: Option<f32>,
    pub right: Option<f32>,
    pub top: Option<f32>,
    pub bottom: Option<f32>,
}

impl Default for EdgepadConfig {
    fn default() -> Self {
        Self {
            device: DeviceConfig::Auto,
            edge_width: DEFAULT_EDGE_WIDTH,
            palm_rejection: DEFAULT_PALM_REJECTION,
            edge_width_overrides: EdgeWidthOverrides::default(),
            tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
            tap_max_duration_ms: DEFAULT_TAP_MAX_DURATION_MS,
            double_tap_timeout_ms: DEFAULT_DOUBLE_TAP_TIMEOUT_MS,
            double_tap_max_distance: DEFAULT_DOUBLE_TAP_MAX_DISTANCE,
            swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
            gestures: Vec::new(),
            sliders: Vec::new(),
        }
    }
}

pub fn load_edgepad_config(path: &Path) -> Result<EdgepadConfig, String> {
    let input = fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    EdgepadConfig::parse(&input)
        .map_err(|err| format!("failed to parse config {}: {err}", path.display()))
}

pub fn default_edgepad_config_path() -> Result<PathBuf, String> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(config_home).join("edgepad/edgepad.toml"));
    }

    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home).join(".config/edgepad/edgepad.toml"));
    }

    Err(
        "config path was not provided; pass --config <file> or set XDG_CONFIG_HOME/HOME"
            .to_string(),
    )
}

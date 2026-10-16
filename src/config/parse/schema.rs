//! Optional settings distinguish omitted values from explicit values. Parsing
//! supplies defaults and checks cross-binding rules; serde rejects unknown keys.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawEdgepadConfig {
    pub(super) device: Option<String>,
    pub(super) palm_rejection: Option<bool>,
    pub(super) edge_width: Option<f32>,
    pub(super) left_edge_width: Option<f32>,
    pub(super) right_edge_width: Option<f32>,
    pub(super) top_edge_width: Option<f32>,
    pub(super) bottom_edge_width: Option<f32>,
    pub(super) tap_min_duration_ms: Option<u64>,
    pub(super) tap_max_duration_ms: Option<u64>,
    pub(super) double_tap_timeout_ms: Option<u64>,
    pub(super) double_tap_max_distance: Option<f32>,
    pub(super) swipe_min_distance: Option<f32>,
    #[serde(default)]
    pub(super) gestures: Vec<RawGestureBindingConfig>,
    #[serde(default)]
    pub(super) sliders: Vec<RawSliderBindingConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawGestureBindingConfig {
    pub(super) zone: String,
    pub(super) direction: String,
    pub(super) action: RawGestureActionConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum RawGestureActionConfig {
    Command(Vec<String>),
    Log { log: bool },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSliderBindingConfig {
    pub(super) zone: String,
    pub(super) step: Option<f32>,
    pub(super) up: Option<Vec<String>>,
    pub(super) down: Option<Vec<String>>,
    pub(super) left: Option<Vec<String>>,
    pub(super) right: Option<Vec<String>>,
}

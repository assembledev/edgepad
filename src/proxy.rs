//! Live touchpad proxy configuration, callbacks, and runtime statistics.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use crate::core::{
    Capabilities, EdgeWidths, EngineOptions, Gesture, GestureDirection, SliderDirection,
    SliderSpec, SliderStep, Zone,
};

mod error;
mod live;
mod session;
#[cfg(test)]
mod test_support;

pub use crate::config::DEFAULT_EDGE_WIDTH;
pub use error::ProxyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    DryRun,
    UinputGrab,
}

#[derive(Debug, Clone)]
pub struct ProxyRunConfig {
    pub device_path: PathBuf,
    pub limit: ProxyRunLimit,
    pub edge_widths: EdgeWidths,
    pub engine_options: EngineOptions,
    pub slider_specs: Vec<SliderSpec>,
    pub mode: ProxyMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyRunSummary {
    pub mode: ProxyMode,
    pub device_path: PathBuf,
    pub capabilities: Capabilities,
    pub edge_widths: EdgeWidths,
    pub requested_frame_boundaries: Option<usize>,
    pub stats: ProxyRuntimeStats,
}

#[derive(Debug, Clone)]
pub enum ProxyRunLimit {
    Frames {
        frame_boundaries: usize,
        stop_after_limit: StopAfterFrameLimit,
    },
    UntilStopped {
        stop: StopToken,
        idle_drain_timeout: Duration,
    },
}

impl ProxyRunLimit {
    fn requested_frame_boundaries(&self) -> Option<usize> {
        match self {
            Self::Frames {
                frame_boundaries, ..
            } => Some(*frame_boundaries),
            Self::UntilStopped { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAfterFrameLimit {
    Immediately,
    WhenIdle,
}

#[derive(Debug, Clone, Default)]
pub struct StopToken {
    stopped: Arc<AtomicBool>,
}

impl StopToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EventHistory {
    #[default]
    Record,
    CountsOnly,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyRuntimeStats {
    pub input_frame_boundaries: usize,
    pub raw_frames: usize,
    pub raw_events: usize,
    pub recognizer_events: usize,
    pub recognizer_passthrough_events: usize,
    pub passthrough_frames: usize,
    pub claimed_edge_frames: usize,
    pub empty_output_frames: usize,
    pub composed_frames: usize,
    pub composed_events: usize,
    pub cleanup_output_frames: usize,
    pub cleanup_output_events: usize,
    pub settle_output_frames: usize,
    pub settle_output_events: usize,
    pub idle_drain_frame_boundaries: usize,
    pub idle_drain_timed_out: bool,
    pub recognition_reloads: usize,
    /// Ordered history for frame-limited diagnostics; empty for daemon runs.
    pub gestures: Vec<Gesture>,
    pub gesture_counts: BTreeMap<GestureCountKey, usize>,
    /// Ordered history for frame-limited diagnostics; empty for daemon runs.
    pub slider_steps: Vec<SliderStep>,
    pub slider_step_counts: BTreeMap<SliderStepCountKey, usize>,
    pub resync_required: bool,
    event_history: EventHistory,
}

impl ProxyRuntimeStats {
    fn for_run(limit: &ProxyRunLimit) -> Self {
        Self {
            event_history: match limit {
                ProxyRunLimit::Frames { .. } => EventHistory::Record,
                ProxyRunLimit::UntilStopped { .. } => EventHistory::CountsOnly,
            },
            ..Self::default()
        }
    }

    /// Total recognized gestures, including those omitted from event history.
    pub fn gesture_count(&self) -> usize {
        self.gesture_counts.values().sum()
    }

    /// Total recognized slider steps, including those omitted from event history.
    pub fn slider_step_count(&self) -> usize {
        self.slider_step_counts.values().sum()
    }

    fn record_gesture(&mut self, gesture: Gesture) {
        *self
            .gesture_counts
            .entry(gesture_count_key(gesture))
            .or_default() += 1;
        if self.event_history == EventHistory::Record {
            self.gestures.push(gesture);
        }
    }

    fn record_slider_step(&mut self, step: SliderStep) {
        *self
            .slider_step_counts
            .entry(slider_step_count_key(step))
            .or_default() += 1;
        if self.event_history == EventHistory::Record {
            self.slider_steps.push(step);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GestureCountKey {
    pub zone: Zone,
    pub direction: GestureDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SliderStepCountKey {
    pub zone: Zone,
    pub direction: SliderDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyRecognitionConfig {
    pub edge_widths: EdgeWidths,
    pub engine_options: EngineOptions,
    pub slider_specs: Vec<SliderSpec>,
}

pub trait GestureHandler {
    fn handle_gesture(&mut self, gesture: Gesture);

    fn handle_slider_step(&mut self, _step: SliderStep) {}

    fn take_recognition_config_reload(&mut self) -> Option<ProxyRecognitionConfig> {
        None
    }
}

#[derive(Debug, Default)]
pub struct NoopGestureHandler;

impl GestureHandler for NoopGestureHandler {
    fn handle_gesture(&mut self, _gesture: Gesture) {}
}

pub fn run_proxy(config: &ProxyRunConfig) -> Result<ProxyRunSummary, ProxyError> {
    let mut handler = NoopGestureHandler;
    run_proxy_with_gesture_handler(config, &mut handler)
}

pub fn run_proxy_with_gesture_handler<H>(
    config: &ProxyRunConfig,
    handler: &mut H,
) -> Result<ProxyRunSummary, ProxyError>
where
    H: GestureHandler,
{
    let mut on_ready = |_device_path: &std::path::Path| Ok(());
    run_proxy_with_gesture_handler_and_ready(config, handler, &mut on_ready)
}

pub fn run_proxy_with_gesture_handler_and_ready<H, F>(
    config: &ProxyRunConfig,
    handler: &mut H,
    on_ready: &mut F,
) -> Result<ProxyRunSummary, ProxyError>
where
    H: GestureHandler,
    F: FnMut(&std::path::Path) -> Result<(), String>,
{
    validate_run_limit(&config.limit)?;
    match config.mode {
        ProxyMode::DryRun => live::proxy_dry_run(config, handler),
        ProxyMode::UinputGrab => live::proxy_uinput_grab(config, handler, on_ready),
    }
}

fn validate_run_limit(limit: &ProxyRunLimit) -> Result<(), ProxyError> {
    match limit {
        ProxyRunLimit::Frames {
            frame_boundaries: 0,
            ..
        } => Err(ProxyError::InvalidFrameLimit),
        _ => Ok(()),
    }
}

fn gesture_count_key(gesture: Gesture) -> GestureCountKey {
    GestureCountKey {
        zone: gesture.zone,
        direction: gesture.direction,
    }
}

fn slider_step_count_key(step: SliderStep) -> SliderStepCountKey {
    SliderStepCountKey {
        zone: step.zone,
        direction: step.direction,
    }
}

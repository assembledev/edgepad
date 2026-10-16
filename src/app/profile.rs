//! Recognition profiles shared by replay and live commands.
//! CLI overrides are reapplied to daemon reload candidates before validation;
//! width overrides preserve which edges have bindings in a configured profile.

use std::fs;
use std::path::{Path, PathBuf};

use edgepad::config::{
    default_edgepad_config_path, load_edgepad_config, DeviceConfig, EdgepadConfig,
};
use edgepad::core::{EdgeWidths, EngineOptions, SliderSpec};
use edgepad::proxy::DEFAULT_EDGE_WIDTH;

#[derive(Default)]
pub(super) struct RecognitionProfileArgs {
    pub(super) config_path: Option<PathBuf>,
    pub(super) built_in_defaults: bool,
}

enum RecognitionProfileSource {
    Config(PathBuf),
    BuiltInDefaults,
}

pub(super) struct RecognitionProfile {
    source: RecognitionProfileSource,
    pub(super) device: Option<DeviceConfig>,
    pub(super) edge_widths: EdgeWidths,
    pub(super) engine_options: EngineOptions,
    pub(super) slider_specs: Vec<SliderSpec>,
}

impl RecognitionProfile {
    fn from_config(path: PathBuf, config: EdgepadConfig) -> Self {
        Self {
            source: RecognitionProfileSource::Config(path),
            device: Some(config.device.clone()),
            edge_widths: config.active_edge_widths(),
            engine_options: config.engine_options(),
            slider_specs: config.slider_specs(),
        }
    }

    fn built_in_defaults() -> Self {
        Self {
            source: RecognitionProfileSource::BuiltInDefaults,
            device: None,
            edge_widths: EdgeWidths::all(DEFAULT_EDGE_WIDTH),
            engine_options: EngineOptions::default(),
            slider_specs: Vec::new(),
        }
    }

    pub(super) fn override_edge_width(&mut self, width: f32) {
        match self.source {
            RecognitionProfileSource::BuiltInDefaults => {
                self.edge_widths = EdgeWidths::all(width);
            }
            RecognitionProfileSource::Config(_) => {
                self.edge_widths = EdgeWidths {
                    left: active_width_override(self.edge_widths.left, width),
                    right: active_width_override(self.edge_widths.right, width),
                    top: active_width_override(self.edge_widths.top, width),
                    bottom: active_width_override(self.edge_widths.bottom, width),
                };
            }
        }
    }

    pub(super) fn source_label(&self) -> String {
        match &self.source {
            RecognitionProfileSource::Config(path) => {
                format!("config {}", path.display())
            }
            RecognitionProfileSource::BuiltInDefaults => "built-in defaults".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct DaemonConfigOverrides {
    pub(super) device: Option<DeviceConfig>,
    pub(super) edge_width: Option<f32>,
}

impl DaemonConfigOverrides {
    pub(super) fn apply(&self, config: &mut EdgepadConfig) {
        if let Some(device) = &self.device {
            config.device = device.clone();
        }
        if let Some(edge_width) = self.edge_width {
            config.override_edge_width(edge_width);
        }
    }
}

pub(super) fn load_daemon_config(path: &Path) -> Result<EdgepadConfig, String> {
    match fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            Err(format!("daemon config path is not a file: {}", path.display()))
        }
        Ok(_) => load_edgepad_config(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "daemon config not found: {}; pass --config <file> or create it from edgepad.toml.example",
            path.display()
        )),
        Err(err) => Err(format!(
            "failed to inspect daemon config {}: {err}",
            path.display()
        )),
    }
}

pub(super) fn load_recognition_profile(
    profile_args: &RecognitionProfileArgs,
) -> Result<RecognitionProfile, String> {
    if profile_args.built_in_defaults {
        return Ok(RecognitionProfile::built_in_defaults());
    }

    let path = match &profile_args.config_path {
        Some(path) => path.clone(),
        None => default_edgepad_config_path()?,
    };
    let config = load_recognition_config(&path)?;
    if config.gestures.is_empty() && config.sliders.is_empty() {
        return Err(format!(
            "recognition config {} has no bindings; add at least one [[gestures]] or [[sliders]] entry, or pass --built-in-defaults",
            path.display()
        ));
    }

    Ok(RecognitionProfile::from_config(path, config))
}

fn load_recognition_config(path: &Path) -> Result<EdgepadConfig, String> {
    match fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(format!(
            "recognition config path is not a file: {}",
            path.display()
        )),
        Ok(_) => load_edgepad_config(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "recognition config not found: {}; pass --config <file> or --built-in-defaults",
            path.display()
        )),
        Err(err) => Err(format!(
            "failed to inspect recognition config {}: {err}",
            path.display()
        )),
    }
}

pub(super) fn validate_daemon_config(
    config: &EdgepadConfig,
    config_path: &Path,
) -> Result<(), String> {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        return Err(format!(
            "daemon config {} has no bindings; add at least one [[gestures]] or [[sliders]] entry",
            config_path.display()
        ));
    }

    Ok(())
}

fn active_width_override(active_width: f32, override_width: f32) -> f32 {
    if active_width > 0.0 {
        override_width
    } else {
        0.0
    }
}

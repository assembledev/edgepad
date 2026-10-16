use std::path::PathBuf;

use crate::config::DeviceConfig;

mod access;
mod commands;
mod configuration;
mod devices;
mod labels;
mod report;
mod service;
#[cfg(test)]
mod test_support;

pub use report::{CheckStatus, DoctorCheck, DoctorCounts, DoctorReport, DoctorSection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorConfig {
    pub config_path: Option<PathBuf>,
    pub device_override: Option<DeviceConfig>,
    pub input_root: PathBuf,
    pub uinput_path: PathBuf,
    pub service_name: String,
}

impl Default for DoctorConfig {
    fn default() -> Self {
        Self {
            config_path: None,
            device_override: None,
            input_root: PathBuf::from("/dev/input"),
            uinput_path: PathBuf::from("/dev/uinput"),
            service_name: "edgepad.service".to_string(),
        }
    }
}

pub fn run_doctor(config: &DoctorConfig) -> DoctorReport {
    let configuration = configuration::check(
        config.config_path.as_deref(),
        config.device_override.as_ref(),
    );
    let mut checks = configuration.checks;
    if let Some(loaded) = configuration.loaded.as_ref() {
        checks.extend(commands::check(loaded));
    }

    // Device access can still be diagnosed when the config is missing or invalid.
    let devices = devices::check(
        &configuration.device,
        &config.input_root,
        &config.uinput_path,
    );
    checks.extend(devices.checks);
    checks.extend(access::check(&devices.access));
    checks.extend(service::check(&config.service_name));
    DoctorReport { checks }
}

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

use evdev::Device;

use crate::config::DeviceConfig;
use crate::device::{
    discover_device_report, format_device_line, touchpad_candidates, DiscoveryReport,
};

use super::{CheckStatus, DoctorCheck, DoctorSection};

pub(super) struct DeviceAccess {
    pub touchpad_path: Option<PathBuf>,
    pub uinput_path: PathBuf,
    pub both_devices_open: bool,
}

pub(super) struct DeviceChecks {
    pub checks: Vec<DoctorCheck>,
    pub access: DeviceAccess,
}

pub(super) fn check(device: &DeviceConfig, input_root: &Path, uinput_path: &Path) -> DeviceChecks {
    let (touchpad_path, selection) = match device {
        DeviceConfig::Path(path) => (
            Some(path.clone()),
            DoctorCheck::new(
                CheckStatus::Warn,
                DoctorSection::Device,
                "touchpad",
                format!(
                    "skipped because explicit device was provided: {}",
                    path.display()
                ),
            ),
        ),
        DeviceConfig::Auto => auto_selection(discover_device_report(input_root), input_root),
    };
    let (input_open, input) = check_input(touchpad_path.as_deref());
    let (uinput_open, uinput) = check_uinput(uinput_path);
    DeviceChecks {
        checks: vec![selection, input, uinput],
        access: DeviceAccess {
            touchpad_path,
            uinput_path: uinput_path.to_path_buf(),
            both_devices_open: input_open && uinput_open,
        },
    }
}

fn auto_selection(
    discovery: io::Result<DiscoveryReport>,
    input_root: &Path,
) -> (Option<PathBuf>, DoctorCheck) {
    let (path, status, detail) = match discovery {
        Ok(discovery) if discovery.event_node_count == 0 => (
            None,
            CheckStatus::Fail,
            format!("no event devices found under {}", input_root.display()),
        ),
        Ok(discovery) if discovery.summaries.is_empty() => (
            None,
            CheckStatus::Fail,
            format!(
                "{} event node(s) found under {}, but none were readable",
                discovery.event_node_count,
                input_root.display()
            ),
        ),
        Ok(discovery) => {
            let candidates = touchpad_candidates(&discovery.summaries);
            match candidates.as_slice() {
                [] => (
                    None,
                    CheckStatus::Fail,
                    format!(
                        "no touchpad candidates among {} readable event device(s)",
                        discovery.summaries.len()
                    ),
                ),
                [candidate] => (
                    Some(candidate.path.clone()),
                    CheckStatus::Ok,
                    format_device_line(candidate),
                ),
                _ => {
                    let devices = candidates
                        .iter()
                        .map(|candidate| candidate.path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    (
                        None,
                        CheckStatus::Fail,
                        format!(
                            "multiple touchpad candidates found; pass --device explicitly: {devices}"
                        ),
                    )
                }
            }
        }
        Err(err) => (
            None,
            CheckStatus::Fail,
            format!("failed to list {}: {err}", input_root.display()),
        ),
    };
    (
        path,
        DoctorCheck::new(status, DoctorSection::Device, "touchpad", detail),
    )
}

fn check_input(path: Option<&Path>) -> (bool, DoctorCheck) {
    let Some(path) = path else {
        return (
            false,
            DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Device,
                "input",
                "skipped because no touchpad event node is selected",
            ),
        );
    };
    let (opened, detail) = match Device::open(path) {
        Ok(_) => (
            true,
            format!("{} can be opened by current user", path.display()),
        ),
        Err(err) => (false, format!("failed to open {}: {err}", path.display())),
    };
    let status = if opened {
        CheckStatus::Ok
    } else {
        CheckStatus::Fail
    };
    (
        opened,
        DoctorCheck::new(status, DoctorSection::Device, "input", detail),
    )
}

fn check_uinput(path: &Path) -> (bool, DoctorCheck) {
    let (opened, detail) = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(_) => (true, format!("{} is readable and writable", path.display())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => (
            false,
            format!(
                "{} is missing; load the uinput kernel module",
                path.display()
            ),
        ),
        Err(err) => (
            false,
            format!("failed to open {} read/write: {err}", path.display()),
        ),
    };
    let status = if opened {
        CheckStatus::Ok
    } else {
        CheckStatus::Fail
    };
    (
        opened,
        DoctorCheck::new(status, DoctorSection::Device, "uinput", detail),
    )
}

#[cfg(test)]
mod tests;

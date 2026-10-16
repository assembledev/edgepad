//! Argument parsing preserves command-specific diagnostics and validation order.
//! Daemon parsing also loads the initial config, so invalid config fails before
//! device discovery or signal installation. Command handlers receive resolved args.

use std::path::PathBuf;

use edgepad::config::{default_edgepad_config_path, DeviceConfig, EdgepadConfig};
use edgepad::doctor::DoctorConfig;
use edgepad::proxy::ProxyMode;
use edgepad::status::StatusConfig;

use super::help::*;
use super::profile::{
    load_daemon_config, validate_daemon_config, DaemonConfigOverrides, RecognitionProfileArgs,
};

pub(super) struct DeviceArgs {
    pub(super) root: PathBuf,
    pub(super) show_all: bool,
}

pub(super) struct DumpArgs {
    pub(super) device: DeviceConfig,
    pub(super) input_root: PathBuf,
    pub(super) out: PathBuf,
    pub(super) frames: Option<usize>,
    pub(super) raw: bool,
}

pub(super) struct ReplayArgs {
    pub(super) path: PathBuf,
    pub(super) profile: RecognitionProfileArgs,
}

pub(super) struct ProxyArgs {
    pub(super) device_override: Option<DeviceConfig>,
    pub(super) input_root: PathBuf,
    pub(super) frames: usize,
    pub(super) mode: ProxyMode,
    pub(super) edge_width_override: Option<f32>,
    pub(super) profile: RecognitionProfileArgs,
}

pub(super) struct DaemonArgs {
    pub(super) config: EdgepadConfig,
    pub(super) config_path: PathBuf,
    pub(super) input_root: PathBuf,
    pub(super) overrides: DaemonConfigOverrides,
}

pub(super) fn parse_devices_args(
    mut args: impl Iterator<Item = String>,
) -> Result<DeviceArgs, String> {
    let mut root = PathBuf::from("/dev/input");
    let mut show_all = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input-root" | "--root" => {
                root = args.next().ok_or_else(|| DEVICES_USAGE.to_string())?.into();
            }
            "--all" => show_all = true,
            _ => return Err(DEVICES_USAGE.to_string()),
        }
    }

    Ok(DeviceArgs { root, show_all })
}

pub(super) fn parse_doctor_args(
    mut args: impl Iterator<Item = String>,
) -> Result<DoctorConfig, String> {
    let mut config = DoctorConfig::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config.config_path =
                    Some(args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?.into());
            }
            "--device" => {
                let raw_value = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?;
                config.device_override = Some(DeviceConfig::parse(&raw_value)?);
            }
            "--input-root" => {
                config.input_root = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?.into();
            }
            "--uinput" => {
                config.uinput_path = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?.into();
            }
            "--service" => {
                config.service_name = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown doctor option {other}"));
            }
            _ => return Err(DOCTOR_USAGE.to_string()),
        }
    }

    Ok(config)
}

pub(super) fn parse_status_args(
    mut args: impl Iterator<Item = String>,
) -> Result<StatusConfig, String> {
    let mut config = StatusConfig::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config.config_path =
                    Some(args.next().ok_or_else(|| STATUS_USAGE.to_string())?.into());
            }
            "--device" => {
                let raw_value = args.next().ok_or_else(|| STATUS_USAGE.to_string())?;
                config.device_override = Some(DeviceConfig::parse(&raw_value)?);
            }
            "--input-root" => {
                config.input_root = args.next().ok_or_else(|| STATUS_USAGE.to_string())?.into();
            }
            "--service" => {
                config.service_name = args.next().ok_or_else(|| STATUS_USAGE.to_string())?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown status option {other}"));
            }
            _ => return Err(STATUS_USAGE.to_string()),
        }
    }

    Ok(config)
}

pub(super) fn parse_dump_args(mut args: impl Iterator<Item = String>) -> Result<DumpArgs, String> {
    let mut device = None;
    let mut input_root = PathBuf::from("/dev/input");
    let mut out = None;
    let mut frames = None;
    let mut raw = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => {
                let raw_value = args.next().ok_or_else(|| DUMP_USAGE.to_string())?;
                device = Some(DeviceConfig::parse(&raw_value)?);
            }
            "--input-root" => {
                input_root = args.next().ok_or_else(|| DUMP_USAGE.to_string())?.into();
            }
            "--out" => out = Some(args.next().ok_or_else(|| DUMP_USAGE.to_string())?.into()),
            "--frames" => {
                let raw_value = args.next().ok_or_else(|| DUMP_USAGE.to_string())?;
                let parsed = parse_positive_frame_limit(&raw_value)?;
                frames = Some(parsed);
            }
            "--raw" => raw = true,
            _ => return Err(DUMP_USAGE.to_string()),
        }
    }

    Ok(DumpArgs {
        device: device.ok_or_else(|| DUMP_USAGE.to_string())?,
        input_root,
        out: out.ok_or_else(|| DUMP_USAGE.to_string())?,
        frames,
        raw,
    })
}

pub(super) fn parse_replay_args(
    mut args: impl Iterator<Item = String>,
    usage: &str,
    command: &str,
) -> Result<ReplayArgs, String> {
    let mut path = None;
    let mut profile = RecognitionProfileArgs::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                profile.config_path = Some(args.next().ok_or_else(|| usage.to_string())?.into());
            }
            "--built-in-defaults" => profile.built_in_defaults = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown {command} option {other}"));
            }
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err(usage.to_string()),
        }
    }

    validate_profile_args(&profile, command)?;
    Ok(ReplayArgs {
        path: path.ok_or_else(|| usage.to_string())?,
        profile,
    })
}

pub(super) fn parse_proxy_args(
    mut args: impl Iterator<Item = String>,
) -> Result<ProxyArgs, String> {
    let mut device_override = None;
    let mut input_root = PathBuf::from("/dev/input");
    let mut frames = None;
    let mut dry_run = false;
    let mut uinput = false;
    let mut grab = false;
    let mut edge_width_override = None;
    let mut profile = RecognitionProfileArgs::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                profile.config_path =
                    Some(args.next().ok_or_else(|| PROXY_USAGE.to_string())?.into());
            }
            "--built-in-defaults" => profile.built_in_defaults = true,
            "--device" => {
                let raw_value = args.next().ok_or_else(|| PROXY_USAGE.to_string())?;
                device_override = Some(DeviceConfig::parse(&raw_value)?);
            }
            "--input-root" => {
                input_root = args.next().ok_or_else(|| PROXY_USAGE.to_string())?.into();
            }
            "--frames" => {
                let raw_value = args.next().ok_or_else(|| PROXY_USAGE.to_string())?;
                frames = Some(parse_positive_frame_limit(&raw_value)?);
            }
            "--dry-run" => dry_run = true,
            "--uinput" => uinput = true,
            "--grab" => grab = true,
            "--edge-width" => {
                let raw_value = args.next().ok_or_else(|| PROXY_USAGE.to_string())?;
                edge_width_override = Some(parse_edge_width(&raw_value)?);
            }
            "--no-grab" => return Err("unknown proxy option --no-grab".to_string()),
            other if other.starts_with('-') => return Err(format!("unknown proxy option {other}")),
            _ => return Err(PROXY_USAGE.to_string()),
        }
    }

    let frames = frames.ok_or_else(|| PROXY_USAGE.to_string())?;
    let mode = match (dry_run, uinput, grab) {
        (true, false, false) => ProxyMode::DryRun,
        (false, true, true) => ProxyMode::UinputGrab,
        (false, true, false) => return Err("proxy --uinput requires --grab".to_string()),
        (false, false, true) => return Err("proxy --grab requires --uinput".to_string()),
        (false, false, false) => {
            return Err("proxy requires either --dry-run or --uinput --grab".to_string())
        }
        _ => return Err("proxy modes are mutually exclusive".to_string()),
    };
    validate_profile_args(&profile, "proxy")?;

    Ok(ProxyArgs {
        device_override,
        input_root,
        frames,
        mode,
        edge_width_override,
        profile,
    })
}

pub(super) fn parse_daemon_args(
    mut args: impl Iterator<Item = String>,
) -> Result<DaemonArgs, String> {
    let mut config_path: Option<PathBuf> = None;
    let mut device_override = None;
    let mut edge_width_override = None;
    let mut input_root = PathBuf::from("/dev/input");

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config_path = Some(args.next().ok_or_else(|| DAEMON_USAGE.to_string())?.into());
            }
            "--device" => {
                let raw_value = args.next().ok_or_else(|| DAEMON_USAGE.to_string())?;
                device_override = Some(DeviceConfig::parse(&raw_value)?);
            }
            "--input-root" => {
                input_root = args.next().ok_or_else(|| DAEMON_USAGE.to_string())?.into();
            }
            "--edge-width" => {
                let raw_value = args.next().ok_or_else(|| DAEMON_USAGE.to_string())?;
                edge_width_override = Some(parse_edge_width(&raw_value)?);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown daemon option {other}"))
            }
            _ => return Err(DAEMON_USAGE.to_string()),
        }
    }

    let config_path = match config_path {
        Some(path) => path,
        None => default_edgepad_config_path()?,
    };
    let overrides = DaemonConfigOverrides {
        device: device_override,
        edge_width: edge_width_override,
    };
    let mut config = load_daemon_config(&config_path)?;
    overrides.apply(&mut config);
    validate_daemon_config(&config, &config_path)?;

    Ok(DaemonArgs {
        config,
        config_path,
        input_root,
        overrides,
    })
}

fn validate_profile_args(profile: &RecognitionProfileArgs, command: &str) -> Result<(), String> {
    if profile.built_in_defaults && profile.config_path.is_some() {
        return Err(format!(
            "{command} --config and --built-in-defaults are mutually exclusive"
        ));
    }
    Ok(())
}

fn parse_positive_frame_limit(raw_value: &str) -> Result<usize, String> {
    let parsed = raw_value
        .parse::<usize>()
        .map_err(|_| "--frames must be a positive integer".to_string())?;
    if parsed == 0 {
        return Err("--frames must be a positive integer".to_string());
    }
    Ok(parsed)
}

fn parse_edge_width(raw_value: &str) -> Result<f32, String> {
    edgepad::config::parse_edge_width(raw_value)
}

#[cfg(test)]
mod tests;

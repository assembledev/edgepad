//! Declarative CLI syntax. Clap validates values and argument relationships before
//! command execution. Config loading stays explicit so help never opens files,
//! discovers devices, or installs signal handlers.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use edgepad::config::{default_edgepad_config_path, DeviceConfig, EdgepadConfig};
use edgepad::doctor::DoctorConfig;
use edgepad::proxy::ProxyMode;
use edgepad::status::StatusConfig;

use super::profile::{
    load_daemon_config, validate_daemon_config, DaemonConfigOverrides, RecognitionProfileArgs,
    CONFIG_PATH_HELP,
};

#[derive(Debug, Parser)]
#[command(name = "edgepad", version, about, args_override_self = true)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// List readable input devices and touchpad candidates
    Devices(DeviceArgs),
    /// Show a short daemon/config/device summary
    Status(DiagnosticArgs),
    /// Check runtime prerequisites and service health
    Doctor(DoctorArgs),
    /// Run the live edge-gesture proxy
    #[command(after_help = "Signals:\n  SIGHUP  Reload config after active contacts lift")]
    Daemon(DaemonOptions),
    /// Capture touchpad events into a replay fixture
    #[command(
        after_help = "The physical device must not be grabbed by another process.\nStop edgepad.service before capturing when the daemon owns the touchpad."
    )]
    Dump(DumpArgs),
    /// Run a bounded live proxy session for diagnostics
    Proxy(ProxyArgs),
    /// Replay a parsed fixture through the recognizer
    Replay(ReplayArgs),
    /// Replay a raw evdev capture through routing and output composition
    ReplayRaw(ReplayArgs),
}

#[derive(Debug, Args)]
pub(super) struct DeviceArgs {
    /// Input device directory
    #[arg(
        long = "input-root",
        visible_alias = "root",
        default_value = "/dev/input",
        value_name = "INPUT_ROOT"
    )]
    pub(super) root: PathBuf,
    /// Show all readable event devices, not only touchpad candidates
    #[arg(long = "all")]
    pub(super) show_all: bool,
}

#[derive(Debug, Args)]
pub(super) struct DiagnosticArgs {
    #[arg(long = "config", value_name = "FILE", help = CONFIG_PATH_HELP)]
    config_path: Option<PathBuf>,
    /// Override touchpad device selection from config (auto or event node)
    #[arg(long = "device", value_name = "DEVICE")]
    device_override: Option<DeviceConfig>,
    /// Input device directory for auto-detection
    #[arg(long, default_value = "/dev/input", value_name = "INPUT_ROOT")]
    input_root: PathBuf,
    /// Systemd user unit to inspect
    #[arg(
        long = "service",
        default_value = "edgepad.service",
        value_name = "UNIT"
    )]
    service_name: String,
}

impl From<DiagnosticArgs> for StatusConfig {
    fn from(args: DiagnosticArgs) -> Self {
        Self {
            config_path: args.config_path,
            device_override: args.device_override,
            input_root: args.input_root,
            service_name: args.service_name,
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct DoctorArgs {
    #[command(flatten)]
    common: DiagnosticArgs,
    /// Uinput device path
    #[arg(long = "uinput", default_value = "/dev/uinput", value_name = "PATH")]
    uinput_path: PathBuf,
}

impl From<DoctorArgs> for DoctorConfig {
    fn from(args: DoctorArgs) -> Self {
        Self {
            config_path: args.common.config_path,
            device_override: args.common.device_override,
            input_root: args.common.input_root,
            service_name: args.common.service_name,
            uinput_path: args.uinput_path,
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct DumpArgs {
    /// Physical touchpad selection (auto or event node)
    #[arg(long, value_name = "DEVICE")]
    pub(super) device: DeviceConfig,
    /// Input device directory for auto-detection
    #[arg(long, default_value = "/dev/input", value_name = "INPUT_ROOT")]
    pub(super) input_root: PathBuf,
    /// Output fixture path
    #[arg(long, value_name = "FILE")]
    pub(super) out: PathBuf,
    /// Capture at least N frame boundaries, then stop when idle
    #[arg(long, value_name = "N", value_parser = parse_positive_frame_limit)]
    pub(super) frames: Option<usize>,
    /// Write raw evdev events instead of replay fixture events
    #[arg(long)]
    pub(super) raw: bool,
}

#[derive(Debug, Args)]
pub(super) struct ReplayArgs {
    /// Capture to replay
    #[arg(value_name = "FILE")]
    pub(super) path: PathBuf,
    #[command(flatten)]
    pub(super) profile: RecognitionProfileArgs,
}

#[derive(Debug, Args)]
pub(super) struct ProxyArgs {
    /// Override config device selection; required with built-in defaults
    #[arg(long = "device", value_name = "DEVICE")]
    pub(super) device_override: Option<DeviceConfig>,
    /// Input device directory for auto-detection
    #[arg(long, default_value = "/dev/input", value_name = "INPUT_ROOT")]
    pub(super) input_root: PathBuf,
    /// Stop after N frame boundaries
    #[arg(long, value_name = "N", value_parser = parse_positive_frame_limit)]
    pub(super) frames: usize,
    /// Override edge zone width from the selected profile
    #[arg(long = "edge-width", value_name = "F", value_parser = edgepad::config::parse_edge_width)]
    pub(super) edge_width_override: Option<f32>,
    #[command(flatten)]
    pub(super) profile: RecognitionProfileArgs,
    #[command(flatten)]
    mode: ProxyModeArgs,
    /// Grab the physical device (requires --uinput)
    #[arg(long, requires = "uinput")]
    grab: bool,
}

#[derive(Debug, Args)]
#[group(id = "proxy_mode", required = true, multiple = false)]
struct ProxyModeArgs {
    /// Recognize and report without emitting output
    #[arg(long, conflicts_with = "grab")]
    dry_run: bool,
    /// Emit output through /dev/uinput (requires --grab)
    #[arg(long, requires = "grab")]
    uinput: bool,
}

impl ProxyArgs {
    pub(super) fn mode(&self) -> ProxyMode {
        // The required group, conflicts, and dependencies leave two valid states.
        // Conversion runs only after clap has validated that whole relationship.
        match (self.mode.dry_run, self.mode.uinput, self.grab) {
            (true, false, false) => ProxyMode::DryRun,
            (false, true, true) => ProxyMode::UinputGrab,
            _ => unreachable!("clap validated the proxy mode"),
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct DaemonOptions {
    #[arg(long = "config", value_name = "FILE", help = CONFIG_PATH_HELP)]
    config_path: Option<PathBuf>,
    /// Input device directory for auto-detection
    #[arg(long, default_value = "/dev/input", value_name = "INPUT_ROOT")]
    input_root: PathBuf,
    #[command(flatten)]
    overrides: DaemonConfigOverrides,
}

pub(super) struct DaemonArgs {
    pub(super) config: EdgepadConfig,
    pub(super) config_path: PathBuf,
    pub(super) input_root: PathBuf,
    pub(super) overrides: DaemonConfigOverrides,
}

impl DaemonOptions {
    pub(super) fn load(self) -> Result<DaemonArgs, String> {
        let config_path = match self.config_path {
            Some(path) => path,
            None => default_edgepad_config_path()?,
        };
        let mut config = load_daemon_config(&config_path)?;
        self.overrides.apply(&mut config);
        validate_daemon_config(&config, &config_path)?;
        Ok(DaemonArgs {
            config,
            config_path,
            input_root: self.input_root,
            overrides: self.overrides,
        })
    }
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

#[cfg(test)]
mod tests;

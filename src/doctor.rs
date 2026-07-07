use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use evdev::Device;

use crate::config::{
    default_edgepad_config_path, load_edgepad_config, DeviceConfig, EdgepadConfig,
    GestureActionConfig,
};
use crate::core::{EdgeWidths, GestureDirection, Zone};
use crate::device::{discover_device_report, format_device_line, touchpad_candidates};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheck {
    pub status: CheckStatus,
    pub name: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
    }

    pub fn counts(&self) -> DoctorCounts {
        let mut counts = DoctorCounts::default();
        for check in &self.checks {
            match check.status {
                CheckStatus::Ok => counts.ok += 1,
                CheckStatus::Warn => counts.warn += 1,
                CheckStatus::Fail => counts.fail += 1,
            }
        }
        counts
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoctorCounts {
    pub ok: usize,
    pub warn: usize,
    pub fail: usize,
}

pub fn run_doctor(config: &DoctorConfig) -> DoctorReport {
    let mut report = DoctorReport::default();
    let loaded_config = check_config(config, &mut report);
    let effective_device = check_config_device(
        loaded_config.as_ref(),
        config.device_override.as_ref(),
        &mut report,
    );
    if let Some(loaded_config) = loaded_config.as_ref() {
        check_action_executables(loaded_config, &mut report);
    }

    let touchpad_path =
        check_touchpad_selection(&effective_device, &config.input_root, &mut report);

    let touchpad_readable = check_touchpad_readable(touchpad_path.as_deref(), &mut report);
    let uinput_readable = check_uinput(&config.uinput_path, &mut report);
    let device_access_ok = touchpad_readable && uinput_readable;
    check_uaccess_tags(
        touchpad_path.as_deref(),
        &config.uinput_path,
        device_access_ok,
        &mut report,
    );
    check_uaccess_acl(
        touchpad_path.as_deref(),
        &config.uinput_path,
        device_access_ok,
        &mut report,
    );
    check_logind_seat(device_access_ok, &mut report);
    let systemd_user_available = check_systemd_user(&mut report);
    check_service_status(&config.service_name, systemd_user_available, &mut report);

    report
}

fn check_config(config: &DoctorConfig, report: &mut DoctorReport) -> Option<EdgepadConfig> {
    let path = match &config.config_path {
        Some(path) => path.clone(),
        None => match default_edgepad_config_path() {
            Ok(path) => path,
            Err(err) => {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Fail,
                    name: "config path",
                    detail: err,
                });
                return None;
            }
        },
    };

    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => report.checks.push(DoctorCheck {
            status: CheckStatus::Ok,
            name: "config path",
            detail: format!("{}", path.display()),
        }),
        Ok(_) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "config path",
                detail: format!("not a file: {}", path.display()),
            });
            return None;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "config path",
                detail: format!(
                    "not found: {}; pass --config <file> or create ~/.config/edgepad/edgepad.toml",
                    path.display()
                ),
            });
            return None;
        }
        Err(err) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "config path",
                detail: format!("failed to inspect {}: {err}", path.display()),
            });
            return None;
        }
    }

    match load_edgepad_config(&path) {
        Ok(edgepad_config) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "config parse",
                detail: format!(
                    "device={} edge_width={:.3} gesture_bindings={}",
                    device_config_label(&edgepad_config.device),
                    edgepad_config.edge_width,
                    edgepad_config.gestures.len()
                ),
            });
            check_gesture_bindings(&edgepad_config, report);
            Some(edgepad_config)
        }
        Err(err) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "config parse",
                detail: err,
            });
            None
        }
    }
}

fn check_gesture_bindings(config: &EdgepadConfig, report: &mut DoctorReport) {
    if config.gestures.is_empty() {
        report.checks.push(DoctorCheck {
            status: CheckStatus::Fail,
            name: "gesture bindings",
            detail: "no gesture bindings configured; add at least one [[gestures]] entry"
                .to_string(),
        });
        return;
    }

    report.checks.push(DoctorCheck {
        status: CheckStatus::Ok,
        name: "gesture bindings",
        detail: format!("{} gesture binding(s) configured", config.gestures.len()),
    });
    report.checks.push(DoctorCheck {
        status: CheckStatus::Ok,
        name: "active zones",
        detail: active_zones_detail(config),
    });
}

fn active_zones_detail(config: &EdgepadConfig) -> String {
    let active_zones: Vec<Zone> = ordered_zones()
        .iter()
        .copied()
        .filter(|zone| config.gestures.iter().any(|binding| binding.zone == *zone))
        .collect();
    let inactive_zones: Vec<Zone> = ordered_zones()
        .iter()
        .copied()
        .filter(|zone| !active_zones.contains(zone))
        .collect();

    format!(
        "active_zones={} inactive_zones={} edge_widths={}",
        zone_list_label(&active_zones),
        zone_list_label(&inactive_zones),
        edge_widths_label(config.active_edge_widths())
    )
}

fn ordered_zones() -> [Zone; 4] {
    [Zone::Left, Zone::Right, Zone::Top, Zone::Bottom]
}

fn zone_list_label(zones: &[Zone]) -> String {
    if zones.is_empty() {
        return "none".to_string();
    }

    zones
        .iter()
        .map(|zone| zone_name(*zone))
        .collect::<Vec<_>>()
        .join(",")
}

fn edge_widths_label(widths: EdgeWidths) -> String {
    format!(
        "left={:.3} right={:.3} top={:.3} bottom={:.3}",
        widths.left, widths.right, widths.top, widths.bottom
    )
}

fn check_config_device(
    loaded_config: Option<&EdgepadConfig>,
    device_override: Option<&DeviceConfig>,
    report: &mut DoctorReport,
) -> DeviceConfig {
    match (loaded_config, device_override) {
        (_, Some(device)) => {
            let detail = match loaded_config {
                Some(config) => format!(
                    "{} from config overridden by --device {}",
                    device_config_label(&config.device),
                    device_config_label(device)
                ),
                None => format!("using --device {}", device_config_label(device)),
            };
            report.checks.push(DoctorCheck {
                status: CheckStatus::Warn,
                name: "config device",
                detail,
            });
            device.clone()
        }
        (Some(config), None) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "config device",
                detail: format!("using {}", device_config_label(&config.device)),
            });
            config.device.clone()
        }
        (None, None) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Warn,
                name: "config device",
                detail: "using device=auto because config was not loaded".to_string(),
            });
            DeviceConfig::Auto
        }
    }
}

fn check_action_executables(config: &EdgepadConfig, report: &mut DoctorReport) {
    if config.gestures.is_empty() {
        return;
    }

    let mut usages = BTreeMap::<String, Vec<String>>::new();
    for (index, binding) in config.gestures.iter().enumerate() {
        if let GestureActionConfig::Command { argv } = &binding.action {
            if let Some(program) = argv.first() {
                usages
                    .entry(program.clone())
                    .or_default()
                    .push(gesture_binding_label(
                        index,
                        binding.zone,
                        binding.direction,
                    ));
            }
        }
    }

    if usages.is_empty() {
        report.checks.push(DoctorCheck {
            status: CheckStatus::Ok,
            name: "action executable",
            detail: "no command actions configured".to_string(),
        });
        return;
    }

    for (program, bindings) in usages {
        let usage = format_binding_usage(&bindings);
        match action_executable_status(&program) {
            ActionExecutableStatus::Found(path) => report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "action executable",
                detail: format!("{program} found at {} for {usage}", path.display()),
            }),
            ActionExecutableStatus::AbsolutePathExecutable => report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "action executable",
                detail: format!("{program} is executable for {usage}"),
            }),
            ActionExecutableStatus::RelativePathExecutable => report.checks.push(DoctorCheck {
                status: CheckStatus::Warn,
                name: "action executable",
                detail: format!(
                    "{program} is a relative executable path for {usage}; user services may run from a different directory"
                ),
            }),
            ActionExecutableStatus::Missing(message) => report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "action executable",
                detail: format!("{message} for {usage}"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActionExecutableStatus {
    Found(PathBuf),
    AbsolutePathExecutable,
    RelativePathExecutable,
    Missing(String),
}

fn action_executable_status(program: &str) -> ActionExecutableStatus {
    if program.contains('/') {
        return path_executable_status(Path::new(program));
    }

    match find_executable_in_path(program, env::var_os("PATH")) {
        Some(path) => ActionExecutableStatus::Found(path),
        None => ActionExecutableStatus::Missing(format!("{program} not found in PATH")),
    }
}

fn path_executable_status(path: &Path) -> ActionExecutableStatus {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {
            if path.is_absolute() {
                ActionExecutableStatus::AbsolutePathExecutable
            } else {
                ActionExecutableStatus::RelativePathExecutable
            }
        }
        Ok(metadata) if metadata.is_file() => ActionExecutableStatus::Missing(format!(
            "{} exists but is not executable",
            path.display()
        )),
        Ok(_) => ActionExecutableStatus::Missing(format!("{} is not a file", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ActionExecutableStatus::Missing(format!("{} not found", path.display()))
        }
        Err(err) => {
            ActionExecutableStatus::Missing(format!("failed to inspect {}: {err}", path.display()))
        }
    }
}

fn find_executable_in_path(program: &str, path_env: Option<OsString>) -> Option<PathBuf> {
    let path_env = path_env?;
    env::split_paths(&path_env)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            fs::metadata(candidate)
                .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}

fn format_binding_usage(bindings: &[String]) -> String {
    match bindings {
        [] => "0 binding(s)".to_string(),
        [binding] => binding.clone(),
        _ => format!("{} binding(s): {}", bindings.len(), bindings.join(", ")),
    }
}

fn gesture_binding_label(index: usize, zone: Zone, direction: GestureDirection) -> String {
    format!(
        "gestures[{index}] {}.{}",
        zone_name(zone),
        direction_name(direction)
    )
}

fn device_config_label(device: &DeviceConfig) -> String {
    match device {
        DeviceConfig::Auto => "device=auto".to_string(),
        DeviceConfig::Path(path) => format!("device={}", path.display()),
    }
}

fn check_touchpad_selection(
    device: &DeviceConfig,
    input_root: &Path,
    report: &mut DoctorReport,
) -> Option<PathBuf> {
    match device {
        DeviceConfig::Path(path) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Warn,
                name: "touchpad auto-detect",
                detail: format!(
                    "skipped because explicit device was provided: {}",
                    path.display()
                ),
            });
            Some(path.clone())
        }
        DeviceConfig::Auto => match discover_device_report(input_root) {
            Ok(discovery) if discovery.event_node_count == 0 => {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Fail,
                    name: "touchpad auto-detect",
                    detail: format!("no event devices found under {}", input_root.display()),
                });
                None
            }
            Ok(discovery) if discovery.summaries.is_empty() => {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Fail,
                    name: "touchpad auto-detect",
                    detail: format!(
                        "{} event node(s) found under {}, but none were readable",
                        discovery.event_node_count,
                        input_root.display()
                    ),
                });
                None
            }
            Ok(discovery) => {
                let candidates = touchpad_candidates(&discovery.summaries);
                match candidates.as_slice() {
                    [] => {
                        report.checks.push(DoctorCheck {
                            status: CheckStatus::Fail,
                            name: "touchpad auto-detect",
                            detail: format!(
                                "no touchpad candidates among {} readable event device(s)",
                                discovery.summaries.len()
                            ),
                        });
                        None
                    }
                    [candidate] => {
                        report.checks.push(DoctorCheck {
                            status: CheckStatus::Ok,
                            name: "touchpad auto-detect",
                            detail: format_device_line(candidate),
                        });
                        Some(candidate.path.clone())
                    }
                    _ => {
                        let devices = candidates
                            .iter()
                            .map(|candidate| candidate.path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        report.checks.push(DoctorCheck {
                            status: CheckStatus::Fail,
                            name: "touchpad auto-detect",
                            detail: format!(
                                "multiple touchpad candidates found; pass --device explicitly: {devices}"
                            ),
                        });
                        None
                    }
                }
            }
            Err(err) => {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Fail,
                    name: "touchpad auto-detect",
                    detail: format!("failed to list {}: {err}", input_root.display()),
                });
                None
            }
        },
    }
}

fn check_touchpad_readable(path: Option<&Path>, report: &mut DoctorReport) -> bool {
    let Some(path) = path else {
        report.checks.push(DoctorCheck {
            status: CheckStatus::Fail,
            name: "touchpad readable",
            detail: "skipped because no touchpad event node is selected".to_string(),
        });
        return false;
    };

    match Device::open(path) {
        Ok(_) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "touchpad readable",
                detail: format!("{} can be opened by current user", path.display()),
            });
            true
        }
        Err(err) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "touchpad readable",
                detail: format!("failed to open {}: {err}", path.display()),
            });
            false
        }
    }
}

fn check_uinput(path: &Path, report: &mut DoctorReport) -> bool {
    match OpenOptions::new().read(true).write(true).open(path) {
        Ok(_) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "/dev/uinput",
                detail: format!("{} is readable and writable", path.display()),
            });
            true
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "/dev/uinput",
                detail: format!(
                    "{} is missing; load the uinput kernel module",
                    path.display()
                ),
            });
            false
        }
        Err(err) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "/dev/uinput",
                detail: format!("failed to open {} read/write: {err}", path.display()),
            });
            false
        }
    }
}

fn check_uaccess_tags(
    touchpad_path: Option<&Path>,
    uinput_path: &Path,
    device_access_ok: bool,
    report: &mut DoctorReport,
) {
    let Some(touchpad_path) = touchpad_path else {
        report.checks.push(DoctorCheck {
            status: CheckStatus::Fail,
            name: "uaccess tags",
            detail: "skipped because no touchpad event node is selected".to_string(),
        });
        return;
    };

    let touchpad = udevadm_current_tags(touchpad_path);
    let uinput = udevadm_current_tags(uinput_path);

    match (touchpad, uinput) {
        (Ok(touchpad_tags), Ok(uinput_tags))
            if has_tag(&touchpad_tags, "uaccess")
                && has_tag(&touchpad_tags, "seat")
                && has_tag(&uinput_tags, "uaccess")
                && has_tag(&uinput_tags, "seat") =>
        {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "uaccess tags",
                detail: format!(
                    "{} and {} have current udev tags seat,uaccess",
                    touchpad_path.display(),
                    uinput_path.display()
                ),
            });
        }
        (Ok(touchpad_tags), Ok(uinput_tags)) => {
            report.checks.push(DoctorCheck {
                status: fallback_sensitive_status(device_access_ok),
                name: "uaccess tags",
                detail: format!(
                    "missing seat/uaccess current tags; touchpad={touchpad_tags:?} uinput={uinput_tags:?}{}",
                    fallback_context(device_access_ok),
                ),
            });
        }
        (Err(err), _) | (_, Err(err)) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "uaccess tags",
                detail: err,
            });
        }
    }
}

fn udevadm_current_tags(path: &Path) -> Result<Vec<String>, String> {
    let output = command_output(
        "udevadm",
        &[
            "info",
            "--query=property",
            "--name",
            &path.display().to_string(),
        ],
    )?;
    if !output.status_success {
        return Err(format!(
            "udevadm info failed for {}: {}",
            path.display(),
            output.stderr_or_stdout()
        ));
    }

    Ok(parse_udev_current_tags(&output.stdout))
}

fn check_uaccess_acl(
    touchpad_path: Option<&Path>,
    uinput_path: &Path,
    device_access_ok: bool,
    report: &mut DoctorReport,
) {
    let Some(touchpad_path) = touchpad_path else {
        report.checks.push(DoctorCheck {
            status: CheckStatus::Fail,
            name: "uaccess ACL",
            detail: "skipped because no touchpad event node is selected".to_string(),
        });
        return;
    };

    let username = current_username();
    let Some(username) = username else {
        report.checks.push(DoctorCheck {
            status: CheckStatus::Warn,
            name: "uaccess ACL",
            detail: "could not determine current username for ACL inspection".to_string(),
        });
        return;
    };

    let touchpad_acl = getfacl_grants_user(touchpad_path, &username);
    let uinput_acl = getfacl_grants_user(uinput_path, &username);

    match (touchpad_acl, uinput_acl) {
        (Ok(true), Ok(true)) => report.checks.push(DoctorCheck {
            status: CheckStatus::Ok,
            name: "uaccess ACL",
            detail: format!(
                "user {username} has rw ACL on {} and {}",
                touchpad_path.display(),
                uinput_path.display()
            ),
        }),
        (Ok(touchpad_ok), Ok(uinput_ok)) => report.checks.push(DoctorCheck {
            status: fallback_sensitive_status(device_access_ok),
            name: "uaccess ACL",
            detail: format!(
                "missing rw ACL for user {username}; touchpad_acl={touchpad_ok} uinput_acl={uinput_ok}{}",
                fallback_context(device_access_ok),
            ),
        }),
        (Err(err), _) | (_, Err(err)) => report.checks.push(DoctorCheck {
            status: CheckStatus::Warn,
            name: "uaccess ACL",
            detail: err,
        }),
    }
}

fn current_username() -> Option<String> {
    env::var("USER")
        .ok()
        .filter(|user| !user.is_empty())
        .or_else(|| {
            let output = command_output("id", &["-un"]).ok()?;
            output
                .status_success
                .then(|| output.stdout.trim().to_string())
        })
        .filter(|user| !user.is_empty())
}

fn getfacl_grants_user(path: &Path, username: &str) -> Result<bool, String> {
    let output = command_output("getfacl", &["-cp", &path.display().to_string()])?;
    if !output.status_success {
        return Err(format!(
            "getfacl failed for {}: {}",
            path.display(),
            output.stderr_or_stdout()
        ));
    }

    Ok(acl_output_grants_user(&output.stdout, username))
}

fn check_logind_seat(device_access_ok: bool, report: &mut DoctorReport) {
    match command_output("loginctl", &["session-status", "--no-pager"]) {
        Ok(output)
            if output.status_success
                && output.stdout.contains("State: active")
                && output.stdout.contains("Seat: seat") =>
        {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "logind seat",
                detail: "current session is active on a local seat".to_string(),
            });
        }
        Ok(output) if output.status_success => report.checks.push(DoctorCheck {
            status: fallback_sensitive_status(device_access_ok),
            name: "logind seat",
            detail: format!(
                "current loginctl session is not active on a local seat{}",
                fallback_context(device_access_ok)
            ),
        }),
        Ok(output) => report.checks.push(DoctorCheck {
            status: fallback_sensitive_status(device_access_ok),
            name: "logind seat",
            detail: format!(
                "loginctl session-status failed: {}{}",
                output.stderr_or_stdout(),
                fallback_context(device_access_ok)
            ),
        }),
        Err(err) => report.checks.push(DoctorCheck {
            status: fallback_sensitive_status(device_access_ok),
            name: "logind seat",
            detail: format!("{err}{}", fallback_context(device_access_ok)),
        }),
    }
}

fn fallback_sensitive_status(device_access_ok: bool) -> CheckStatus {
    if device_access_ok {
        CheckStatus::Warn
    } else {
        CheckStatus::Fail
    }
}

fn fallback_context(device_access_ok: bool) -> &'static str {
    if device_access_ok {
        "; device access is currently functional, so group or broader filesystem permissions may be masking this"
    } else {
        ""
    }
}

fn check_systemd_user(report: &mut DoctorReport) -> bool {
    match command_output("systemctl", &["--user", "show-environment"]) {
        Ok(output) if output.status_success => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Ok,
                name: "systemd user",
                detail: "systemctl --user is available".to_string(),
            });
            true
        }
        Ok(output) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "systemd user",
                detail: format!("systemctl --user failed: {}", output.stderr_or_stdout()),
            });
            false
        }
        Err(err) => {
            report.checks.push(DoctorCheck {
                status: CheckStatus::Fail,
                name: "systemd user",
                detail: err,
            });
            false
        }
    }
}

fn check_service_status(
    service_name: &str,
    systemd_user_available: bool,
    report: &mut DoctorReport,
) {
    if !systemd_user_available {
        report.checks.push(DoctorCheck {
            status: CheckStatus::Warn,
            name: "edgepad service",
            detail: "skipped because systemctl --user is not available".to_string(),
        });
        return;
    }

    let output = command_output(
        "systemctl",
        &[
            "--user",
            "show",
            service_name,
            "-p",
            "LoadState",
            "-p",
            "ActiveState",
            "-p",
            "SubState",
        ],
    );

    match output {
        Ok(output) if output.status_success => {
            let load_state = property_value(&output.stdout, "LoadState").unwrap_or("unknown");
            let active_state = property_value(&output.stdout, "ActiveState").unwrap_or("unknown");
            let sub_state = property_value(&output.stdout, "SubState").unwrap_or("unknown");
            if load_state == "not-found" {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Warn,
                    name: "edgepad service",
                    detail: format!("{service_name} is not installed in the user manager"),
                });
            } else if active_state == "active" {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Ok,
                    name: "edgepad service",
                    detail: format!("{service_name} is active ({sub_state})"),
                });
            } else if active_state == "failed" {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Fail,
                    name: "edgepad service",
                    detail: format!("{service_name} is failed ({sub_state})"),
                });
            } else {
                report.checks.push(DoctorCheck {
                    status: CheckStatus::Warn,
                    name: "edgepad service",
                    detail: format!("{service_name} is loaded={load_state} active={active_state} sub={sub_state}"),
                });
            }
        }
        Ok(output) => report.checks.push(DoctorCheck {
            status: CheckStatus::Warn,
            name: "edgepad service",
            detail: format!(
                "could not inspect {service_name}: {}",
                output.stderr_or_stdout()
            ),
        }),
        Err(err) => report.checks.push(DoctorCheck {
            status: CheckStatus::Warn,
            name: "edgepad service",
            detail: err,
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandOutput {
    status_success: bool,
    stdout: String,
    stderr: String,
}

impl CommandOutput {
    fn stderr_or_stdout(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            stderr.to_string()
        } else {
            self.stdout.trim().to_string()
        }
    }
}

fn command_output(program: &str, args: &[&str]) -> Result<CommandOutput, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("failed to run {program}: {err}"))?;

    Ok(CommandOutput {
        status_success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn parse_udev_current_tags(output: &str) -> Vec<String> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("CURRENT_TAGS="))
        .or_else(|| output.lines().find_map(|line| line.strip_prefix("TAGS=")))
        .map(parse_colon_tags)
        .unwrap_or_default()
}

fn parse_colon_tags(raw: &str) -> Vec<String> {
    raw.split(':')
        .filter(|tag| !tag.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn has_tag(tags: &[String], wanted: &str) -> bool {
    tags.iter().any(|tag| tag == wanted)
}

fn acl_output_grants_user(output: &str, username: &str) -> bool {
    let prefix = format!("user:{username}:");
    output.lines().any(|line| {
        let Some(perms) = line.strip_prefix(&prefix) else {
            return false;
        };
        perms.as_bytes().first() == Some(&b'r') && perms.as_bytes().get(1) == Some(&b'w')
    })
}

fn property_value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

fn direction_name(direction: GestureDirection) -> &'static str {
    match direction {
        GestureDirection::Up => "up",
        GestureDirection::Down => "down",
        GestureDirection::Left => "left",
        GestureDirection::Right => "right",
        GestureDirection::Tap => "tap",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GestureActionConfig, GestureBindingConfig};

    #[test]
    fn parses_current_tags_from_udevadm_properties() {
        let tags = parse_udev_current_tags(
            "DEVNAME=/dev/input/event7\nTAGS=:seat:uaccess:\nCURRENT_TAGS=:uaccess:seat:\n",
        );

        assert_eq!(tags, vec!["uaccess", "seat"]);
    }

    #[test]
    fn falls_back_to_static_tags_when_current_tags_are_absent() {
        let tags = parse_udev_current_tags("DEVNAME=/dev/uinput\nTAGS=:seat:uaccess:\n");

        assert_eq!(tags, vec!["seat", "uaccess"]);
    }

    #[test]
    fn detects_named_user_rw_acl() {
        assert!(acl_output_grants_user(
            "user::rw-\nuser:use:rw-\ngroup::---\n",
            "use"
        ));
        assert!(!acl_output_grants_user(
            "user::rw-\nuser:use:r--\ngroup::---\n",
            "use"
        ));
    }

    #[test]
    fn fallback_sensitive_status_warns_when_device_access_is_functional() {
        assert_eq!(fallback_sensitive_status(true), CheckStatus::Warn);
        assert_eq!(fallback_sensitive_status(false), CheckStatus::Fail);
    }

    #[test]
    fn fallback_context_explains_functional_non_uaccess_access() {
        assert!(fallback_context(true).contains("device access is currently functional"));
        assert_eq!(fallback_context(false), "");
    }

    #[test]
    fn active_zones_detail_reports_daemon_claim_widths() {
        let config = EdgepadConfig {
            device: DeviceConfig::Auto,
            edge_width: 0.20,
            gestures: vec![
                GestureBindingConfig {
                    zone: Zone::Right,
                    direction: GestureDirection::Up,
                    action: GestureActionConfig::Log,
                },
                GestureBindingConfig {
                    zone: Zone::Top,
                    direction: GestureDirection::Left,
                    action: GestureActionConfig::Log,
                },
            ],
        };

        assert_eq!(
            active_zones_detail(&config),
            "active_zones=right,top inactive_zones=left,bottom edge_widths=left=0.000 right=0.200 top=0.200 bottom=0.000"
        );
    }

    #[test]
    fn config_device_uses_config_when_no_cli_override_is_present() {
        let mut report = DoctorReport::default();
        let config = EdgepadConfig {
            device: DeviceConfig::Path(PathBuf::from("/dev/input/event7")),
            edge_width: 0.10,
            gestures: Vec::new(),
        };

        let device = check_config_device(Some(&config), None, &mut report);

        assert_eq!(
            device,
            DeviceConfig::Path(PathBuf::from("/dev/input/event7"))
        );
        assert_eq!(report.checks[0].status, CheckStatus::Ok);
        assert!(report.checks[0].detail.contains("/dev/input/event7"));
    }

    #[test]
    fn config_device_uses_cli_override_when_present() {
        let mut report = DoctorReport::default();
        let config = EdgepadConfig {
            device: DeviceConfig::Auto,
            edge_width: 0.10,
            gestures: Vec::new(),
        };

        let device = check_config_device(
            Some(&config),
            Some(&DeviceConfig::Path(PathBuf::from("/dev/input/event9"))),
            &mut report,
        );

        assert_eq!(
            device,
            DeviceConfig::Path(PathBuf::from("/dev/input/event9"))
        );
        assert_eq!(report.checks[0].status, CheckStatus::Warn);
        assert!(report.checks[0].detail.contains("overridden by --device"));
    }

    #[test]
    fn action_executable_check_reports_missing_path_command() {
        let mut report = DoctorReport::default();
        let config = EdgepadConfig {
            device: DeviceConfig::Auto,
            edge_width: 0.10,
            gestures: vec![GestureBindingConfig {
                zone: Zone::Right,
                direction: GestureDirection::Up,
                action: GestureActionConfig::Command {
                    argv: vec!["/tmp/edgepad-definitely-missing-command".to_string()],
                },
            }],
        };

        check_action_executables(&config, &mut report);

        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].status, CheckStatus::Fail);
        assert!(report.checks[0].detail.contains("not found"));
        assert!(report.checks[0].detail.contains("gestures[0] right.up"));
    }

    #[test]
    fn find_executable_in_path_finds_executable_file() {
        let root = unique_temp_dir("edgepad-doctor-path");
        let bin_dir = root.join("bin");
        let program = bin_dir.join("edgepad-test-tool");
        fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        fs::write(&program, "#!/bin/sh\n").expect("test executable should be written");
        let mut permissions = fs::metadata(&program)
            .expect("metadata should be available")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&program, permissions).expect("permissions should be updated");

        let found = find_executable_in_path(
            "edgepad-test-tool",
            Some(OsString::from(bin_dir.display().to_string())),
        );

        assert_eq!(found, Some(program));
        fs::remove_dir_all(root).expect("temp dir should be removed");
    }

    #[test]
    fn report_counts_failures_warnings_and_successes() {
        let report = DoctorReport {
            checks: vec![
                DoctorCheck {
                    status: CheckStatus::Ok,
                    name: "a",
                    detail: String::new(),
                },
                DoctorCheck {
                    status: CheckStatus::Warn,
                    name: "b",
                    detail: String::new(),
                },
                DoctorCheck {
                    status: CheckStatus::Fail,
                    name: "c",
                    detail: String::new(),
                },
            ],
        };

        assert!(report.has_failures());
        assert_eq!(
            report.counts(),
            DoctorCounts {
                ok: 1,
                warn: 1,
                fail: 1
            }
        );
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
    }
}

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
use crate::core::{EdgeWidths, GestureDirection, SliderDirection, Zone};
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
    pub section: DoctorSection,
    pub label: &'static str,
    pub detail: String,
}

impl DoctorCheck {
    fn new(
        status: CheckStatus,
        section: DoctorSection,
        label: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status,
            section,
            label,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorSection {
    Config,
    Actions,
    Device,
    Access,
    Session,
    Service,
}

impl DoctorSection {
    pub const ALL: [Self; 6] = [
        Self::Config,
        Self::Actions,
        Self::Device,
        Self::Access,
        Self::Session,
        Self::Service,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Config => "Config",
            Self::Actions => "Actions",
            Self::Device => "Device",
            Self::Access => "Access",
            Self::Session => "Session",
            Self::Service => "Service",
        }
    }
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
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Fail,
                    DoctorSection::Config,
                    "path",
                    err,
                ));
                return None;
            }
        },
    };

    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => report.checks.push(DoctorCheck::new(
            CheckStatus::Ok,
            DoctorSection::Config,
            "path",
            format!("{}", path.display()),
        )),
        Ok(_) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Config,
                "path",
                format!("not a file: {}", path.display()),
            ));
            return None;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Config,
                "path",
                format!(
                    "not found: {}; pass --config <file> or create ~/.config/edgepad/edgepad.toml",
                    path.display()
                ),
            ));
            return None;
        }
        Err(err) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Config,
                "path",
                format!("failed to inspect {}: {err}", path.display()),
            ));
            return None;
        }
    }

    match load_edgepad_config(&path) {
        Ok(edgepad_config) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Config,
                "file",
                format!(
                    "device {}, edge width {}, {}, {}",
                    device_config_value_label(&edgepad_config.device),
                    percent_label(edgepad_config.edge_width),
                    gesture_binding_count_label(edgepad_config.gestures.len()),
                    slider_count_label(edgepad_config.sliders.len())
                ),
            ));
            check_bindings(&edgepad_config, report);
            Some(edgepad_config)
        }
        Err(err) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Config,
                "file",
                err,
            ));
            None
        }
    }
}

fn check_bindings(config: &EdgepadConfig, report: &mut DoctorReport) {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        report.checks.push(DoctorCheck::new(
            CheckStatus::Fail,
            DoctorSection::Config,
            "bindings",
            "no bindings configured; add at least one [[gestures]] or [[sliders]] entry",
        ));
        return;
    }

    report.checks.push(DoctorCheck::new(
        CheckStatus::Ok,
        DoctorSection::Config,
        "bindings",
        format!(
            "{}, {} configured",
            gesture_binding_count_label(config.gestures.len()),
            slider_count_label(config.sliders.len())
        ),
    ));
    report.checks.push(DoctorCheck::new(
        CheckStatus::Ok,
        DoctorSection::Config,
        "zones",
        active_zones_detail(config),
    ));
}

fn active_zones_detail(config: &EdgepadConfig) -> String {
    let active_zones: Vec<Zone> = ordered_zones()
        .iter()
        .copied()
        .filter(|zone| {
            config.gestures.iter().any(|binding| binding.zone == *zone)
                || config.sliders.iter().any(|binding| binding.zone == *zone)
        })
        .collect();
    let inactive_zones: Vec<Zone> = ordered_zones()
        .iter()
        .copied()
        .filter(|zone| !active_zones.contains(zone))
        .collect();

    format!(
        "claiming {}; passthrough {}; widths: {}",
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
        .join(", ")
}

fn edge_widths_label(widths: EdgeWidths) -> String {
    format!(
        "left {}, right {}, top {}, bottom {}",
        edge_width_label(widths.left),
        edge_width_label(widths.right),
        edge_width_label(widths.top),
        edge_width_label(widths.bottom)
    )
}

fn edge_width_label(width: f32) -> String {
    if width <= f32::EPSILON {
        "off".to_string()
    } else {
        percent_label(width)
    }
}

fn percent_label(value: f32) -> String {
    format!("{:.1}%", value * 100.0)
}

fn gesture_binding_count_label(count: usize) -> String {
    match count {
        1 => "1 gesture binding".to_string(),
        _ => format!("{count} gesture bindings"),
    }
}

fn slider_count_label(count: usize) -> String {
    match count {
        1 => "1 slider".to_string(),
        _ => format!("{count} sliders"),
    }
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
                    "config device {} overridden by --device {}",
                    device_config_value_label(&config.device),
                    device_config_value_label(device)
                ),
                None => format!("using --device {}", device_config_value_label(device)),
            };
            report.checks.push(DoctorCheck::new(
                CheckStatus::Warn,
                DoctorSection::Config,
                "device",
                detail,
            ));
            device.clone()
        }
        (Some(config), None) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Config,
                "device",
                format!("using {}", device_config_value_label(&config.device)),
            ));
            config.device.clone()
        }
        (None, None) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Warn,
                DoctorSection::Config,
                "device",
                "using auto because config was not loaded",
            ));
            DeviceConfig::Auto
        }
    }
}

fn check_action_executables(config: &EdgepadConfig, report: &mut DoctorReport) {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        return;
    }

    let mut usages = BTreeMap::<String, Vec<String>>::new();
    for binding in &config.gestures {
        if let GestureActionConfig::Command { argv } = &binding.action {
            if let Some(program) = argv.first() {
                usages
                    .entry(program.clone())
                    .or_default()
                    .push(gesture_binding_label(binding.zone, binding.direction));
            }
        }
    }
    for slider in &config.sliders {
        let negative_label = slider_binding_label(
            slider.zone,
            match slider.axis {
                crate::core::SliderAxis::Vertical => SliderDirection::Up,
                crate::core::SliderAxis::Horizontal => SliderDirection::Left,
            },
        );
        if let Some(program) = slider.negative.argv.first() {
            usages
                .entry(program.clone())
                .or_default()
                .push(negative_label);
        }

        let positive_label = slider_binding_label(
            slider.zone,
            match slider.axis {
                crate::core::SliderAxis::Vertical => SliderDirection::Down,
                crate::core::SliderAxis::Horizontal => SliderDirection::Right,
            },
        );
        if let Some(program) = slider.positive.argv.first() {
            usages
                .entry(program.clone())
                .or_default()
                .push(positive_label);
        }
    }

    if usages.is_empty() {
        report.checks.push(DoctorCheck::new(
            CheckStatus::Ok,
            DoctorSection::Actions,
            "command",
            "no command actions configured",
        ));
        return;
    }

    let mut usages = usages
        .into_iter()
        .map(|(program, bindings)| (action_program_name(&program), program, bindings))
        .collect::<Vec<_>>();
    usages.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    for (_program_name, program, bindings) in usages {
        let usage = format_binding_usage(&bindings);
        match action_executable_status(&program) {
            ActionExecutableStatus::Found(path) => report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Actions,
                "command",
                action_ok_detail(&program, &usage, &path),
            )),
            ActionExecutableStatus::AbsolutePathExecutable => report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Actions,
                "command",
                action_ok_detail(&program, &usage, Path::new(&program)),
            )),
            ActionExecutableStatus::RelativePathExecutable => report.checks.push(DoctorCheck::new(
                CheckStatus::Warn,
                DoctorSection::Actions,
                "command",
                format!(
                    "{}: {usage}\npath: {program}\nwarning: relative executable paths may not work from a user service",
                    action_program_name(&program)
                ),
            )),
            ActionExecutableStatus::Missing(message) => report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Actions,
                "command",
                format!(
                    "{}: {usage}\nproblem: {message}",
                    action_program_name(&program)
                ),
            )),
        }
    }
}

fn action_ok_detail(program: &str, usage: &str, path: &Path) -> String {
    format!(
        "{}: {usage}\npath: {}",
        action_program_name(program),
        path.display()
    )
}

fn action_program_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .to_string()
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
        [] => "0 bindings".to_string(),
        [binding] => format!("1 binding ({binding})"),
        _ => format!("{} bindings ({})", bindings.len(), bindings.join(", ")),
    }
}

fn gesture_binding_label(zone: Zone, direction: GestureDirection) -> String {
    format!("{}.{}", zone_name(zone), direction_name(direction))
}

fn slider_binding_label(zone: Zone, direction: SliderDirection) -> String {
    format!("{}.{}", zone_name(zone), slider_direction_name(direction))
}

fn device_config_value_label(device: &DeviceConfig) -> String {
    match device {
        DeviceConfig::Auto => "auto".to_string(),
        DeviceConfig::Path(path) => format!("{}", path.display()),
    }
}

fn check_touchpad_selection(
    device: &DeviceConfig,
    input_root: &Path,
    report: &mut DoctorReport,
) -> Option<PathBuf> {
    match device {
        DeviceConfig::Path(path) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Warn,
                DoctorSection::Device,
                "touchpad",
                format!(
                    "skipped because explicit device was provided: {}",
                    path.display()
                ),
            ));
            Some(path.clone())
        }
        DeviceConfig::Auto => match discover_device_report(input_root) {
            Ok(discovery) if discovery.event_node_count == 0 => {
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Fail,
                    DoctorSection::Device,
                    "touchpad",
                    format!("no event devices found under {}", input_root.display()),
                ));
                None
            }
            Ok(discovery) if discovery.summaries.is_empty() => {
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Fail,
                    DoctorSection::Device,
                    "touchpad",
                    format!(
                        "{} event node(s) found under {}, but none were readable",
                        discovery.event_node_count,
                        input_root.display()
                    ),
                ));
                None
            }
            Ok(discovery) => {
                let candidates = touchpad_candidates(&discovery.summaries);
                match candidates.as_slice() {
                    [] => {
                        report.checks.push(DoctorCheck::new(
                            CheckStatus::Fail,
                            DoctorSection::Device,
                            "touchpad",
                            format!(
                                "no touchpad candidates among {} readable event device(s)",
                                discovery.summaries.len()
                            ),
                        ));
                        None
                    }
                    [candidate] => {
                        report.checks.push(DoctorCheck::new(
                            CheckStatus::Ok,
                            DoctorSection::Device,
                            "touchpad",
                            format_device_line(candidate),
                        ));
                        Some(candidate.path.clone())
                    }
                    _ => {
                        let devices = candidates
                            .iter()
                            .map(|candidate| candidate.path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        report.checks.push(DoctorCheck::new(
                            CheckStatus::Fail,
                            DoctorSection::Device,
                            "touchpad",
                            format!(
                                "multiple touchpad candidates found; pass --device explicitly: {devices}"
                            ),
                        ));
                        None
                    }
                }
            }
            Err(err) => {
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Fail,
                    DoctorSection::Device,
                    "touchpad",
                    format!("failed to list {}: {err}", input_root.display()),
                ));
                None
            }
        },
    }
}

fn check_touchpad_readable(path: Option<&Path>, report: &mut DoctorReport) -> bool {
    let Some(path) = path else {
        report.checks.push(DoctorCheck::new(
            CheckStatus::Fail,
            DoctorSection::Device,
            "input",
            "skipped because no touchpad event node is selected",
        ));
        return false;
    };

    match Device::open(path) {
        Ok(_) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Device,
                "input",
                format!("{} can be opened by current user", path.display()),
            ));
            true
        }
        Err(err) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Device,
                "input",
                format!("failed to open {}: {err}", path.display()),
            ));
            false
        }
    }
}

fn check_uinput(path: &Path, report: &mut DoctorReport) -> bool {
    match OpenOptions::new().read(true).write(true).open(path) {
        Ok(_) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Device,
                "uinput",
                format!("{} is readable and writable", path.display()),
            ));
            true
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Device,
                "uinput",
                format!(
                    "{} is missing; load the uinput kernel module",
                    path.display()
                ),
            ));
            false
        }
        Err(err) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Device,
                "uinput",
                format!("failed to open {} read/write: {err}", path.display()),
            ));
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
        report.checks.push(DoctorCheck::new(
            CheckStatus::Fail,
            DoctorSection::Access,
            "udev tags",
            "skipped because no touchpad event node is selected",
        ));
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
            report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Access,
                "udev tags",
                format!(
                    "{} and {} have current udev tags seat,uaccess",
                    touchpad_path.display(),
                    uinput_path.display()
                ),
            ));
        }
        (Ok(touchpad_tags), Ok(uinput_tags)) => {
            report.checks.push(DoctorCheck::new(
                access_model_gap_status(device_access_ok),
                DoctorSection::Access,
                "udev tags",
                format!(
                    "missing seat/uaccess current tags; touchpad={touchpad_tags:?} uinput={uinput_tags:?}{}",
                    access_model_gap_note(device_access_ok),
                ),
            ));
        }
        (Err(err), _) | (_, Err(err)) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Access,
                "udev tags",
                err,
            ));
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
        report.checks.push(DoctorCheck::new(
            CheckStatus::Fail,
            DoctorSection::Access,
            "acl",
            "skipped because no touchpad event node is selected",
        ));
        return;
    };

    let username = current_username();
    let Some(username) = username else {
        report.checks.push(DoctorCheck::new(
            CheckStatus::Warn,
            DoctorSection::Access,
            "acl",
            "could not determine current username for ACL inspection",
        ));
        return;
    };

    let touchpad_acl = getfacl_grants_user(touchpad_path, &username);
    let uinput_acl = getfacl_grants_user(uinput_path, &username);

    match (touchpad_acl, uinput_acl) {
        (Ok(true), Ok(true)) => report.checks.push(DoctorCheck::new(
            CheckStatus::Ok,
            DoctorSection::Access,
            "acl",
            format!(
                "user {username} has rw ACL on {} and {}",
                touchpad_path.display(),
                uinput_path.display()
            ),
        )),
        (Ok(touchpad_ok), Ok(uinput_ok)) => report.checks.push(DoctorCheck::new(
            access_model_gap_status(device_access_ok),
            DoctorSection::Access,
            "acl",
            format!(
                "missing rw ACL for user {username}; touchpad_acl={touchpad_ok} uinput_acl={uinput_ok}{}",
                access_model_gap_note(device_access_ok),
            ),
        )),
        (Err(err), _) | (_, Err(err)) => report.checks.push(DoctorCheck::new(
            CheckStatus::Warn,
            DoctorSection::Access,
            "acl",
            err,
        )),
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
            report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Session,
                "seat",
                "current session is active on a local seat",
            ));
        }
        Ok(output) if output.status_success => report.checks.push(DoctorCheck::new(
            access_model_gap_status(device_access_ok),
            DoctorSection::Session,
            "seat",
            format!(
                "current loginctl session is not active on a local seat{}",
                access_model_gap_note(device_access_ok)
            ),
        )),
        Ok(output) => report.checks.push(DoctorCheck::new(
            access_model_gap_status(device_access_ok),
            DoctorSection::Session,
            "seat",
            format!(
                "loginctl session-status failed: {}{}",
                output.stderr_or_stdout(),
                access_model_gap_note(device_access_ok)
            ),
        )),
        Err(err) => report.checks.push(DoctorCheck::new(
            access_model_gap_status(device_access_ok),
            DoctorSection::Session,
            "seat",
            format!("{err}{}", access_model_gap_note(device_access_ok)),
        )),
    }
}

fn access_model_gap_status(device_access_ok: bool) -> CheckStatus {
    if device_access_ok {
        CheckStatus::Warn
    } else {
        CheckStatus::Fail
    }
}

fn access_model_gap_note(device_access_ok: bool) -> &'static str {
    if device_access_ok {
        "; device access is currently functional through another access model"
    } else {
        ""
    }
}

fn check_systemd_user(report: &mut DoctorReport) -> bool {
    match command_output("systemctl", &["--user", "show-environment"]) {
        Ok(output) if output.status_success => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Session,
                "systemd",
                "systemctl --user is available",
            ));
            true
        }
        Ok(output) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Session,
                "systemd",
                format!("systemctl --user failed: {}", output.stderr_or_stdout()),
            ));
            false
        }
        Err(err) => {
            report.checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Session,
                "systemd",
                err,
            ));
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
        report.checks.push(DoctorCheck::new(
            CheckStatus::Warn,
            DoctorSection::Service,
            "unit",
            "skipped because systemctl --user is not available",
        ));
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
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Warn,
                    DoctorSection::Service,
                    "unit",
                    format!("{service_name} is not installed in the user manager"),
                ));
            } else if active_state == "active" {
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Ok,
                    DoctorSection::Service,
                    "unit",
                    format!("{service_name} is active ({sub_state})"),
                ));
            } else if active_state == "failed" {
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Fail,
                    DoctorSection::Service,
                    "unit",
                    format!("{service_name} is failed ({sub_state})"),
                ));
            } else {
                report.checks.push(DoctorCheck::new(
                    CheckStatus::Warn,
                    DoctorSection::Service,
                    "unit",
                    format!(
                        "{service_name} is loaded={load_state} active={active_state} sub={sub_state}"
                    ),
                ));
            }
        }
        Ok(output) => report.checks.push(DoctorCheck::new(
            CheckStatus::Warn,
            DoctorSection::Service,
            "unit",
            format!(
                "could not inspect {service_name}: {}",
                output.stderr_or_stdout()
            ),
        )),
        Err(err) => report.checks.push(DoctorCheck::new(
            CheckStatus::Warn,
            DoctorSection::Service,
            "unit",
            err,
        )),
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

fn slider_direction_name(direction: SliderDirection) -> &'static str {
    match direction {
        SliderDirection::Up => "up",
        SliderDirection::Down => "down",
        SliderDirection::Left => "left",
        SliderDirection::Right => "right",
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
    fn uses_static_tags_when_current_tags_are_absent() {
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
    fn access_model_gap_status_warns_when_device_access_is_functional() {
        assert_eq!(access_model_gap_status(true), CheckStatus::Warn);
        assert_eq!(access_model_gap_status(false), CheckStatus::Fail);
    }

    #[test]
    fn access_model_gap_note_explains_functional_non_uaccess_access() {
        assert!(access_model_gap_note(true).contains("another access model"));
        assert_eq!(access_model_gap_note(false), "");
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
            sliders: Vec::new(),
        };

        assert_eq!(
            active_zones_detail(&config),
            "claiming right, top; passthrough left, bottom; widths: left off, right 20.0%, top 20.0%, bottom off"
        );
    }

    #[test]
    fn config_device_uses_config_when_no_cli_override_is_present() {
        let mut report = DoctorReport::default();
        let config = EdgepadConfig {
            device: DeviceConfig::Path(PathBuf::from("/dev/input/event7")),
            edge_width: 0.10,
            gestures: Vec::new(),
            sliders: Vec::new(),
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
            sliders: Vec::new(),
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
            sliders: Vec::new(),
        };

        check_action_executables(&config, &mut report);

        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].status, CheckStatus::Fail);
        assert!(report.checks[0].detail.contains("not found"));
        assert!(report.checks[0].detail.contains("right.up"));
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
                DoctorCheck::new(CheckStatus::Ok, DoctorSection::Config, "a", ""),
                DoctorCheck::new(CheckStatus::Warn, DoctorSection::Config, "b", ""),
                DoctorCheck::new(CheckStatus::Fail, DoctorSection::Config, "c", ""),
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

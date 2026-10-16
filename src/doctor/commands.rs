use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::config::{EdgepadConfig, GestureActionConfig};
use crate::core::{GestureDirection, SliderAxis, SliderDirection, Zone};

use super::labels::{direction_name, slider_direction_name, zone_name};
use super::{CheckStatus, DoctorCheck, DoctorSection};

pub(super) fn check(config: &EdgepadConfig) -> Vec<DoctorCheck> {
    check_with_path(config, env::var_os("PATH").as_deref())
}

fn check_with_path(config: &EdgepadConfig, search_path: Option<&OsStr>) -> Vec<DoctorCheck> {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        return Vec::new();
    }
    let mut usages = command_usages(config)
        .into_iter()
        .map(|(program, bindings)| (action_program_name(&program), program, bindings))
        .collect::<Vec<_>>();
    usages.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    if usages.is_empty() {
        return vec![DoctorCheck::new(
            CheckStatus::Ok,
            DoctorSection::Actions,
            "command",
            "no command actions configured",
        )];
    }
    usages
        .into_iter()
        .map(|(_, program, bindings)| command_check(&program, &bindings, search_path))
        .collect()
}

fn command_check(program: &str, bindings: &[String], search_path: Option<&OsStr>) -> DoctorCheck {
    let usage = format_binding_usage(bindings);
    let (status, detail) = match action_executable_status(program, search_path) {
        ActionExecutableStatus::Found(path) => (
            CheckStatus::Ok,
            action_ok_detail(program, &usage, &path),
        ),
        ActionExecutableStatus::RelativePathExecutable => (
            CheckStatus::Warn,
            format!(
                "{}: {usage}\npath: {program}\nwarning: relative executable paths may not work from a user service",
                action_program_name(program)
            ),
        ),
        ActionExecutableStatus::Missing(message) => (
            CheckStatus::Fail,
            format!("{}: {usage}\nproblem: {message}", action_program_name(program)),
        ),
    };
    DoctorCheck::new(status, DoctorSection::Actions, "command", detail)
}

fn command_usages(config: &EdgepadConfig) -> BTreeMap<String, Vec<String>> {
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
        let directions = match slider.axis {
            SliderAxis::Vertical => [SliderDirection::Up, SliderDirection::Down],
            SliderAxis::Horizontal => [SliderDirection::Left, SliderDirection::Right],
        };
        for direction in directions {
            if let Some(program) = slider.action_for(direction).argv.first() {
                usages
                    .entry(program.clone())
                    .or_default()
                    .push(slider_binding_label(slider.zone, direction));
            }
        }
    }

    usages
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActionExecutableStatus {
    Found(PathBuf),
    RelativePathExecutable,
    Missing(String),
}

fn action_executable_status(program: &str, search_path: Option<&OsStr>) -> ActionExecutableStatus {
    if program.contains('/') {
        return path_executable_status(Path::new(program));
    }

    match find_executable_in_path(program, search_path) {
        Some(path) => ActionExecutableStatus::Found(path),
        None => ActionExecutableStatus::Missing(format!("{program} not found in PATH")),
    }
}

fn path_executable_status(path: &Path) -> ActionExecutableStatus {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {
            if path.is_absolute() {
                ActionExecutableStatus::Found(path.to_path_buf())
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

fn find_executable_in_path(program: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let path_env = path_env?;
    env::split_paths(path_env)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            fs::metadata(candidate)
                .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
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

#[cfg(test)]
mod tests;

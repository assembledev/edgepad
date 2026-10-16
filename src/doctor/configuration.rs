use std::fs;
use std::path::Path;

use crate::config::{
    default_edgepad_config_path, load_edgepad_config, DeviceConfig, EdgepadConfig,
};
use crate::core::{EdgeWidths, Zone};

use super::labels::{device_config_value_label, zone_name};
use super::{CheckStatus, DoctorCheck, DoctorSection};

pub(super) struct ConfigurationChecks {
    pub checks: Vec<DoctorCheck>,
    pub loaded: Option<EdgepadConfig>,
    pub device: DeviceConfig,
}

pub(super) fn check(
    config_path: Option<&Path>,
    device_override: Option<&DeviceConfig>,
) -> ConfigurationChecks {
    let mut checks = Vec::new();
    let loaded = load_config(config_path, &mut checks);
    let (device, device_check) = select_device(loaded.as_ref(), device_override);
    checks.push(device_check);
    ConfigurationChecks {
        checks,
        loaded,
        device,
    }
}

fn load_config(config_path: Option<&Path>, checks: &mut Vec<DoctorCheck>) -> Option<EdgepadConfig> {
    let path = match config_path {
        Some(path) => path.to_path_buf(),
        None => match default_edgepad_config_path() {
            Ok(path) => path,
            Err(err) => {
                checks.push(DoctorCheck::new(
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
        Ok(metadata) if metadata.is_file() => checks.push(DoctorCheck::new(
            CheckStatus::Ok,
            DoctorSection::Config,
            "path",
            format!("{}", path.display()),
        )),
        Ok(_) => {
            checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Config,
                "path",
                format!("not a file: {}", path.display()),
            ));
            return None;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            checks.push(DoctorCheck::new(
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
            checks.push(DoctorCheck::new(
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
            checks.push(DoctorCheck::new(
                CheckStatus::Ok,
                DoctorSection::Config,
                "file",
                format!(
                    "device {}, default edge width {}, tap min {}ms, swipe min {}, {}, {}",
                    device_config_value_label(&edgepad_config.device),
                    percent_label(edgepad_config.edge_width),
                    edgepad_config.tap_min_duration_ms,
                    percent_label(edgepad_config.swipe_min_distance),
                    gesture_binding_count_label(edgepad_config.gestures.len()),
                    slider_count_label(edgepad_config.sliders.len())
                ),
            ));
            check_bindings(&edgepad_config, checks);
            Some(edgepad_config)
        }
        Err(err) => {
            checks.push(DoctorCheck::new(
                CheckStatus::Fail,
                DoctorSection::Config,
                "file",
                err,
            ));
            None
        }
    }
}

fn check_bindings(config: &EdgepadConfig, checks: &mut Vec<DoctorCheck>) {
    if config.gestures.is_empty() && config.sliders.is_empty() {
        checks.push(DoctorCheck::new(
            CheckStatus::Fail,
            DoctorSection::Config,
            "bindings",
            "no bindings configured; add at least one [[gestures]] or [[sliders]] entry",
        ));
        return;
    }

    checks.push(DoctorCheck::new(
        CheckStatus::Ok,
        DoctorSection::Config,
        "bindings",
        format!(
            "{}, {} configured",
            gesture_binding_count_label(config.gestures.len()),
            slider_count_label(config.sliders.len())
        ),
    ));
    checks.push(DoctorCheck::new(
        CheckStatus::Ok,
        DoctorSection::Config,
        "zones",
        active_zones_detail(config),
    ));
}

fn select_device(
    loaded_config: Option<&EdgepadConfig>,
    device_override: Option<&DeviceConfig>,
) -> (DeviceConfig, DoctorCheck) {
    let (device, status, detail) = match (loaded_config, device_override) {
        (_, Some(device)) => {
            let detail = match loaded_config {
                Some(config) => format!(
                    "config device {} overridden by --device {}",
                    device_config_value_label(&config.device),
                    device_config_value_label(device)
                ),
                None => format!("using --device {}", device_config_value_label(device)),
            };
            (device.clone(), CheckStatus::Warn, detail)
        }
        (Some(config), None) => (
            config.device.clone(),
            CheckStatus::Ok,
            format!("using {}", device_config_value_label(&config.device)),
        ),
        (None, None) => (
            DeviceConfig::Auto,
            CheckStatus::Warn,
            "using auto because config was not loaded".to_string(),
        ),
    };
    (
        device,
        DoctorCheck::new(status, DoctorSection::Config, "device", detail),
    )
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

#[cfg(test)]
mod tests;

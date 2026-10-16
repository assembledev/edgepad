//! Parse device selectors independently from discovery. Auto-selection is deferred
//! until resolve, so config validation does not depend on hardware availability.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::device::{
    discover_device_report, format_device_line, touchpad_candidates, DiscoveryReport,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceConfig {
    Auto,
    Path(PathBuf),
}

impl DeviceConfig {
    pub fn parse(raw: &str) -> Result<Self, String> {
        raw.parse()
    }

    pub fn resolve(&self, input_root: &Path) -> Result<PathBuf, String> {
        match self {
            Self::Auto => resolve_auto_touchpad(input_root),
            Self::Path(path) => Ok(path.clone()),
        }
    }
}

impl FromStr for DeviceConfig {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let value = raw.trim();
        if value.is_empty() {
            return Err("device must be `auto` or an event node path".to_string());
        }
        if value == "auto" {
            Ok(Self::Auto)
        } else {
            Ok(Self::Path(PathBuf::from(value)))
        }
    }
}

fn resolve_auto_touchpad(input_root: &Path) -> Result<PathBuf, String> {
    let report = discover_device_report(input_root)
        .map_err(|err| format!("failed to list {}: {err}", input_root.display()))?;
    resolve_auto_touchpad_from_report(input_root, &report)
}

fn resolve_auto_touchpad_from_report(
    input_root: &Path,
    report: &DiscoveryReport,
) -> Result<PathBuf, String> {
    if report.event_node_count == 0 {
        return Err(format!(
            "device=auto found no event devices under {}",
            input_root.display()
        ));
    }

    if report.summaries.is_empty() {
        return Err(format!(
            "device=auto found no readable event devices under {} ({}; try sudo, group input, or seat/logind ACLs)",
            input_root.display(),
            event_node_count_text(report.event_node_count)
        ));
    }

    let candidates = touchpad_candidates(&report.summaries);
    match candidates.as_slice() {
        [] => Err(format!(
            "device=auto found no touchpad candidates under {} (readable non-touchpad devices: {})",
            input_root.display(),
            report.summaries.len()
        )),
        [candidate] => Ok(candidate.path.clone()),
        _ => {
            let mut message = format!(
                "device=auto matched multiple touchpad candidates under {}; pass --device <event-node> explicitly",
                input_root.display()
            );
            for candidate in candidates {
                message.push_str("\n  ");
                message.push_str(&format_device_line(candidate));
            }
            Err(message)
        }
    }
}

fn event_node_count_text(count: usize) -> String {
    if count == 1 {
        "1 event node was present but could not be opened".to_string()
    } else {
        format!("{count} event nodes were present but could not be opened")
    }
}

#[cfg(test)]
mod tests;

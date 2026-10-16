//! Parse device selectors independently from discovery. Auto-selection is deferred
//! until resolve, so config validation does not depend on hardware availability.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::device::{
    discover_device_report, format_device_line, touchpad_candidates, DeviceSummary, DiscoveryReport,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceConfig {
    Auto,
    Path(PathBuf),
}

/// Errors from automatic touchpad selection in [`DeviceConfig::resolve`].
#[derive(Debug)]
pub enum DeviceResolveError {
    Discovery {
        input_root: PathBuf,
        source: io::Error,
    },
    NoEventDevices {
        input_root: PathBuf,
    },
    NoReadableEventDevices {
        input_root: PathBuf,
        event_node_count: usize,
    },
    NoTouchpadCandidates {
        input_root: PathBuf,
        readable_count: usize,
    },
    AmbiguousTouchpads {
        input_root: PathBuf,
        candidates: Vec<DeviceSummary>,
    },
}

impl fmt::Display for DeviceResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery { input_root, source } =>
                write!(f, "failed to list {}: {source}", input_root.display()),
            Self::NoEventDevices { input_root } =>
                write!(f, "device=auto found no event devices under {}", input_root.display()),
            Self::NoReadableEventDevices { input_root, event_node_count } =>
                write!(f, "device=auto found no readable event devices under {} ({}; try sudo, group input, or seat/logind ACLs)",
                    input_root.display(), event_node_count_text(*event_node_count)),
            Self::NoTouchpadCandidates { input_root, readable_count } =>
                write!(f, "device=auto found no touchpad candidates under {} (readable non-touchpad devices: {readable_count})",
                    input_root.display()),
            Self::AmbiguousTouchpads { input_root, candidates } => {
                write!(f, "device=auto matched multiple touchpad candidates under {}; pass --device <event-node> explicitly",
                    input_root.display())?;
                for candidate in candidates {
                    write!(f, "\n  {}", format_device_line(candidate))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for DeviceResolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Discovery { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl DeviceConfig {
    pub fn parse(raw: &str) -> Result<Self, String> {
        raw.parse()
    }

    /// Resolve `auto` to the only readable touchpad under `input_root`.
    /// Explicit paths pass through without filesystem access.
    ///
    /// # Errors
    ///
    /// Automatic selection fails if discovery fails or finds zero or multiple
    /// readable touchpads.
    pub fn resolve(&self, input_root: &Path) -> Result<PathBuf, DeviceResolveError> {
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

fn resolve_auto_touchpad(input_root: &Path) -> Result<PathBuf, DeviceResolveError> {
    let report =
        discover_device_report(input_root).map_err(|source| DeviceResolveError::Discovery {
            input_root: input_root.to_path_buf(),
            source,
        })?;
    resolve_auto_touchpad_from_report(input_root, &report)
}

fn resolve_auto_touchpad_from_report(
    input_root: &Path,
    report: &DiscoveryReport,
) -> Result<PathBuf, DeviceResolveError> {
    if report.event_node_count == 0 {
        return Err(DeviceResolveError::NoEventDevices {
            input_root: input_root.to_path_buf(),
        });
    }
    if report.summaries.is_empty() {
        return Err(DeviceResolveError::NoReadableEventDevices {
            input_root: input_root.to_path_buf(),
            event_node_count: report.event_node_count,
        });
    }
    let candidates = touchpad_candidates(&report.summaries);
    match candidates.as_slice() {
        [] => Err(DeviceResolveError::NoTouchpadCandidates {
            input_root: input_root.to_path_buf(),
            readable_count: report.summaries.len(),
        }),
        [candidate] => Ok(candidate.path.clone()),
        _ => Err(DeviceResolveError::AmbiguousTouchpads {
            input_root: input_root.to_path_buf(),
            candidates: candidates.into_iter().cloned().collect(),
        }),
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

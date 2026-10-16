use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

/// Errors returned by [`run_proxy`](super::run_proxy).
#[derive(Debug)]
pub enum ProxyError {
    InvalidFrameLimit,
    OpenDevice {
        device_path: PathBuf,
        source: io::Error,
    },
    CapabilitiesUnavailable {
        device_path: PathBuf,
        /// `None` when the device lacks a required multitouch axis.
        source: Option<io::Error>,
    },
    CreateVirtualDevice {
        source: io::Error,
    },
    GrabDevice {
        device_path: PathBuf,
        source: io::Error,
    },
    ReadTouchState {
        source: io::Error,
    },
    TouchpadBusy {
        device_path: PathBuf,
    },
    ReadSlotState {
        field: &'static str,
        source: io::Error,
    },
    Readiness(String),
    Runtime(String),
    UngrabDevice {
        device_path: PathBuf,
        after: Option<&'static str>,
        source: io::Error,
    },
    /// [`Error::source`] returns `primary`; `cleanup` contains the ungrab error.
    WithCleanup {
        primary: Box<Self>,
        cleanup: Box<Self>,
    },
}

impl ProxyError {
    pub(super) fn with_cleanup(self, cleanup: Self) -> Self {
        Self::WithCleanup {
            primary: Box::new(self),
            cleanup: Box::new(cleanup),
        }
    }
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFrameLimit => f.write_str("proxy frame limit must be a positive integer"),
            Self::OpenDevice { device_path, source } =>
                write!(f, "failed to open device {}: {source}", device_path.display()),
            Self::CapabilitiesUnavailable { device_path, .. } =>
                write!(f, "failed to read touchpad capabilities from {}; proxy needs ABS_MT_SLOT, ABS_MT_POSITION_X, and ABS_MT_POSITION_Y", device_path.display()),
            Self::CreateVirtualDevice { source } =>
                write!(f, "failed to create virtual touchpad via /dev/uinput before grabbing physical device: {source}"),
            Self::GrabDevice { device_path, source } =>
                write!(f, "failed to grab device {}: {source}", device_path.display()),
            Self::ReadTouchState { source } =>
                write!(f, "failed to read current touch state before live proxy: {source}"),
            Self::TouchpadBusy { device_path } =>
                write!(f, "touchpad is already touched on {}; release all fingers and retry live proxy", device_path.display()),
            Self::ReadSlotState { field, source } =>
                write!(f, "failed to read current multitouch {field}: {source}"),
            Self::Readiness(message) | Self::Runtime(message) => f.write_str(message),
            Self::UngrabDevice { device_path, after, source } => {
                write!(f, "failed to ungrab device {}", device_path.display())?;
                if let Some(after) = after {
                    write!(f, " after {after}")?;
                }
                write!(f, ": {source}")
            }
            Self::WithCleanup { primary, cleanup } => write!(f, "{primary}; additionally {cleanup}"),
        }
    }
}

impl Error for ProxyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::OpenDevice { source, .. }
            | Self::CreateVirtualDevice { source }
            | Self::GrabDevice { source, .. }
            | Self::ReadTouchState { source }
            | Self::ReadSlotState { source, .. }
            | Self::UngrabDevice { source, .. } => Some(source),
            Self::CapabilitiesUnavailable { source, .. } => {
                source.as_ref().map(|source| source as &dyn Error)
            }
            Self::WithCleanup { primary, .. } => Some(primary.as_ref()),
            _ => None,
        }
    }
}

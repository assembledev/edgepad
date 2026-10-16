use super::super::*;
use std::io;

#[test]
fn discovery_availability_failures_retry_but_ambiguity_is_fatal() {
    let root = PathBuf::from("/input");
    let cases = [
        (
            DeviceResolveError::Discovery {
                input_root: root.clone(),
                source: io::Error::other("discovery failed"),
            },
            true,
        ),
        (
            DeviceResolveError::NoEventDevices {
                input_root: root.clone(),
            },
            true,
        ),
        (
            DeviceResolveError::NoReadableEventDevices {
                input_root: root.clone(),
                event_node_count: 2,
            },
            true,
        ),
        (
            DeviceResolveError::NoTouchpadCandidates {
                input_root: root.clone(),
                readable_count: 2,
            },
            true,
        ),
        (
            DeviceResolveError::AmbiguousTouchpads {
                input_root: root,
                candidates: Vec::new(),
            },
            false,
        ),
    ];
    for (error, expected_retry) in cases {
        assert_eq!(
            DaemonStartupError::from(error).should_retry(),
            expected_retry
        );
    }
}

#[test]
fn proxy_setup_retries_by_operation_independently_of_io_kind_or_message() {
    for kind in [
        io::ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput,
    ] {
        let path = PathBuf::from("/input/event7");
        let cases = [
            ProxyError::OpenDevice {
                device_path: path.clone(),
                source: io::Error::new(kind, "changed diagnostic"),
            },
            ProxyError::CapabilitiesUnavailable {
                device_path: path.clone(),
                source: None,
            },
            ProxyError::CapabilitiesUnavailable {
                device_path: path.clone(),
                source: Some(io::Error::new(kind, "axis query failed")),
            },
            ProxyError::CreateVirtualDevice {
                source: io::Error::new(kind, "changed diagnostic"),
            },
            ProxyError::GrabDevice {
                device_path: path,
                source: io::Error::new(kind, "changed diagnostic"),
            },
        ];
        for error in cases {
            assert!(DaemonStartupError::from(error).should_retry());
        }
    }
}

#[test]
fn invalid_or_unsafe_setup_and_runtime_failures_do_not_retry() {
    let cases = [
        ProxyError::InvalidFrameLimit,
        ProxyError::ReadTouchState {
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        },
        ProxyError::TouchpadBusy {
            device_path: PathBuf::from("/input/event7"),
        },
        ProxyError::ReadSlotState {
            field: "X positions",
            source: io::Error::from(io::ErrorKind::NotFound),
        },
        ProxyError::Readiness("failed to open device notification socket".to_string()),
        ProxyError::Runtime("failed to grab device during forwarding".to_string()),
        ProxyError::UngrabDevice {
            device_path: PathBuf::from("/input/event7"),
            after: None,
            source: io::Error::from(io::ErrorKind::NotFound),
        },
    ];
    for error in cases {
        assert!(!DaemonStartupError::from(error).should_retry());
    }
}

#[test]
fn cleanup_diagnostics_cannot_change_the_primary_retry_decision() {
    for (primary, expected_retry) in [
        (
            ProxyError::OpenDevice {
                device_path: PathBuf::from("/input/event7"),
                source: io::Error::other("open failed"),
            },
            true,
        ),
        (
            ProxyError::Readiness("failed to open device notification socket".to_string()),
            false,
        ),
    ] {
        let error = ProxyError::WithCleanup {
            primary: Box::new(primary),
            cleanup: Box::new(ProxyError::UngrabDevice {
                device_path: PathBuf::from("/input/event7"),
                after: None,
                source: io::Error::other("cleanup failed"),
            }),
        };
        assert_eq!(
            DaemonStartupError::from(error).should_retry(),
            expected_retry
        );
    }
}

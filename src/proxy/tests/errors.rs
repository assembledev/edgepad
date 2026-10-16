use super::super::*;
use std::error::Error;
use std::io;

fn missing_device_config(mode: ProxyMode) -> ProxyRunConfig {
    ProxyRunConfig {
        device_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/edgepad-missing-error-test-device/event0"),
        limit: ProxyRunLimit::Frames {
            frame_boundaries: 1,
            stop_after_limit: StopAfterFrameLimit::Immediately,
        },
        edge_widths: EdgeWidths::all(0.1),
        engine_options: EngineOptions::default(),
        slider_specs: Vec::new(),
        mode,
    }
}

#[test]
fn both_proxy_modes_preserve_open_failure_and_skip_readiness() {
    for mode in [ProxyMode::DryRun, ProxyMode::UinputGrab] {
        let config = missing_device_config(mode);
        let mut handler = NoopGestureHandler;
        let mut ready_called = false;
        let err = run_proxy_with_gesture_handler_and_ready(&config, &mut handler, &mut |_| {
            ready_called = true;
            Ok(())
        })
        .expect_err("missing device should fail before any live setup");
        assert!(!ready_called);
        assert!(
            matches!(&err, ProxyError::OpenDevice { device_path, .. } if device_path == &config.device_path)
        );
        let source = err
            .source()
            .and_then(|source| source.downcast_ref::<io::Error>())
            .expect("open failure should retain its io::Error");
        assert_eq!(source.kind(), io::ErrorKind::NotFound);
        assert_eq!(
            err.to_string(),
            format!(
                "failed to open device {}: {source}",
                config.device_path.display()
            )
        );
    }
}

#[test]
fn invalid_frame_limit_is_reported_before_opening_a_device() {
    let mut config = missing_device_config(ProxyMode::UinputGrab);
    config.limit = ProxyRunLimit::Frames {
        frame_boundaries: 0,
        stop_after_limit: StopAfterFrameLimit::Immediately,
    };
    assert!(matches!(
        run_proxy(&config),
        Err(ProxyError::InvalidFrameLimit)
    ));
}

#[test]
fn cleanup_preserves_primary_and_secondary_io_errors() {
    let primary = ProxyError::ReadSlotState {
        field: "X positions",
        source: io::Error::from_raw_os_error(libc::EIO),
    };
    let cleanup = ProxyError::UngrabDevice {
        device_path: PathBuf::from("/input/event3"),
        after: Some("slot-state read failure"),
        source: io::Error::from_raw_os_error(libc::ENODEV),
    };
    let expected = format!("{primary}; additionally {cleanup}");
    let err = combine_proxy_run_and_ungrab_result::<()>(Err(primary), Err(cleanup))
        .expect_err("both errors should survive cleanup");
    assert_eq!(err.to_string(), expected);
    let primary = err
        .source()
        .expect("primary failure should be the error-chain source");
    assert_eq!(
        primary
            .source()
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .raw_os_error(),
        Some(libc::EIO)
    );
    match err {
        ProxyError::WithCleanup { cleanup, .. } => assert_eq!(
            cleanup
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(libc::ENODEV)
        ),
        _ => panic!("cleanup failure should be attached to the primary"),
    }
}

#[test]
fn cleanup_failure_after_success_becomes_the_primary_failure() {
    let err = combine_proxy_run_and_ungrab_result(
        Ok(()),
        Err(ProxyError::UngrabDevice {
            device_path: PathBuf::from("/input/event3"),
            after: None,
            source: io::Error::other("cleanup failed"),
        }),
    )
    .expect_err("cleanup failure should not be swallowed");
    assert!(matches!(err, ProxyError::UngrabDevice { .. }));
}

#[test]
fn successful_cleanup_preserves_the_result() {
    assert_eq!(
        combine_proxy_run_and_ungrab_result(Ok(7), Ok(())).unwrap(),
        7
    );
    let err = combine_proxy_run_and_ungrab_result::<()>(
        Err(ProxyError::Readiness("notification failed".to_string())),
        Ok(()),
    )
    .expect_err("successful cleanup should retain the failure");
    assert!(matches!(err, ProxyError::Readiness(_)));
}

use super::*;
use crate::core::{Capabilities, EdgeWidths, EngineOptions, ResyncContact};
use crate::proxy::test_support::{input_event_triples, test_capabilities, RecordingUinputWriter};
use crate::proxy::{
    validate_run_limit, ProxyError, ProxyMode, ProxyRunConfig, ProxyRunLimit, ProxyRunSummary,
    ProxyRuntimeStats, StopAfterFrameLimit,
};
use crate::raw::{
    RawEvent, RecordingRawOutputSink, ABS_MT_SLOT, ABS_X, BTN_TOOL_DOUBLETAP, BTN_TOOL_FINGER,
    BTN_TOOL_QUADTAP, BTN_TOOL_QUINTTAP, BTN_TOOL_TRIPLETAP, BTN_TOUCH, EV_ABS, EV_KEY,
};
use std::path::PathBuf;
mod errors;

#[test]
fn proxy_settle_output_neutralizes_all_slots_and_touch_tools_before_ungrab() {
    let capabilities = Capabilities {
        slot_min: 0,
        slot_max: 2,
        ..test_capabilities()
    };
    let mut sink = RecordingRawOutputSink::default();
    let mut stats = ProxyRuntimeStats::default();

    emit_proxy_settle_output(capabilities, &mut sink, &mut stats)
        .expect("settle frame should emit");

    assert_eq!(stats.settle_output_frames, 1);
    assert_eq!(stats.settle_output_events, 12);
    assert_eq!(sink.frames().len(), 1);
    assert_eq!(
        sink.frames()[0].events,
        vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::abs_mt_slot(2),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
            RawEvent::btn_tool_doubletap(false),
            RawEvent::btn_tool_tripletap(false),
            RawEvent::btn_tool_quadtap(false),
            RawEvent::btn_tool_quinttap(false),
        ]
    );
}

#[test]
fn failed_uinput_proxy_run_discards_buffered_frame_before_settle() {
    let capabilities = Capabilities {
        slot_min: 0,
        slot_max: 1,
        ..test_capabilities()
    };
    let mut sink = UinputRawOutputSink::new(RecordingUinputWriter::default());
    sink.emit(RawEvent::abs_mt_tracking_id(777))
        .expect("test event should buffer");

    let result = settle_after_uinput_proxy_run(
        capabilities,
        &mut sink,
        Err("proxy loop failed".to_string()),
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("proxy loop failed")
    );
    let writer = sink.into_inner();
    assert_eq!(writer.batches.len(), 1);
    assert_eq!(
        input_event_triples(&writer.batches[0]),
        vec![
            (EV_ABS, ABS_MT_SLOT, 0),
            (EV_ABS, ABS_MT_TRACKING_ID, -1),
            (EV_ABS, ABS_MT_SLOT, 1),
            (EV_ABS, ABS_MT_TRACKING_ID, -1),
            (EV_KEY, BTN_TOUCH, 0),
            (EV_KEY, BTN_TOOL_FINGER, 0),
            (EV_KEY, BTN_TOOL_DOUBLETAP, 0),
            (EV_KEY, BTN_TOOL_TRIPLETAP, 0),
            (EV_KEY, BTN_TOOL_QUADTAP, 0),
            (EV_KEY, BTN_TOOL_QUINTTAP, 0),
        ]
    );
}

#[test]
fn successful_uinput_proxy_run_records_settle_output_in_stats() {
    let capabilities = Capabilities {
        slot_min: 0,
        slot_max: 1,
        ..test_capabilities()
    };
    let mut sink = UinputRawOutputSink::new(RecordingUinputWriter::default());

    let stats =
        settle_after_uinput_proxy_run(capabilities, &mut sink, Ok(ProxyRuntimeStats::default()))
            .expect("settle after successful proxy run should succeed");

    assert_eq!(stats.settle_output_frames, 1);
    assert_eq!(stats.settle_output_events, 10);
    assert_eq!(sink.into_inner().batches.len(), 1);
}

#[test]
fn post_grab_ungrab_error_is_reported_with_primary_failure() {
    let result = combine_proxy_run_and_ungrab_result::<ProxyRuntimeStats>(
        Err(ProxyError::Runtime("proxy loop failed".to_string())),
        Err(ProxyError::UngrabDevice {
            device_path: PathBuf::from("/dev/input/event5"),
            after: None,
            source: std::io::Error::other("EIO"),
        }),
    );
    let err = result.expect_err("both failures should be reported");
    assert_eq!(
        err.to_string(),
        "proxy loop failed; additionally failed to ungrab device /dev/input/event5: EIO"
    );
    assert!(matches!(err, ProxyError::WithCleanup { .. }));
}

#[test]
fn frames_limit_rejects_zero_boundaries() {
    assert!(matches!(
        validate_run_limit(&ProxyRunLimit::Frames {
            frame_boundaries: 0,
            stop_after_limit: StopAfterFrameLimit::Immediately,
        }),
        Err(ProxyError::InvalidFrameLimit)
    ));
}

#[test]
fn slot_snapshots_and_resync_include_optional_tool_types() {
    let capabilities = test_capabilities();
    let snapshots = slot_snapshots_from_values(
        capabilities,
        vec![20, 500],
        vec![300, 400],
        Some(vec![2, 0]),
    );
    assert_eq!(snapshots[0].tool_type, Some(2));
    let contacts = resync_contacts_from_slot_values(
        capabilities,
        &[701, -1],
        &[20, 500],
        &[300, 400],
        Some(&[2, 0]),
    );
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].tool_type, Some(2));
    let unsupported = slot_snapshots_from_values(capabilities, vec![20, 500], vec![300, 400], None);
    assert!(unsupported.iter().all(|slot| slot.tool_type.is_none()));
}

#[test]
fn resync_snapshot_keeps_only_active_slots_with_real_slot_numbers() {
    let contacts = resync_contacts_from_slot_values(
        Capabilities {
            slot_min: 2,
            slot_max: 4,
            ..test_capabilities()
        },
        &[-1, 501, -1],
        &[10, 20, 30],
        &[40, 50, 60],
        None,
    );

    assert_eq!(
        contacts,
        vec![ResyncContact {
            slot: 3,
            tracking_id: 501,
            x: 20,
            y: 50,
            tool_type: None,
        }]
    );
}

#[test]
fn proxy_summary_preserves_runtime_metadata() {
    let config = ProxyRunConfig {
        device_path: PathBuf::from("/dev/input/event7"),
        limit: ProxyRunLimit::Frames {
            frame_boundaries: 12,
            stop_after_limit: StopAfterFrameLimit::Immediately,
        },
        edge_widths: EdgeWidths::all(0.2),
        engine_options: EngineOptions::default(),
        slider_specs: Vec::new(),
        mode: ProxyMode::DryRun,
    };
    let summary = ProxyRunSummary {
        mode: config.mode,
        device_path: config.device_path.clone(),
        capabilities: test_capabilities(),
        edge_widths: config.edge_widths,
        requested_frame_boundaries: config.limit.requested_frame_boundaries(),
        stats: ProxyRuntimeStats::default(),
    };

    assert_eq!(summary.mode, ProxyMode::DryRun);
    assert_eq!(summary.device_path, config.device_path);
    assert_eq!(summary.edge_widths, EdgeWidths::all(0.2));
    assert_eq!(summary.requested_frame_boundaries, Some(12));
}

#[test]
fn proxy_settle_events_do_not_emit_legacy_abs_positions() {
    let capabilities = Capabilities {
        slot_min: 0,
        slot_max: 0,
        ..test_capabilities()
    };

    assert!(!proxy_settle_events(capabilities)
        .iter()
        .any(|event| event.kind == EV_ABS && event.code == ABS_X));
}

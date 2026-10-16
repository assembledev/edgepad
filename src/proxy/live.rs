use std::path::Path;
use std::time::{Duration, Instant, UNIX_EPOCH};

use evdev::{raw_stream::RawDevice, AbsoluteAxisCode, PropType};

use crate::core::{Capabilities, ResyncContact, SlotSnapshot};
use crate::device::{mt_slot_values, physical_touch_is_down, wait_for_raw_device_events};
use crate::dump::capabilities_from_raw_device;
use crate::raw::{
    is_pointer_button_code, RawEvent, RawFrame, RawOutputSink, RecordingRawOutputSink,
    ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TOOL_TYPE, ABS_MT_TRACKING_ID,
};
use crate::uinput::{
    build_virtual_touchpad, UinputEventWriter, UinputRawOutputSink, VirtualTouchpadSpec,
};

use super::session::{ProxyInputEvent, ProxySession, ResyncSnapshot, SessionConfig, SessionStep};
use super::{
    GestureHandler, ProxyError, ProxyMode, ProxyRunConfig, ProxyRunLimit, ProxyRunSummary,
    ProxyRuntimeStats,
};

const UINPUT_UNGRAB_SETTLE_DELAY: Duration = Duration::from_millis(30);

pub(super) fn proxy_dry_run<H>(
    config: &ProxyRunConfig,
    handler: &mut H,
) -> Result<ProxyRunSummary, ProxyError>
where
    H: GestureHandler,
{
    let (mut device, capabilities) = open_proxy_device(&config.device_path)?;
    let buttonpad = device.properties().contains(PropType::BUTTONPAD);
    let initial_slot_snapshots =
        read_slot_snapshots(&device, capabilities, config.engine_options.palm_rejection)?;
    let mut sink = RecordingRawOutputSink::default();
    let stats = run_proxy_loop(
        &mut device,
        SessionConfig {
            capabilities,
            edge_widths: config.edge_widths,
            engine_options: config.engine_options,
            slider_specs: config.slider_specs.clone(),
            initial_slot_snapshots,
            buttonpad,
        },
        &config.limit,
        &mut sink,
        handler,
    )
    .map_err(ProxyError::Runtime)?;

    Ok(ProxyRunSummary {
        mode: ProxyMode::DryRun,
        device_path: config.device_path.clone(),
        capabilities,
        edge_widths: config.edge_widths,
        requested_frame_boundaries: config.limit.requested_frame_boundaries(),
        stats,
    })
}

pub(super) fn proxy_uinput_grab<H, F>(
    config: &ProxyRunConfig,
    handler: &mut H,
    on_ready: &mut F,
) -> Result<ProxyRunSummary, ProxyError>
where
    H: GestureHandler,
    F: FnMut(&std::path::Path) -> Result<(), String>,
{
    let (mut device, capabilities) = open_proxy_device(&config.device_path)?;
    ensure_physical_touchpad_idle_at_start(&config.device_path, &device, capabilities)?;

    let buttonpad = device.properties().contains(PropType::BUTTONPAD);
    let spec = VirtualTouchpadSpec::from_raw_device(
        &device,
        capabilities,
        config.engine_options.palm_rejection,
    );
    // Establish an output device before taking input away from the compositor.
    let virtual_device = build_virtual_touchpad(&spec)
        .map_err(|source| ProxyError::CreateVirtualDevice { source })?;
    let mut sink = UinputRawOutputSink::new(virtual_device);

    device.grab().map_err(|source| ProxyError::GrabDevice {
        device_path: config.device_path.clone(),
        source,
    })?;
    let initial_slot_snapshots =
        match read_slot_snapshots(&device, capabilities, config.engine_options.palm_rejection) {
            Ok(positions) => positions,
            Err(err) => {
                return combine_proxy_run_and_ungrab_result(
                    Err(err),
                    ungrab_proxy_device(
                        &mut device,
                        &config.device_path,
                        Some("slot-state read failure"),
                    ),
                );
            }
        };
    if let Err(ready_err) = on_ready(&config.device_path) {
        return combine_proxy_run_and_ungrab_result(
            Err(ProxyError::Readiness(ready_err)),
            ungrab_proxy_device(&mut device, &config.device_path, Some("readiness failure")),
        );
    }
    let run_result = run_proxy_loop(
        &mut device,
        SessionConfig {
            capabilities,
            edge_widths: config.edge_widths,
            engine_options: config.engine_options,
            slider_specs: config.slider_specs.clone(),
            initial_slot_snapshots,
            buttonpad,
        },
        &config.limit,
        &mut sink,
        handler,
    );
    let settle_result = settle_after_uinput_proxy_run(capabilities, &mut sink, run_result)
        .map_err(ProxyError::Runtime);
    // Give the compositor time to consume neutral virtual state before the
    // physical device resumes delivery and the virtual device is dropped.
    std::thread::sleep(UINPUT_UNGRAB_SETTLE_DELAY);
    let ungrab_result = ungrab_proxy_device(&mut device, &config.device_path, None);
    let stats = combine_proxy_run_and_ungrab_result(settle_result, ungrab_result)?;

    Ok(ProxyRunSummary {
        mode: ProxyMode::UinputGrab,
        device_path: config.device_path.clone(),
        capabilities,
        edge_widths: config.edge_widths,
        requested_frame_boundaries: config.limit.requested_frame_boundaries(),
        stats,
    })
}

fn open_proxy_device(device_path: &Path) -> Result<(RawDevice, Capabilities), ProxyError> {
    let device = RawDevice::open(device_path).map_err(|source| ProxyError::OpenDevice {
        device_path: device_path.to_path_buf(),
        source,
    })?;
    let capabilities = capabilities_from_raw_device(&device)
        .map_err(|source| ProxyError::CapabilitiesUnavailable {
            device_path: device_path.to_path_buf(),
            source: Some(source),
        })?
        .ok_or_else(|| ProxyError::CapabilitiesUnavailable {
            device_path: device_path.to_path_buf(),
            source: None,
        })?;
    Ok((device, capabilities))
}

fn ensure_physical_touchpad_idle_at_start(
    device_path: &Path,
    device: &RawDevice,
    capabilities: Capabilities,
) -> Result<(), ProxyError> {
    if !physical_touch_is_down(device, Some(capabilities))
        .map_err(|source| ProxyError::ReadTouchState { source })?
    {
        return Ok(());
    }
    Err(ProxyError::TouchpadBusy {
        device_path: device_path.to_path_buf(),
    })
}

fn ungrab_proxy_device(
    device: &mut RawDevice,
    device_path: &Path,
    after: Option<&'static str>,
) -> Result<(), ProxyError> {
    device.ungrab().map_err(|source| ProxyError::UngrabDevice {
        device_path: device_path.to_path_buf(),
        after,
        source,
    })
}

fn read_resync_slot_snapshot(
    device: &RawDevice,
    capabilities: Capabilities,
    palm_rejection: bool,
) -> Result<(Vec<ResyncContact>, Vec<SlotSnapshot>), ProxyError> {
    let tracking_ids = read_slot_values(device, capabilities, ABS_MT_TRACKING_ID, "tracking IDs")?;
    let x_values = read_slot_values(device, capabilities, ABS_MT_POSITION_X, "X positions")?;
    let y_values = read_slot_values(device, capabilities, ABS_MT_POSITION_Y, "Y positions")?;
    let tool_types = read_slot_tool_types(device, capabilities, palm_rejection)?;
    let contacts = resync_contacts_from_slot_values(
        capabilities,
        &tracking_ids,
        &x_values,
        &y_values,
        tool_types.as_deref(),
    );
    let positions = slot_snapshots_from_values(capabilities, x_values, y_values, tool_types);
    Ok((contacts, positions))
}

fn read_slot_snapshots(
    device: &RawDevice,
    capabilities: Capabilities,
    palm_rejection: bool,
) -> Result<Vec<SlotSnapshot>, ProxyError> {
    let x_values = read_slot_values(device, capabilities, ABS_MT_POSITION_X, "X positions")?;
    let y_values = read_slot_values(device, capabilities, ABS_MT_POSITION_Y, "Y positions")?;
    let tool_types = read_slot_tool_types(device, capabilities, palm_rejection)?;
    Ok(slot_snapshots_from_values(
        capabilities,
        x_values,
        y_values,
        tool_types,
    ))
}

fn read_slot_tool_types(
    device: &RawDevice,
    capabilities: Capabilities,
    palm_rejection: bool,
) -> Result<Option<Vec<i32>>, ProxyError> {
    if !palm_rejection
        || !device
            .supported_absolute_axes()
            .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_TOOL_TYPE))
    {
        return Ok(None);
    }
    read_slot_values(device, capabilities, ABS_MT_TOOL_TYPE, "tool types").map(Some)
}

fn read_slot_values(
    device: &RawDevice,
    capabilities: Capabilities,
    axis_code: u16,
    field: &'static str,
) -> Result<Vec<i32>, ProxyError> {
    mt_slot_values(device, capabilities, axis_code)
        .map_err(|source| ProxyError::ReadSlotState { field, source })
}

fn slot_snapshots_from_values(
    capabilities: Capabilities,
    x_values: Vec<i32>,
    y_values: Vec<i32>,
    tool_types: Option<Vec<i32>>,
) -> Vec<SlotSnapshot> {
    x_values
        .into_iter()
        .zip(y_values)
        .enumerate()
        .map(|(index, (x, y))| SlotSnapshot {
            slot: capabilities.slot_min + index as i32,
            x,
            y,
            tool_type: tool_types.as_ref().map(|values| values[index]),
        })
        .collect()
}

fn read_pressed_physical_buttons(device: &RawDevice) -> Result<Vec<u16>, String> {
    device
        .get_key_state()
        .map(|keys| {
            keys.iter()
                .map(|key| key.0)
                .filter(|code| is_pointer_button_code(*code))
                .collect()
        })
        .map_err(|err| format!("failed to read current physical button state: {err}"))
}

fn resync_contacts_from_slot_values(
    capabilities: Capabilities,
    tracking_ids: &[i32],
    x_values: &[i32],
    y_values: &[i32],
    tool_types: Option<&[i32]>,
) -> Vec<ResyncContact> {
    tracking_ids
        .iter()
        .zip(x_values)
        .zip(y_values)
        .enumerate()
        .filter_map(|(index, ((tracking_id, x), y))| {
            (*tracking_id >= 0).then_some(ResyncContact {
                slot: capabilities.slot_min + index as i32,
                tracking_id: *tracking_id,
                x: *x,
                y: *y,
                tool_type: tool_types.map(|values| values[index]),
            })
        })
        .collect()
}

fn settle_after_uinput_proxy_run<W>(
    capabilities: Capabilities,
    sink: &mut UinputRawOutputSink<W>,
    run_result: Result<ProxyRuntimeStats, String>,
) -> Result<ProxyRuntimeStats, String>
where
    W: UinputEventWriter,
    W::Error: std::fmt::Debug,
{
    match run_result {
        Ok(mut stats) => {
            emit_proxy_settle_output(capabilities, sink, &mut stats)?;
            Ok(stats)
        }
        Err(err) => {
            sink.discard_buffered_events();
            let mut settle_stats = ProxyRuntimeStats::default();
            emit_proxy_settle_output(capabilities, sink, &mut settle_stats).map_err(
                |settle_err| {
                    append_additional_error(
                        err.clone(),
                        format!("failed to emit neutral settle frame before ungrab: {settle_err}"),
                    )
                },
            )?;
            Err(err)
        }
    }
}

fn combine_proxy_run_and_ungrab_result<T>(
    run_result: Result<T, ProxyError>,
    ungrab_result: Result<(), ProxyError>,
) -> Result<T, ProxyError> {
    match (run_result, ungrab_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(ungrab_err)) => Err(ungrab_err),
        (Err(err), Err(ungrab_err)) => Err(err.with_cleanup(ungrab_err)),
    }
}

fn append_additional_error(primary: String, additional: String) -> String {
    format!("{primary}; additionally {additional}")
}

fn run_proxy_loop<S, H>(
    device: &mut RawDevice,
    config: SessionConfig,
    limit: &ProxyRunLimit,
    sink: &mut S,
    handler: &mut H,
) -> Result<ProxyRuntimeStats, String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
    H: GestureHandler,
{
    let capabilities = config.capabilities;
    let palm_rejection = config.engine_options.palm_rejection;
    let mut session = ProxySession::new(config, limit)?;
    loop {
        let timeout = session.poll_timeout(Instant::now());
        let Some(events) = fetch_proxy_events(device, timeout)? else {
            if session.on_idle_poll(Instant::now(), sink, handler)? {
                return Ok(session.into_stats());
            }
            continue;
        };
        for event in events {
            let finished = match session.on_event(event, Instant::now(), sink, handler)? {
                SessionStep::Continue => false,
                SessionStep::Finished => true,
                SessionStep::Resync { timestamp } => {
                    let (contacts, slots) =
                        read_resync_slot_snapshot(device, capabilities, palm_rejection)
                            .map_err(|err| err.to_string())?;
                    let pressed_buttons = read_pressed_physical_buttons(device)?;
                    session.restore_snapshot(
                        ResyncSnapshot {
                            contacts,
                            slots,
                            pressed_buttons,
                        },
                        timestamp,
                        Instant::now(),
                        sink,
                        handler,
                    )?
                }
            };
            if finished {
                return Ok(session.into_stats());
            }
        }
    }
}

fn fetch_proxy_events(
    device: &mut RawDevice,
    timeout: Option<Duration>,
) -> Result<Option<Vec<ProxyInputEvent>>, String> {
    if let Some(timeout) = timeout {
        let ready = wait_for_raw_device_events(device, timeout)
            .map_err(|err| format!("failed to wait for proxy events: {err}"))?;
        if !ready {
            return Ok(None);
        }
    }

    let events = device
        .fetch_events()
        .map_err(|err| format!("failed to read events from proxy device: {err}"))?
        .map(|event| {
            let timestamp = event
                .timestamp()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "proxy input event timestamp predates the Unix epoch".to_string())?;
            Ok(ProxyInputEvent {
                raw: RawEvent::new(event.event_type().0, event.code(), event.value()),
                timestamp: Some(timestamp),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Some(events))
}

fn emit_proxy_settle_output<S>(
    capabilities: Capabilities,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
{
    let events = proxy_settle_events(capabilities);
    stats.settle_output_frames += 1;
    stats.settle_output_events += events.len();
    sink.write_frame(&RawFrame::new(events))
        .map_err(|err| format!("proxy output settle {err}"))?;
    Ok(())
}

fn proxy_settle_events(capabilities: Capabilities) -> Vec<RawEvent> {
    let mut events = Vec::new();
    for slot in capabilities.slot_min..=capabilities.slot_max {
        events.push(RawEvent::abs_mt_slot(slot));
        events.push(RawEvent::abs_mt_tracking_id(-1));
    }
    events.extend([
        RawEvent::btn_touch(false),
        RawEvent::btn_tool_finger(false),
        RawEvent::btn_tool_doubletap(false),
        RawEvent::btn_tool_tripletap(false),
        RawEvent::btn_tool_quadtap(false),
        RawEvent::btn_tool_quinttap(false),
    ]);
    events
}

#[cfg(test)]
mod tests;

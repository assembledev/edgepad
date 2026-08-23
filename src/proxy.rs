use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::core::{
    Capabilities, EdgeWidths, Engine, EngineOptions, Gesture, GestureDirection, ResyncContact,
    SliderDirection, SliderSpec, SliderStep, SlotPosition, Zone,
};
#[cfg(test)]
use crate::core::{Event, SliderAxis, ZoneSet};
use crate::device::{mt_slot_values, physical_touch_is_down, wait_for_raw_device_events};
use crate::dump::capabilities_from_raw_device;
use crate::raw::{
    extract_core_events, is_pointer_button_code, route_raw_frame, route_recognition_deadline,
    route_resync_contacts, RawEvent, RawFrame, RawOutputComposer, RawOutputSink,
    RecordingRawOutputSink, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_SLOT, ABS_MT_TRACKING_ID,
    BTN_TOUCH, EV_ABS, EV_KEY, EV_SYN, SYN_DROPPED, SYN_REPORT,
};
use crate::uinput::{
    build_virtual_touchpad, UinputEventWriter, UinputRawOutputSink, VirtualTouchpadSpec,
};
use evdev::{raw_stream::RawDevice, PropType};

pub use crate::config::DEFAULT_EDGE_WIDTH;

const UINPUT_UNGRAB_SETTLE_DELAY: Duration = Duration::from_millis(30);
const UINPUT_IDLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProxyInputEvent {
    raw: RawEvent,
    timestamp: Option<Duration>,
}

#[derive(Debug, Clone)]
struct ProxyLoopConfig {
    capabilities: Capabilities,
    edge_widths: EdgeWidths,
    engine_options: EngineOptions,
    slider_specs: Vec<SliderSpec>,
    initial_slot_positions: Vec<SlotPosition>,
    buttonpad: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyRecognitionConfig {
    pub edge_widths: EdgeWidths,
    pub engine_options: EngineOptions,
    pub slider_specs: Vec<SliderSpec>,
}

#[derive(Debug, Default)]
struct PendingRawFrame {
    events: Vec<RawEvent>,
    timestamp: Option<Duration>,
}

impl PendingRawFrame {
    fn push(&mut self, event: ProxyInputEvent) {
        self.events.push(event.raw);
        if let Some(timestamp) = event.timestamp {
            self.timestamp = Some(timestamp);
        }
    }

    fn take_frame(&mut self, frame_timestamp: Option<Duration>) -> Option<RawFrame> {
        if self.events.is_empty() {
            self.timestamp = None;
            return None;
        }

        let timestamp = frame_timestamp.or(self.timestamp);
        self.timestamp = None;
        Some(raw_frame_with_timestamp(
            std::mem::take(&mut self.events),
            timestamp,
        ))
    }

    fn discard(&mut self) {
        self.events.clear();
        self.timestamp = None;
    }

    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[derive(Debug, Default)]
struct RecognitionDeadline {
    input_timestamp: Option<Duration>,
    wall_deadline: Option<Instant>,
}

impl RecognitionDeadline {
    fn sync(&mut self, engine: &Engine, input_now: Option<Duration>, wall_now: Instant) {
        let Some(input_deadline) = engine.next_deadline() else {
            self.input_timestamp = None;
            self.wall_deadline = None;
            return;
        };
        let Some(input_now) = input_now else {
            return;
        };

        // Kernel event timestamps and Instant use different epochs. Only their
        // elapsed durations are comparable, so anchor each engine deadline to
        // the wall clock at the frame that scheduled it.
        self.input_timestamp = Some(input_deadline);
        self.wall_deadline = Some(wall_now + input_deadline.saturating_sub(input_now));
    }

    fn poll_timeout(&self, wall_now: Instant) -> Option<Duration> {
        self.wall_deadline
            .map(|deadline| deadline.saturating_duration_since(wall_now))
    }

    fn take_due(&mut self, wall_now: Instant) -> Option<Duration> {
        if self
            .wall_deadline
            .is_some_and(|deadline| deadline <= wall_now)
        {
            self.wall_deadline = None;
            return self.input_timestamp.take();
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResyncStreamAction {
    ProcessNormally,
    StartResync,
    CompleteResync,
    Ignore,
}

fn observe_resync_stream_event(
    resync_pending: &mut bool,
    pending: &mut PendingRawFrame,
    event: RawEvent,
) -> ResyncStreamAction {
    if *resync_pending {
        return match (event.kind, event.code) {
            (EV_SYN, SYN_REPORT) => {
                *resync_pending = false;
                ResyncStreamAction::CompleteResync
            }
            _ => ResyncStreamAction::Ignore,
        };
    }

    if (event.kind, event.code) == (EV_SYN, SYN_DROPPED) {
        pending.discard();
        *resync_pending = true;
        ResyncStreamAction::StartResync
    } else {
        ResyncStreamAction::ProcessNormally
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    DryRun,
    UinputGrab,
}

#[derive(Debug, Clone)]
pub struct ProxyRunConfig {
    pub device_path: PathBuf,
    pub limit: ProxyRunLimit,
    pub edge_widths: EdgeWidths,
    pub engine_options: EngineOptions,
    pub slider_specs: Vec<SliderSpec>,
    pub mode: ProxyMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyRunSummary {
    pub mode: ProxyMode,
    pub device_path: PathBuf,
    pub capabilities: Capabilities,
    pub edge_widths: EdgeWidths,
    pub requested_frame_boundaries: Option<usize>,
    pub stats: ProxyRuntimeStats,
}

#[derive(Debug, Clone)]
pub enum ProxyRunLimit {
    Frames {
        frame_boundaries: usize,
        stop_after_limit: StopAfterFrameLimit,
    },
    UntilStopped {
        stop: StopToken,
        idle_drain_timeout: Duration,
    },
}

impl ProxyRunLimit {
    fn requested_frame_boundaries(&self) -> Option<usize> {
        match self {
            Self::Frames {
                frame_boundaries, ..
            } => Some(*frame_boundaries),
            Self::UntilStopped { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAfterFrameLimit {
    Immediately,
    WhenIdle,
}

#[derive(Debug, Clone, Default)]
pub struct StopToken {
    stopped: Arc<AtomicBool>,
}

impl StopToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyRuntimeStats {
    pub input_frame_boundaries: usize,
    pub raw_frames: usize,
    pub raw_events: usize,
    pub recognizer_events: usize,
    pub recognizer_passthrough_events: usize,
    pub passthrough_frames: usize,
    pub claimed_edge_frames: usize,
    pub empty_output_frames: usize,
    pub composed_frames: usize,
    pub composed_events: usize,
    pub cleanup_output_frames: usize,
    pub cleanup_output_events: usize,
    pub settle_output_frames: usize,
    pub settle_output_events: usize,
    pub idle_drain_frame_boundaries: usize,
    pub idle_drain_timed_out: bool,
    pub recognition_reloads: usize,
    pub gestures: Vec<Gesture>,
    pub gesture_counts: BTreeMap<GestureCountKey, usize>,
    pub slider_steps: Vec<SliderStep>,
    pub slider_step_counts: BTreeMap<SliderStepCountKey, usize>,
    pub resync_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GestureCountKey {
    pub zone: Zone,
    pub direction: GestureDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SliderStepCountKey {
    pub zone: Zone,
    pub direction: SliderDirection,
}

pub trait GestureHandler {
    fn handle_gesture(&mut self, gesture: Gesture);

    fn handle_slider_step(&mut self, _step: SliderStep) {}

    fn take_recognition_config_reload(&mut self) -> Option<ProxyRecognitionConfig> {
        None
    }
}

#[derive(Debug, Default)]
pub struct NoopGestureHandler;

impl GestureHandler for NoopGestureHandler {
    fn handle_gesture(&mut self, _gesture: Gesture) {}
}

pub fn run_proxy(config: &ProxyRunConfig) -> Result<ProxyRunSummary, String> {
    let mut handler = NoopGestureHandler;
    run_proxy_with_gesture_handler(config, &mut handler)
}

pub fn run_proxy_with_gesture_handler<H>(
    config: &ProxyRunConfig,
    handler: &mut H,
) -> Result<ProxyRunSummary, String>
where
    H: GestureHandler,
{
    let mut on_ready = |_device_path: &std::path::Path| Ok(());
    run_proxy_with_gesture_handler_and_ready(config, handler, &mut on_ready)
}

pub fn run_proxy_with_gesture_handler_and_ready<H, F>(
    config: &ProxyRunConfig,
    handler: &mut H,
    on_ready: &mut F,
) -> Result<ProxyRunSummary, String>
where
    H: GestureHandler,
    F: FnMut(&std::path::Path) -> Result<(), String>,
{
    validate_run_limit(&config.limit)?;
    match config.mode {
        ProxyMode::DryRun => proxy_dry_run(config, handler),
        ProxyMode::UinputGrab => proxy_uinput_grab(config, handler, on_ready),
    }
}

fn validate_run_limit(limit: &ProxyRunLimit) -> Result<(), String> {
    match limit {
        ProxyRunLimit::Frames {
            frame_boundaries: 0,
            ..
        } => Err("proxy frame limit must be a positive integer".to_string()),
        _ => Ok(()),
    }
}

fn proxy_dry_run<H>(config: &ProxyRunConfig, handler: &mut H) -> Result<ProxyRunSummary, String>
where
    H: GestureHandler,
{
    let (mut device, capabilities) = open_proxy_device(&config.device_path)?;
    let buttonpad = device.properties().contains(PropType::BUTTONPAD);
    let initial_slot_positions = read_slot_positions(&device, capabilities)?;
    let mut sink = RecordingRawOutputSink::default();
    let stats = run_proxy_loop(
        &mut device,
        ProxyLoopConfig {
            capabilities,
            edge_widths: config.edge_widths,
            engine_options: config.engine_options,
            slider_specs: config.slider_specs.clone(),
            initial_slot_positions,
            buttonpad,
        },
        &config.limit,
        &mut sink,
        handler,
    )?;

    Ok(ProxyRunSummary {
        mode: ProxyMode::DryRun,
        device_path: config.device_path.clone(),
        capabilities,
        edge_widths: config.edge_widths,
        requested_frame_boundaries: config.limit.requested_frame_boundaries(),
        stats,
    })
}

fn proxy_uinput_grab<H, F>(
    config: &ProxyRunConfig,
    handler: &mut H,
    on_ready: &mut F,
) -> Result<ProxyRunSummary, String>
where
    H: GestureHandler,
    F: FnMut(&std::path::Path) -> Result<(), String>,
{
    let (mut device, capabilities) = open_proxy_device(&config.device_path)?;
    ensure_physical_touchpad_idle_at_start(&config.device_path, &device, capabilities)?;

    let buttonpad = device.properties().contains(PropType::BUTTONPAD);
    let spec = VirtualTouchpadSpec::from_raw_device(&device, capabilities);
    let virtual_device = build_virtual_touchpad(&spec).map_err(|err| {
        format!("failed to create virtual touchpad via /dev/uinput before grabbing physical device: {err}")
    })?;
    let mut sink = UinputRawOutputSink::new(virtual_device);

    device.grab().map_err(|err| {
        format!(
            "failed to grab device {}: {err}",
            config.device_path.display()
        )
    })?;
    let initial_slot_positions = match read_slot_positions(&device, capabilities) {
        Ok(positions) => positions,
        Err(err) => {
            let ungrab_result = device.ungrab().map_err(|ungrab_err| {
                format!(
                    "failed to ungrab device {} after slot-state read failure: {ungrab_err}",
                    config.device_path.display()
                )
            });
            return match ungrab_result {
                Ok(()) => Err(err),
                Err(ungrab_err) => Err(append_additional_error(err, ungrab_err)),
            };
        }
    };
    if let Err(ready_err) = on_ready(&config.device_path) {
        let ungrab_result = device.ungrab().map_err(|err| {
            format!(
                "failed to ungrab device {} after readiness failure: {err}",
                config.device_path.display()
            )
        });
        return match ungrab_result {
            Ok(()) => Err(ready_err),
            Err(ungrab_err) => Err(append_additional_error(ready_err, ungrab_err)),
        };
    }
    let run_result = run_proxy_loop(
        &mut device,
        ProxyLoopConfig {
            capabilities,
            edge_widths: config.edge_widths,
            engine_options: config.engine_options,
            slider_specs: config.slider_specs.clone(),
            initial_slot_positions,
            buttonpad,
        },
        &config.limit,
        &mut sink,
        handler,
    );
    let settle_result = settle_after_uinput_proxy_run(capabilities, &mut sink, run_result);
    std::thread::sleep(UINPUT_UNGRAB_SETTLE_DELAY);
    let ungrab_result = device.ungrab().map_err(|err| {
        format!(
            "failed to ungrab device {}: {err}",
            config.device_path.display()
        )
    });
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

fn open_proxy_device(device_path: &std::path::Path) -> Result<(RawDevice, Capabilities), String> {
    let device = RawDevice::open(device_path)
        .map_err(|err| format!("failed to open device {}: {err}", device_path.display()))?;
    let capabilities = capabilities_from_raw_device(&device).ok_or_else(|| {
        format!(
            "failed to read touchpad capabilities from {}; proxy needs ABS_MT_SLOT, ABS_MT_POSITION_X, and ABS_MT_POSITION_Y",
            device_path.display()
        )
    })?;
    Ok((device, capabilities))
}

fn ensure_physical_touchpad_idle_at_start(
    device_path: &std::path::Path,
    device: &RawDevice,
    capabilities: Capabilities,
) -> Result<(), String> {
    if !physical_touch_is_down(device, Some(capabilities))
        .map_err(|err| format!("failed to read current touch state before live proxy: {err}"))?
    {
        return Ok(());
    }

    Err(format!(
        "touchpad is already touched on {}; release all fingers and retry live proxy",
        device_path.display()
    ))
}

fn read_resync_slot_snapshot(
    device: &RawDevice,
    capabilities: Capabilities,
) -> Result<(Vec<ResyncContact>, Vec<SlotPosition>), String> {
    let tracking_ids = mt_slot_values(device, capabilities, ABS_MT_TRACKING_ID)
        .map_err(|err| format!("failed to read current multitouch tracking IDs: {err}"))?;
    let x_values = mt_slot_values(device, capabilities, ABS_MT_POSITION_X)
        .map_err(|err| format!("failed to read current multitouch X positions: {err}"))?;
    let y_values = mt_slot_values(device, capabilities, ABS_MT_POSITION_Y)
        .map_err(|err| format!("failed to read current multitouch Y positions: {err}"))?;
    let contacts =
        resync_contacts_from_slot_values(capabilities, &tracking_ids, &x_values, &y_values);
    let positions = slot_positions_from_values(capabilities, x_values, y_values);
    Ok((contacts, positions))
}

fn read_slot_positions(
    device: &RawDevice,
    capabilities: Capabilities,
) -> Result<Vec<SlotPosition>, String> {
    let x_values = mt_slot_values(device, capabilities, ABS_MT_POSITION_X)
        .map_err(|err| format!("failed to read current multitouch X positions: {err}"))?;
    let y_values = mt_slot_values(device, capabilities, ABS_MT_POSITION_Y)
        .map_err(|err| format!("failed to read current multitouch Y positions: {err}"))?;
    Ok(slot_positions_from_values(capabilities, x_values, y_values))
}

fn slot_positions_from_values(
    capabilities: Capabilities,
    x_values: Vec<i32>,
    y_values: Vec<i32>,
) -> Vec<SlotPosition> {
    x_values
        .into_iter()
        .zip(y_values)
        .enumerate()
        .map(|(index, (x, y))| SlotPosition {
            slot: capabilities.slot_min + index as i32,
            x,
            y,
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

fn combine_proxy_run_and_ungrab_result(
    run_result: Result<ProxyRuntimeStats, String>,
    ungrab_result: Result<(), String>,
) -> Result<ProxyRuntimeStats, String> {
    match (run_result, ungrab_result) {
        (Ok(stats), Ok(())) => Ok(stats),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(ungrab_err)) => Err(ungrab_err),
        (Err(err), Err(ungrab_err)) => Err(append_additional_error(err, ungrab_err)),
    }
}

fn append_additional_error(primary: String, additional: String) -> String {
    format!("{primary}; additionally {additional}")
}

fn run_proxy_loop<S, H>(
    device: &mut RawDevice,
    config: ProxyLoopConfig,
    limit: &ProxyRunLimit,
    sink: &mut S,
    handler: &mut H,
) -> Result<ProxyRuntimeStats, String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
    H: GestureHandler,
{
    let mut engine = Engine::with_options(
        config.capabilities,
        config.edge_widths,
        config.slider_specs.clone(),
        config.engine_options,
    );
    engine
        .seed_slot_positions(&config.initial_slot_positions)
        .map_err(|err| format!("failed to seed multitouch slot positions: {err:?}"))?;
    engine.set_buttonpad(config.buttonpad);
    let mut composer = RawOutputComposer::new(config.capabilities);
    let mut stats = ProxyRuntimeStats::default();
    let mut touch_state = PhysicalTouchState::new(config.capabilities);
    let mut stopper = ProxyLoopStopper::new(limit);
    let mut drain_deadline: Option<Instant> = None;
    let mut pending = PendingRawFrame::default();
    let mut resync_pending = false;
    let mut recognition_deadline = RecognitionDeadline::default();

    loop {
        let mut timeout = drain_deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .or_else(|| stopper.poll_timeout());
        if let Some(recognition_timeout) = recognition_deadline.poll_timeout(Instant::now()) {
            timeout = Some(
                timeout
                    .map(|current| current.min(recognition_timeout))
                    .unwrap_or(recognition_timeout),
            );
        }
        let Some(events) = fetch_proxy_events(device, timeout)? else {
            if let Some(input_deadline) = recognition_deadline.take_due(Instant::now()) {
                process_proxy_recognition_deadline(
                    input_deadline,
                    &mut engine,
                    &mut composer,
                    sink,
                    &mut stats,
                    handler,
                )?;
                recognition_deadline.sync(&engine, Some(input_deadline), Instant::now());
            }
            if drain_deadline.is_some_and(|deadline| deadline <= Instant::now()) {
                stats.idle_drain_timed_out = true;
                finish_proxy_output(&mut composer, sink, &mut stats)?;
                return Ok(stats);
            }
            if stopper
                .observe_idle_poll(touch_state.is_touch_down() || !engine.is_recognition_idle())
            {
                finish_proxy_output(&mut composer, sink, &mut stats)?;
                return Ok(stats);
            }
            if pending.is_empty() && !resync_pending {
                stats.recognition_reloads += apply_pending_recognition_reload(
                    &mut engine,
                    touch_state.is_touch_down(),
                    handler,
                ) as usize;
            }
            drain_deadline = sync_physical_drain_deadline(
                drain_deadline,
                &stopper,
                touch_state.is_touch_down(),
                Instant::now(),
            );
            continue;
        };

        for raw in events {
            let event = raw;
            let raw = event.raw;
            match observe_resync_stream_event(&mut resync_pending, &mut pending, raw) {
                ResyncStreamAction::Ignore => continue,
                ResyncStreamAction::StartResync => {
                    let dropped = raw_frame_with_timestamp(vec![raw], event.timestamp);
                    process_proxy_raw_frame(
                        &dropped,
                        &mut touch_state,
                        &mut engine,
                        &mut composer,
                        sink,
                        &mut stats,
                        handler,
                    )?;
                    recognition_deadline.sync(&engine, dropped.timestamp, Instant::now());
                    touch_state.mark_desynchronized();
                    stats.input_frame_boundaries += 1;
                    continue;
                }
                ResyncStreamAction::CompleteResync => {
                    let (contacts, slot_positions) =
                        read_resync_slot_snapshot(device, config.capabilities)?;
                    let pressed_physical_buttons = read_pressed_physical_buttons(device)?;
                    process_proxy_resync_contacts(
                        &contacts,
                        &pressed_physical_buttons,
                        &mut engine,
                        &mut composer,
                        sink,
                        &mut stats,
                        handler,
                    )?;
                    engine.seed_slot_positions(&slot_positions).map_err(|err| {
                        format!("failed to restore multitouch slot positions: {err:?}")
                    })?;
                    recognition_deadline.sync(&engine, event.timestamp, Instant::now());
                    touch_state.restore_contacts(&contacts);
                    stats.input_frame_boundaries += 1;
                    if stopper.observe_frame_boundary(
                        touch_state.is_touch_down() || !engine.is_recognition_idle(),
                    ) {
                        stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                        finish_proxy_output(&mut composer, sink, &mut stats)?;
                        return Ok(stats);
                    }
                    stats.recognition_reloads += apply_pending_recognition_reload(
                        &mut engine,
                        touch_state.is_touch_down(),
                        handler,
                    ) as usize;
                    drain_deadline = sync_physical_drain_deadline(
                        drain_deadline,
                        &stopper,
                        touch_state.is_touch_down(),
                        Instant::now(),
                    );
                    stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                    continue;
                }
                ResyncStreamAction::ProcessNormally => {}
            }
            match (raw.kind, raw.code) {
                (EV_SYN, SYN_REPORT) => {
                    if let Some(frame) = pending.take_frame(event.timestamp) {
                        process_proxy_raw_frame(
                            &frame,
                            &mut touch_state,
                            &mut engine,
                            &mut composer,
                            sink,
                            &mut stats,
                            handler,
                        )?;
                        recognition_deadline.sync(&engine, frame.timestamp, Instant::now());
                    }
                    stats.input_frame_boundaries += 1;
                    if stopper.observe_frame_boundary(
                        touch_state.is_touch_down() || !engine.is_recognition_idle(),
                    ) {
                        stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                        finish_proxy_output(&mut composer, sink, &mut stats)?;
                        return Ok(stats);
                    }
                    stats.recognition_reloads += apply_pending_recognition_reload(
                        &mut engine,
                        touch_state.is_touch_down(),
                        handler,
                    ) as usize;
                    drain_deadline = sync_physical_drain_deadline(
                        drain_deadline,
                        &stopper,
                        touch_state.is_touch_down(),
                        Instant::now(),
                    );
                    stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                }
                _ => pending.push(event),
            }
        }
    }
}

fn sync_physical_drain_deadline(
    current: Option<Instant>,
    stopper: &ProxyLoopStopper<'_>,
    physical_touch_down: bool,
    now: Instant,
) -> Option<Instant> {
    // This timeout is a safety valve for a contact that never reaches an idle
    // boundary. Pending tap arbitration is already bounded by the recognizer's
    // own deadline and must not be cut short by the physical-contact guard.
    if !stopper.is_draining() || !physical_touch_down {
        return None;
    }

    current.or_else(|| stopper.drain_timeout().map(|timeout| now + timeout))
}

fn apply_pending_recognition_reload<H>(
    engine: &mut Engine,
    physical_touch_down: bool,
    handler: &mut H,
) -> bool
where
    H: GestureHandler,
{
    if physical_touch_down || !engine.is_recognition_idle() {
        return false;
    }

    let Some(reloaded) = handler.take_recognition_config_reload() else {
        return false;
    };
    engine.reconfigure_recognition(
        reloaded.edge_widths,
        reloaded.slider_specs,
        reloaded.engine_options,
    );
    true
}

fn process_proxy_recognition_deadline<S, H>(
    input_deadline: Duration,
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
    handler: &mut H,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
    H: GestureHandler,
{
    let routed = route_recognition_deadline(engine, input_deadline);
    process_proxy_routed_frame(0, composer, sink, stats, routed, handler)
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

fn process_proxy_raw_frame<S, H>(
    frame: &RawFrame,
    touch_state: &mut PhysicalTouchState,
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
    handler: &mut H,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
    H: GestureHandler,
{
    touch_state.observe_frame(frame);
    process_proxy_frame_with_handler(engine, composer, sink, stats, frame, handler)
}

fn raw_frame_with_timestamp(events: Vec<RawEvent>, timestamp: Option<Duration>) -> RawFrame {
    match timestamp {
        Some(timestamp) => RawFrame::new_at(events, timestamp),
        None => RawFrame::new(events),
    }
}

#[cfg(test)]
fn process_proxy_frame<S>(
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
    frame: &RawFrame,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
{
    let mut handler = NoopGestureHandler;
    process_proxy_frame_with_handler(engine, composer, sink, stats, frame, &mut handler)
}

fn process_proxy_frame_with_handler<S, H>(
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
    frame: &RawFrame,
    handler: &mut H,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
    H: GestureHandler,
{
    stats.raw_frames += 1;
    stats.raw_events += frame.events.len();

    let recognizer_events = extract_core_events(frame).len();
    stats.recognizer_events += recognizer_events;

    let routed = route_raw_frame(engine, frame).map_err(|err| format!("proxy failed: {err:?}"))?;
    process_proxy_routed_frame(recognizer_events, composer, sink, stats, routed, handler)
}

fn process_proxy_resync_contacts<S, H>(
    contacts: &[ResyncContact],
    pressed_physical_buttons: &[u16],
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
    handler: &mut H,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
    H: GestureHandler,
{
    let mut routed = route_resync_contacts(engine, contacts)
        .map_err(|err| format!("proxy resync failed: {err:?}"))?;
    engine.restore_pressed_physical_buttons(pressed_physical_buttons);
    routed.physical_buttons = pressed_physical_buttons
        .iter()
        .copied()
        .map(|code| RawEvent::new(EV_KEY, code, 1))
        .collect();
    let recognizer_events = routed.passthrough.len();
    stats.recognizer_events += recognizer_events;
    process_proxy_routed_frame(recognizer_events, composer, sink, stats, routed, handler)
}

fn process_proxy_routed_frame<S, H>(
    recognizer_events: usize,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
    routed: crate::raw::RoutedRawFrame,
    handler: &mut H,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
    H: GestureHandler,
{
    let passthrough_events = routed.passthrough.len();
    stats.recognizer_passthrough_events += passthrough_events;
    if passthrough_events > 0 {
        stats.passthrough_frames += 1;
    }
    if recognizer_events > passthrough_events {
        stats.claimed_edge_frames += 1;
    }
    stats.resync_required |= routed.resync_required;
    for gesture in routed.gestures.iter().copied() {
        *stats
            .gesture_counts
            .entry(gesture_count_key(gesture))
            .or_default() += 1;
        stats.gestures.push(gesture);
        handler.handle_gesture(gesture);
    }
    for step in routed.slider_steps.iter().copied() {
        *stats
            .slider_step_counts
            .entry(slider_step_count_key(step))
            .or_default() += 1;
        stats.slider_steps.push(step);
        handler.handle_slider_step(step);
    }

    let output_frame = composer
        .compose_frame(&routed)
        .map_err(|err| format!("proxy output compose failed: {err:?}"))?;
    if output_frame.events.is_empty() {
        stats.empty_output_frames += 1;
        return Ok(());
    }

    stats.composed_frames += 1;
    stats.composed_events += output_frame.events.len();
    for event in output_frame.events {
        sink.emit(event)
            .map_err(|err| format!("proxy output emit failed: {err:?}"))?;
    }
    sink.sync()
        .map_err(|err| format!("proxy output sync failed: {err:?}"))?;

    Ok(())
}

fn finish_proxy_output<S>(
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
{
    let output_frame = composer
        .finish()
        .map_err(|err| format!("proxy output finish failed: {err:?}"))?;
    if output_frame.events.is_empty() {
        return Ok(());
    }

    let event_count = output_frame.events.len();
    stats.composed_frames += 1;
    stats.composed_events += event_count;
    stats.cleanup_output_frames += 1;
    stats.cleanup_output_events += event_count;
    for event in output_frame.events {
        sink.emit(event)
            .map_err(|err| format!("proxy output cleanup emit failed: {err:?}"))?;
    }
    sink.sync()
        .map_err(|err| format!("proxy output cleanup sync failed: {err:?}"))?;

    Ok(())
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
    for event in events {
        sink.emit(event)
            .map_err(|err| format!("proxy output settle emit failed: {err:?}"))?;
    }
    sink.sync()
        .map_err(|err| format!("proxy output settle sync failed: {err:?}"))?;
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

fn gesture_count_key(gesture: Gesture) -> GestureCountKey {
    GestureCountKey {
        zone: gesture.zone,
        direction: gesture.direction,
    }
}

fn slider_step_count_key(step: SliderStep) -> SliderStepCountKey {
    SliderStepCountKey {
        zone: step.zone,
        direction: step.direction,
    }
}

#[derive(Debug, Clone)]
struct ProxyLoopStopper<'a> {
    limit: &'a ProxyRunLimit,
    observed_frame_boundaries: usize,
    extra_frame_boundaries: usize,
    draining: bool,
}

impl<'a> ProxyLoopStopper<'a> {
    fn new(limit: &'a ProxyRunLimit) -> Self {
        Self {
            limit,
            observed_frame_boundaries: 0,
            extra_frame_boundaries: 0,
            draining: false,
        }
    }

    fn observe_frame_boundary(&mut self, recognition_busy: bool) -> bool {
        self.observed_frame_boundaries += 1;
        match self.limit {
            ProxyRunLimit::Frames {
                frame_boundaries,
                stop_after_limit,
            } => self.observe_frame_limit_boundary(
                *frame_boundaries,
                *stop_after_limit,
                recognition_busy,
            ),
            ProxyRunLimit::UntilStopped { stop, .. } => {
                self.observe_stop_token_boundary(stop, recognition_busy)
            }
        }
    }

    fn observe_idle_poll(&mut self, recognition_busy: bool) -> bool {
        if self.draining && !recognition_busy {
            return true;
        }
        match self.limit {
            ProxyRunLimit::UntilStopped { stop, .. } if stop.is_stopped() => {
                self.observe_requested_stop(recognition_busy)
            }
            _ => false,
        }
    }

    fn poll_timeout(&self) -> Option<Duration> {
        match self.limit {
            ProxyRunLimit::UntilStopped { .. } => Some(STOP_POLL_INTERVAL),
            ProxyRunLimit::Frames { .. } => None,
        }
    }

    fn drain_timeout(&self) -> Option<Duration> {
        match self.limit {
            ProxyRunLimit::Frames {
                stop_after_limit: StopAfterFrameLimit::WhenIdle,
                ..
            } => Some(UINPUT_IDLE_DRAIN_TIMEOUT),
            ProxyRunLimit::UntilStopped {
                idle_drain_timeout, ..
            } => Some(*idle_drain_timeout),
            _ => None,
        }
    }

    fn is_draining(&self) -> bool {
        self.draining
    }

    fn extra_frame_boundaries(&self) -> usize {
        self.extra_frame_boundaries
    }

    fn observe_frame_limit_boundary(
        &mut self,
        frame_boundaries: usize,
        stop_after_limit: StopAfterFrameLimit,
        recognition_busy: bool,
    ) -> bool {
        if self.observed_frame_boundaries < frame_boundaries {
            return false;
        }

        if self.observed_frame_boundaries == frame_boundaries {
            return match stop_after_limit {
                StopAfterFrameLimit::Immediately => true,
                StopAfterFrameLimit::WhenIdle if !recognition_busy => true,
                StopAfterFrameLimit::WhenIdle => {
                    self.draining = true;
                    false
                }
            };
        }

        self.extra_frame_boundaries += 1;
        match stop_after_limit {
            StopAfterFrameLimit::Immediately => true,
            StopAfterFrameLimit::WhenIdle => {
                self.draining = recognition_busy;
                !recognition_busy
            }
        }
    }

    fn observe_stop_token_boundary(&mut self, stop: &StopToken, recognition_busy: bool) -> bool {
        if self.draining {
            self.extra_frame_boundaries += 1;
            self.draining = recognition_busy;
            return !recognition_busy;
        }

        if stop.is_stopped() {
            return self.observe_requested_stop(recognition_busy);
        }

        false
    }

    fn observe_requested_stop(&mut self, recognition_busy: bool) -> bool {
        if recognition_busy {
            self.draining = true;
            false
        } else {
            true
        }
    }
}

#[derive(Debug, Clone)]
struct PhysicalTouchState {
    current_slot: i32,
    active_slots: Vec<bool>,
    btn_touch_down: bool,
    desynchronized: bool,
    capabilities: Capabilities,
}

impl PhysicalTouchState {
    fn new(capabilities: Capabilities) -> Self {
        let slot_count = (capabilities.slot_max - capabilities.slot_min + 1) as usize;
        Self {
            current_slot: capabilities.slot_min,
            active_slots: vec![false; slot_count],
            btn_touch_down: false,
            desynchronized: false,
            capabilities,
        }
    }

    fn observe_frame(&mut self, frame: &RawFrame) {
        for event in &frame.events {
            match (event.kind, event.code) {
                (EV_ABS, ABS_MT_SLOT) => {
                    if self.slot_index(event.value).is_some() {
                        self.current_slot = event.value;
                    }
                }
                (EV_ABS, ABS_MT_TRACKING_ID) if event.value >= 0 => {
                    if let Some(index) = self.slot_index(self.current_slot) {
                        self.active_slots[index] = true;
                    }
                }
                (EV_ABS, ABS_MT_TRACKING_ID) => {
                    if let Some(index) = self.slot_index(self.current_slot) {
                        self.active_slots[index] = false;
                    }
                }
                (EV_KEY, BTN_TOUCH) => self.btn_touch_down = event.value != 0,
                _ => {}
            }
        }
    }

    fn is_touch_down(&self) -> bool {
        self.desynchronized || self.btn_touch_down || self.active_slots.iter().any(|active| *active)
    }

    fn mark_desynchronized(&mut self) {
        self.active_slots.fill(false);
        self.btn_touch_down = false;
        self.desynchronized = true;
    }

    fn restore_contacts(&mut self, contacts: &[ResyncContact]) {
        self.current_slot = self.capabilities.slot_min;
        self.active_slots.fill(false);
        self.btn_touch_down = !contacts.is_empty();
        self.desynchronized = false;
        for contact in contacts {
            if let Some(index) = self.slot_index(contact.slot) {
                self.active_slots[index] = true;
                self.current_slot = contact.slot;
            }
        }
    }

    fn slot_index(&self, slot: i32) -> Option<usize> {
        (slot >= self.capabilities.slot_min && slot <= self.capabilities.slot_max)
            .then_some((slot - self.capabilities.slot_min) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AxisRange;
    use crate::raw::{
        ABS_X, BTN_LEFT, BTN_TOOL_DOUBLETAP, BTN_TOOL_FINGER, BTN_TOOL_QUADTAP, BTN_TOOL_QUINTTAP,
        BTN_TOOL_TRIPLETAP,
    };

    fn test_capabilities() -> Capabilities {
        Capabilities {
            slot_min: 0,
            slot_max: 9,
            x: AxisRange { min: 0, max: 1000 },
            y: AxisRange { min: 0, max: 700 },
        }
    }

    #[derive(Debug, Default)]
    struct ReloadingGestureHandler {
        pending: Option<ProxyRecognitionConfig>,
        reload_polls: usize,
    }

    impl GestureHandler for ReloadingGestureHandler {
        fn handle_gesture(&mut self, _gesture: Gesture) {}

        fn take_recognition_config_reload(&mut self) -> Option<ProxyRecognitionConfig> {
            self.reload_polls += 1;
            self.pending.take()
        }
    }

    #[test]
    fn recognition_reload_waits_for_idle_and_preserves_button_state() {
        let capabilities = test_capabilities();
        let config = ProxyLoopConfig {
            capabilities,
            edge_widths: EdgeWidths::all(0.10),
            engine_options: EngineOptions::default(),
            slider_specs: Vec::new(),
            initial_slot_positions: Vec::new(),
            buttonpad: true,
        };
        let mut engine = Engine::new(capabilities, config.edge_widths);
        engine
            .seed_slot_positions(&[SlotPosition {
                slot: 0,
                x: 20,
                y: 300,
            }])
            .expect("slot state should seed");
        engine.set_buttonpad(true);
        engine.update_physical_button(BTN_LEFT, true);
        let mut handler = ReloadingGestureHandler {
            pending: Some(ProxyRecognitionConfig {
                edge_widths: EdgeWidths::all(0.20),
                engine_options: EngineOptions {
                    tap_min_duration: Duration::from_millis(120),
                    swipe_min_distance: 0.05,
                    ..EngineOptions::default()
                },
                slider_specs: Vec::new(),
            }),
            reload_polls: 0,
        };

        assert!(!apply_pending_recognition_reload(
            &mut engine,
            true,
            &mut handler
        ));
        assert_eq!(handler.reload_polls, 0);
        assert!(handler.pending.is_some());

        assert!(apply_pending_recognition_reload(
            &mut engine,
            false,
            &mut handler
        ));
        assert_eq!(handler.reload_polls, 1);
        assert!(handler.pending.is_none());
        assert_eq!(engine.pressed_physical_buttons(), vec![BTN_LEFT]);

        engine.update_physical_button(BTN_LEFT, false);
        engine
            .process_frame_at(
                &[Event::slot(0), Event::tracking_id(9)],
                Duration::from_millis(1000),
            )
            .expect("reload should preserve retained Type-B axes");
        let released = engine
            .process_frame_at(
                &[Event::slot(0), Event::tracking_id(-1)],
                Duration::from_millis(1130),
            )
            .expect("retained-position contact should release");
        assert_eq!(released.gestures[0].zone, Zone::Left);
        assert_eq!(released.gestures[0].direction, GestureDirection::Tap);
    }

    #[test]
    fn recognition_deadline_wakes_and_releases_pending_single_tap_without_input() {
        let capabilities = test_capabilities();
        let options = EngineOptions {
            double_tap_zones: ZoneSet::from_zones([Zone::Left]),
            single_tap_zones: ZoneSet::from_zones([Zone::Left]),
            ..EngineOptions::default()
        };
        let mut engine =
            Engine::with_options(capabilities, EdgeWidths::all(0.10), Vec::new(), options);
        engine
            .process_frame_at(
                &[
                    Event::slot(0),
                    Event::tracking_id(1),
                    Event::x(20),
                    Event::y(300),
                ],
                Duration::from_millis(1000),
            )
            .expect("tap should start");
        let released = engine
            .process_frame_at(
                &[Event::slot(0), Event::tracking_id(-1)],
                Duration::from_millis(1060),
            )
            .expect("tap should release");
        assert!(released.gestures.is_empty());

        let wall_start = Instant::now();
        let mut timer = RecognitionDeadline::default();
        timer.sync(&engine, Some(Duration::from_millis(1060)), wall_start);
        assert_eq!(
            timer.poll_timeout(wall_start),
            Some(Duration::from_millis(300))
        );
        assert!(timer
            .take_due(wall_start + Duration::from_millis(299))
            .is_none());
        let input_deadline = timer
            .take_due(wall_start + Duration::from_millis(300))
            .expect("timer should wake at the engine deadline");
        let routed = route_recognition_deadline(&mut engine, input_deadline);
        assert_eq!(routed.gestures.len(), 1);
        assert_eq!(routed.gestures[0].direction, GestureDirection::Tap);
    }

    #[test]
    fn bounded_proxy_can_finish_on_recognition_deadline_without_an_extra_input_frame() {
        let capabilities = test_capabilities();
        let mut engine = Engine::with_options(
            capabilities,
            EdgeWidths::all(0.10),
            Vec::new(),
            EngineOptions {
                double_tap_timeout: Duration::from_millis(1500),
                double_tap_zones: ZoneSet::from_zones([Zone::Left]),
                single_tap_zones: ZoneSet::from_zones([Zone::Left]),
                ..EngineOptions::default()
            },
        );
        engine
            .process_frame_at(
                &[
                    Event::slot(0),
                    Event::tracking_id(1),
                    Event::x(20),
                    Event::y(300),
                ],
                Duration::from_millis(1000),
            )
            .expect("tap should start");
        engine
            .process_frame_at(
                &[Event::slot(0), Event::tracking_id(-1)],
                Duration::from_millis(1060),
            )
            .expect("tap should become pending");

        let limit = ProxyRunLimit::Frames {
            frame_boundaries: 1,
            stop_after_limit: StopAfterFrameLimit::WhenIdle,
        };
        let mut stopper = ProxyLoopStopper::new(&limit);
        let wall_start = Instant::now();
        let mut timer = RecognitionDeadline::default();
        timer.sync(&engine, Some(Duration::from_millis(1060)), wall_start);

        assert!(!stopper.observe_frame_boundary(true));
        assert!(stopper.is_draining());
        assert!(sync_physical_drain_deadline(None, &stopper, false, wall_start).is_none());
        assert_eq!(
            timer.poll_timeout(wall_start),
            Some(Duration::from_millis(1500))
        );

        let input_deadline = timer
            .take_due(wall_start + Duration::from_millis(1500))
            .expect("recognition deadline should remain armed beyond the safety drain interval");
        let routed = route_recognition_deadline(&mut engine, input_deadline);
        assert_eq!(routed.gestures.len(), 1);
        assert_eq!(routed.gestures[0].direction, GestureDirection::Tap);
        assert!(stopper.observe_idle_poll(false));
    }

    #[test]
    fn physical_contact_drain_still_has_a_bounded_safety_deadline() {
        let limit = ProxyRunLimit::Frames {
            frame_boundaries: 1,
            stop_after_limit: StopAfterFrameLimit::WhenIdle,
        };
        let mut stopper = ProxyLoopStopper::new(&limit);
        let now = Instant::now();

        assert!(!stopper.observe_frame_boundary(true));
        assert_eq!(
            sync_physical_drain_deadline(None, &stopper, true, now),
            Some(now + UINPUT_IDLE_DRAIN_TIMEOUT)
        );
    }

    #[test]
    fn recognition_reload_waits_for_pending_tap_deadline() {
        let capabilities = test_capabilities();
        let config = ProxyLoopConfig {
            capabilities,
            edge_widths: EdgeWidths::all(0.10),
            engine_options: EngineOptions::default(),
            slider_specs: Vec::new(),
            initial_slot_positions: Vec::new(),
            buttonpad: false,
        };
        let mut engine = Engine::with_options(
            capabilities,
            config.edge_widths,
            Vec::new(),
            EngineOptions {
                double_tap_zones: ZoneSet::from_zones([Zone::Left]),
                single_tap_zones: ZoneSet::from_zones([Zone::Left]),
                ..EngineOptions::default()
            },
        );
        engine
            .process_frame_at(
                &[
                    Event::slot(0),
                    Event::tracking_id(1),
                    Event::x(20),
                    Event::y(300),
                ],
                Duration::from_millis(1000),
            )
            .expect("tap should start");
        engine
            .process_frame_at(
                &[Event::slot(0), Event::tracking_id(-1)],
                Duration::from_millis(1060),
            )
            .expect("tap should release");
        let mut handler = ReloadingGestureHandler {
            pending: Some(ProxyRecognitionConfig {
                edge_widths: EdgeWidths::all(0.20),
                engine_options: EngineOptions::default(),
                slider_specs: Vec::new(),
            }),
            reload_polls: 0,
        };

        assert!(!apply_pending_recognition_reload(
            &mut engine,
            false,
            &mut handler
        ));
        assert_eq!(handler.reload_polls, 0);

        engine.advance_time(Duration::from_millis(1360));
        assert!(apply_pending_recognition_reload(
            &mut engine,
            false,
            &mut handler
        ));
        assert_eq!(handler.reload_polls, 1);
    }

    #[test]
    fn proxy_dry_run_frame_stats_match_raw_replay_output() {
        let capabilities = test_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyRuntimeStats::default();
        let frame = RawFrame::new(vec![
            RawEvent::btn_touch(true),
            RawEvent::abs_x(20),
            RawEvent::abs_y(300),
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(100),
            RawEvent::abs_mt_position_x(20),
            RawEvent::abs_mt_position_y(300),
            RawEvent::abs_mt_slot(1),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(520),
            RawEvent::abs_mt_position_y(320),
        ]);

        process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &frame)
            .expect("mixed frame should process");

        assert_eq!(stats.raw_frames, 1);
        assert_eq!(stats.raw_events, 11);
        assert_eq!(stats.recognizer_events, 8);
        assert_eq!(stats.recognizer_passthrough_events, 4);
        assert_eq!(stats.claimed_edge_frames, 1);
        assert_eq!(stats.passthrough_frames, 1);
        assert_eq!(stats.empty_output_frames, 0);
        assert_eq!(stats.composed_frames, 1);
        assert_eq!(stats.composed_events, 8);
        assert_eq!(stats.gestures.len(), 0);
        assert!(stats.gesture_counts.is_empty());
        assert!(!stats.resync_required);
    }

    #[test]
    fn proxy_finish_output_releases_active_passthrough_contact_before_exit() {
        let capabilities = test_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyRuntimeStats::default();
        let frame = RawFrame::new(vec![
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(520),
            RawEvent::abs_mt_position_y(320),
        ]);

        process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &frame)
            .expect("center frame should process");
        finish_proxy_output(&mut composer, &mut sink, &mut stats)
            .expect("finish should emit a synthetic release frame");

        assert_eq!(stats.composed_frames, 2);
        assert_eq!(stats.composed_events, 11);
        assert_eq!(stats.cleanup_output_frames, 1);
        assert_eq!(stats.cleanup_output_events, 3);
        assert_eq!(sink.frames().len(), 2);
        assert_eq!(
            sink.frames()[1].events,
            vec![
                RawEvent::abs_mt_tracking_id(-1),
                RawEvent::btn_touch(false),
                RawEvent::btn_tool_finger(false),
            ]
        );
    }

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

    #[derive(Debug, Default)]
    struct RecordingUinputWriter {
        batches: Vec<Vec<evdev::InputEvent>>,
    }

    #[derive(Debug, Default)]
    struct RecordingGestureHandler {
        gestures: Vec<Gesture>,
        slider_steps: Vec<SliderStep>,
    }

    impl GestureHandler for RecordingGestureHandler {
        fn handle_gesture(&mut self, gesture: Gesture) {
            self.gestures.push(gesture);
        }

        fn handle_slider_step(&mut self, step: SliderStep) {
            self.slider_steps.push(step);
        }
    }

    #[test]
    fn resync_restores_held_buttonpad_button_before_new_edge_contacts() {
        let capabilities = test_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        engine.set_buttonpad(true);
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyRuntimeStats::default();
        let mut handler = RecordingGestureHandler::default();

        process_proxy_resync_contacts(
            &[],
            &[BTN_LEFT],
            &mut engine,
            &mut composer,
            &mut sink,
            &mut stats,
            &mut handler,
        )
        .expect("resync should restore held physical button");

        process_proxy_frame_with_handler(
            &mut engine,
            &mut composer,
            &mut sink,
            &mut stats,
            &RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(100),
                RawEvent::abs_mt_position_x(20),
                RawEvent::abs_mt_position_y(300),
            ]),
            &mut handler,
        )
        .expect("new edge contact while restored button is held should process");

        assert_eq!(sink.frames().len(), 2);
        assert_eq!(
            sink.frames()[0].events,
            vec![RawEvent::new(EV_KEY, BTN_LEFT, 1)]
        );
        assert_eq!(
            sink.frames()[1].events,
            vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(100),
                RawEvent::abs_mt_position_x(20),
                RawEvent::abs_mt_position_y(300),
                RawEvent::btn_touch(true),
                RawEvent::btn_tool_finger(true),
                RawEvent::abs_x(20),
                RawEvent::abs_y(300),
            ]
        );
        assert!(handler.gestures.is_empty());
    }

    impl UinputEventWriter for RecordingUinputWriter {
        type Error = std::convert::Infallible;

        fn emit_events(&mut self, events: &[evdev::InputEvent]) -> Result<(), Self::Error> {
            self.batches.push(events.to_vec());
            Ok(())
        }
    }

    fn input_event_triples(events: &[evdev::InputEvent]) -> Vec<(u16, u16, i32)> {
        events
            .iter()
            .map(|event| (event.event_type().0, event.code(), event.value()))
            .collect()
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

        let stats = settle_after_uinput_proxy_run(
            capabilities,
            &mut sink,
            Ok(ProxyRuntimeStats::default()),
        )
        .expect("settle after successful proxy run should succeed");

        assert_eq!(stats.settle_output_frames, 1);
        assert_eq!(stats.settle_output_events, 10);
        assert_eq!(sink.into_inner().batches.len(), 1);
    }

    #[test]
    fn post_grab_ungrab_error_is_reported_with_primary_failure() {
        let result = combine_proxy_run_and_ungrab_result(
            Err("proxy loop failed".to_string()),
            Err("failed to ungrab device /dev/input/event5: EIO".to_string()),
        );

        assert_eq!(
            result.as_ref().err().map(String::as_str),
            Some("proxy loop failed; additionally failed to ungrab device /dev/input/event5: EIO")
        );
    }

    #[test]
    fn physical_touch_state_tracks_touch_lifecycle_from_raw_frames() {
        let capabilities = test_capabilities();
        let mut touch_state = PhysicalTouchState::new(capabilities);

        touch_state.observe_frame(&RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(10),
            RawEvent::btn_touch(true),
        ]));
        assert!(touch_state.is_touch_down());

        touch_state.observe_frame(&RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
        ]));
        assert!(!touch_state.is_touch_down());
    }

    #[test]
    fn frame_limit_stop_waits_for_idle_after_budget_when_configured() {
        let limit = ProxyRunLimit::Frames {
            frame_boundaries: 2,
            stop_after_limit: StopAfterFrameLimit::WhenIdle,
        };
        let mut stopper = ProxyLoopStopper::new(&limit);

        assert!(!stopper.observe_frame_boundary(true));
        assert!(!stopper.observe_frame_boundary(true));
        assert_eq!(stopper.extra_frame_boundaries(), 0);
        assert!(!stopper.observe_frame_boundary(true));
        assert_eq!(stopper.extra_frame_boundaries(), 1);
        assert!(stopper.observe_frame_boundary(false));
        assert_eq!(stopper.extra_frame_boundaries(), 2);
    }

    #[test]
    fn frame_limit_stop_keeps_exact_budget_for_dry_run() {
        let limit = ProxyRunLimit::Frames {
            frame_boundaries: 2,
            stop_after_limit: StopAfterFrameLimit::Immediately,
        };
        let mut stopper = ProxyLoopStopper::new(&limit);

        assert!(!stopper.observe_frame_boundary(true));
        assert!(stopper.observe_frame_boundary(true));
        assert_eq!(stopper.extra_frame_boundaries(), 0);
    }

    #[test]
    fn until_stopped_finishes_at_next_idle_boundary_after_stop_token() {
        let stop = StopToken::new();
        let limit = ProxyRunLimit::UntilStopped {
            stop: stop.clone(),
            idle_drain_timeout: Duration::from_millis(250),
        };
        let mut stopper = ProxyLoopStopper::new(&limit);

        assert!(!stopper.observe_frame_boundary(false));
        stop.stop();

        assert!(stopper.observe_frame_boundary(false));
        assert_eq!(stopper.extra_frame_boundaries(), 0);
    }

    #[test]
    fn until_stopped_drains_after_stop_token_when_touch_is_active() {
        let stop = StopToken::new();
        let limit = ProxyRunLimit::UntilStopped {
            stop: stop.clone(),
            idle_drain_timeout: Duration::from_millis(250),
        };
        let mut stopper = ProxyLoopStopper::new(&limit);

        assert!(!stopper.observe_frame_boundary(true));
        stop.stop();
        assert!(!stopper.observe_frame_boundary(true));
        assert!(stopper.is_draining());
        assert_eq!(stopper.drain_timeout(), Some(Duration::from_millis(250)));
        assert!(!stopper.observe_frame_boundary(true));
        assert_eq!(stopper.extra_frame_boundaries(), 1);

        assert!(stopper.observe_frame_boundary(false));
        assert_eq!(stopper.extra_frame_boundaries(), 2);
    }

    #[test]
    fn until_stopped_idle_poll_wakes_stopped_idle_loop() {
        let stop = StopToken::new();
        let limit = ProxyRunLimit::UntilStopped {
            stop: stop.clone(),
            idle_drain_timeout: Duration::from_millis(250),
        };
        let mut stopper = ProxyLoopStopper::new(&limit);

        assert_eq!(stopper.poll_timeout(), Some(STOP_POLL_INTERVAL));
        assert!(!stopper.observe_idle_poll(false));
        stop.stop();

        assert!(stopper.observe_idle_poll(false));
    }

    #[test]
    fn until_stopped_finishes_after_synthetic_frame_sequence_reaches_idle() {
        let capabilities = test_capabilities();
        let stop = StopToken::new();
        let limit = ProxyRunLimit::UntilStopped {
            stop: stop.clone(),
            idle_drain_timeout: Duration::from_millis(250),
        };
        let mut stopper = ProxyLoopStopper::new(&limit);
        let mut touch_state = PhysicalTouchState::new(capabilities);
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyRuntimeStats::default();

        let start = RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(200),
            RawEvent::abs_mt_position_x(520),
            RawEvent::abs_mt_position_y(320),
        ]);
        touch_state.observe_frame(&start);
        process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &start)
            .expect("center start should process");
        assert!(!stopper.observe_frame_boundary(touch_state.is_touch_down()));

        stop.stop();
        let release = RawFrame::new(vec![
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
        ]);
        touch_state.observe_frame(&release);
        process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, &release)
            .expect("center release should process");
        assert!(stopper.observe_frame_boundary(touch_state.is_touch_down()));
        finish_proxy_output(&mut composer, &mut sink, &mut stats)
            .expect("finished idle composer should not fail");

        assert_eq!(stats.cleanup_output_frames, 0);
        assert_eq!(sink.frames().len(), 2);
    }

    #[test]
    fn frames_limit_rejects_zero_boundaries() {
        assert_eq!(
            validate_run_limit(&ProxyRunLimit::Frames {
                frame_boundaries: 0,
                stop_after_limit: StopAfterFrameLimit::Immediately,
            }),
            Err("proxy frame limit must be a positive integer".to_string())
        );
    }

    #[test]
    fn syn_dropped_discards_partial_frame_and_ignores_events_until_syn_report() {
        let mut pending = PendingRawFrame::default();
        pending.push(ProxyInputEvent {
            raw: RawEvent::abs_mt_position_x(500),
            timestamp: None,
        });
        let mut resync_pending = false;

        assert_eq!(
            observe_resync_stream_event(&mut resync_pending, &mut pending, RawEvent::syn_dropped(),),
            ResyncStreamAction::StartResync
        );
        assert!(pending.events.is_empty());
        assert!(resync_pending);
        assert_eq!(
            observe_resync_stream_event(
                &mut resync_pending,
                &mut pending,
                RawEvent::abs_mt_tracking_id(77),
            ),
            ResyncStreamAction::Ignore
        );
        assert_eq!(
            observe_resync_stream_event(&mut resync_pending, &mut pending, RawEvent::syn_report(),),
            ResyncStreamAction::CompleteResync
        );
        assert!(!resync_pending);
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
        );

        assert_eq!(
            contacts,
            vec![ResyncContact {
                slot: 3,
                tracking_id: 501,
                x: 20,
                y: 50,
            }]
        );
    }

    #[test]
    fn physical_touch_state_stays_busy_until_resync_snapshot_arrives() {
        let mut state = PhysicalTouchState::new(test_capabilities());
        state.mark_desynchronized();
        assert!(state.is_touch_down());

        state.restore_contacts(&[]);
        assert!(!state.is_touch_down());
    }

    #[test]
    fn proxy_dry_run_stats_count_gestures_by_zone_and_direction() {
        let capabilities = test_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyRuntimeStats::default();
        let frames = [
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(300),
                RawEvent::abs_mt_position_x(980),
                RawEvent::abs_mt_position_y(400),
            ]),
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_position_x(980),
                RawEvent::abs_mt_position_y(620),
            ]),
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(-1),
            ]),
        ];

        for frame in &frames {
            process_proxy_frame(&mut engine, &mut composer, &mut sink, &mut stats, frame)
                .expect("edge frames should process");
        }

        assert_eq!(stats.raw_frames, 3);
        assert_eq!(stats.claimed_edge_frames, 3);
        assert_eq!(stats.passthrough_frames, 0);
        assert_eq!(stats.empty_output_frames, 3);
        assert_eq!(stats.composed_frames, 0);
        assert_eq!(stats.gestures.len(), 1);
        assert_eq!(
            stats.gesture_counts.get(&GestureCountKey {
                zone: Zone::Right,
                direction: GestureDirection::Down,
            }),
            Some(&1)
        );
    }

    #[test]
    fn proxy_handler_receives_recognized_gestures_live() {
        let capabilities = test_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyRuntimeStats::default();
        let mut handler = RecordingGestureHandler::default();
        let frames = [
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(300),
                RawEvent::abs_mt_position_x(980),
                RawEvent::abs_mt_position_y(400),
            ]),
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_position_x(980),
                RawEvent::abs_mt_position_y(620),
            ]),
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(-1),
            ]),
        ];

        for frame in &frames {
            process_proxy_frame_with_handler(
                &mut engine,
                &mut composer,
                &mut sink,
                &mut stats,
                frame,
                &mut handler,
            )
            .expect("edge frames should process");
        }

        assert_eq!(handler.gestures, stats.gestures);
        assert_eq!(handler.gestures.len(), 1);
        assert_eq!(handler.gestures[0].zone, Zone::Right);
        assert_eq!(handler.gestures[0].direction, GestureDirection::Down);
    }

    #[test]
    fn proxy_handler_receives_slider_steps_live() {
        let capabilities = test_capabilities();
        let mut engine = Engine::with_sliders(
            capabilities,
            EdgeWidths::all(0.10),
            vec![SliderSpec {
                zone: Zone::Right,
                axis: SliderAxis::Vertical,
                step: 0.09,
            }],
        );
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyRuntimeStats::default();
        let mut handler = RecordingGestureHandler::default();
        let frames = [
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(300),
                RawEvent::abs_mt_position_x(980),
                RawEvent::abs_mt_position_y(400),
            ]),
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_position_y(610),
            ]),
            RawFrame::new(vec![
                RawEvent::abs_mt_slot(0),
                RawEvent::abs_mt_tracking_id(-1),
            ]),
        ];

        for frame in &frames {
            process_proxy_frame_with_handler(
                &mut engine,
                &mut composer,
                &mut sink,
                &mut stats,
                frame,
                &mut handler,
            )
            .expect("slider frames should process");
        }

        assert_eq!(handler.slider_steps, stats.slider_steps);
        assert_eq!(handler.slider_steps.len(), 3);
        assert!(stats.gestures.is_empty());
        assert_eq!(
            stats.slider_step_counts.get(&SliderStepCountKey {
                zone: Zone::Right,
                direction: SliderDirection::Down,
            }),
            Some(&3)
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
}

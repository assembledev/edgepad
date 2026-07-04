use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::core::{Capabilities, EdgeWidths, Engine, Gesture, GestureDirection, Zone};
use crate::dump::capabilities_from_raw_device;
use crate::raw::{
    extract_core_events, route_raw_frame, RawEvent, RawFrame, RawOutputComposer, RawOutputSink,
    RecordingRawOutputSink, ABS_MT_SLOT, ABS_MT_TRACKING_ID, BTN_TOUCH, EV_ABS, EV_KEY, EV_SYN,
    SYN_DROPPED, SYN_REPORT,
};
use crate::uinput::{
    build_virtual_touchpad, UinputEventWriter, UinputRawOutputSink, VirtualTouchpadSpec,
};
use evdev::{raw_stream::RawDevice, KeyCode};

nix::ioctl_read_buf!(eviocgmtslots, b'E', 0x0a, u8);

pub const DEFAULT_EDGE_WIDTH: f32 = 0.10;

const UINPUT_UNGRAB_SETTLE_DELAY: Duration = Duration::from_millis(30);
const UINPUT_IDLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    DryRun,
    UinputGrab,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyRunConfig {
    pub device_path: PathBuf,
    pub frame_limit: usize,
    pub edge_widths: EdgeWidths,
    pub mode: ProxyMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyRunSummary {
    pub mode: ProxyMode,
    pub device_path: PathBuf,
    pub capabilities: Capabilities,
    pub edge_widths: EdgeWidths,
    pub requested_frame_boundaries: usize,
    pub stats: ProxyRuntimeStats,
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
    pub gestures: Vec<Gesture>,
    pub gesture_counts: BTreeMap<GestureCountKey, usize>,
    pub resync_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GestureCountKey {
    pub zone: Zone,
    pub direction: GestureDirection,
}

pub fn run_proxy(config: &ProxyRunConfig) -> Result<ProxyRunSummary, String> {
    match config.mode {
        ProxyMode::DryRun => proxy_dry_run(config),
        ProxyMode::UinputGrab => proxy_uinput_grab(config),
    }
}

fn proxy_dry_run(config: &ProxyRunConfig) -> Result<ProxyRunSummary, String> {
    let (mut device, capabilities) = open_proxy_device(&config.device_path)?;
    let mut sink = RecordingRawOutputSink::default();
    let stats = run_proxy_loop(
        &mut device,
        capabilities,
        config.edge_widths,
        config.frame_limit,
        StopAfterFrameLimit::Immediately,
        None,
        &mut sink,
    )?;

    Ok(ProxyRunSummary {
        mode: ProxyMode::DryRun,
        device_path: config.device_path.clone(),
        capabilities,
        edge_widths: config.edge_widths,
        requested_frame_boundaries: config.frame_limit,
        stats,
    })
}

fn proxy_uinput_grab(config: &ProxyRunConfig) -> Result<ProxyRunSummary, String> {
    let (mut device, capabilities) = open_proxy_device(&config.device_path)?;
    ensure_physical_touchpad_idle_at_start(&config.device_path, &device, capabilities)?;

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
    let run_result = run_proxy_loop(
        &mut device,
        capabilities,
        config.edge_widths,
        config.frame_limit,
        StopAfterFrameLimit::WhenIdle,
        Some(UINPUT_IDLE_DRAIN_TIMEOUT),
        &mut sink,
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
        requested_frame_boundaries: config.frame_limit,
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
    if !physical_touch_is_down(device, capabilities)? {
        return Ok(());
    }

    Err(format!(
        "touchpad is already touched on {}; release all fingers and retry live proxy",
        device_path.display()
    ))
}

fn physical_touch_is_down(device: &RawDevice, capabilities: Capabilities) -> Result<bool, String> {
    let key_state = device.get_key_state().map_err(|err| {
        format!("failed to read current touch key state before live proxy: {err}")
    })?;
    let tracking_ids = mt_tracking_ids(device, capabilities)?;
    Ok(physical_touch_snapshot_is_down(
        key_state.contains(KeyCode::BTN_TOUCH),
        &tracking_ids,
    ))
}

fn physical_touch_snapshot_is_down(btn_touch_down: bool, mt_tracking_ids: &[i32]) -> bool {
    btn_touch_down || mt_tracking_ids.iter().any(|tracking_id| *tracking_id >= 0)
}

fn mt_tracking_ids(device: &RawDevice, capabilities: Capabilities) -> Result<Vec<i32>, String> {
    let slot_count = (capabilities.slot_max - capabilities.slot_min + 1) as usize;
    let mut request = vec![0_i32; slot_count + 1];
    request[0] = ABS_MT_TRACKING_ID as i32;
    let request_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            request.as_mut_ptr().cast::<u8>(),
            request.len() * std::mem::size_of::<i32>(),
        )
    };
    unsafe { eviocgmtslots(device.as_raw_fd(), request_bytes) }.map_err(|err| {
        format!("failed to read current multitouch slot state before live proxy: {err}")
    })?;
    Ok(request[1..].to_vec())
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

fn run_proxy_loop<S>(
    device: &mut RawDevice,
    capabilities: Capabilities,
    edge_widths: EdgeWidths,
    frame_limit: usize,
    stop_after_frame_limit: StopAfterFrameLimit,
    drain_timeout: Option<Duration>,
    sink: &mut S,
) -> Result<ProxyRuntimeStats, String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
{
    let mut engine = Engine::new(capabilities, edge_widths);
    let mut composer = RawOutputComposer::new(capabilities);
    let mut stats = ProxyRuntimeStats::default();
    let mut touch_state = PhysicalTouchState::new(capabilities);
    let mut stopper = FrameLimitStopper::new(frame_limit, stop_after_frame_limit);
    let mut drain_deadline: Option<Instant> = None;
    let mut current = Vec::new();

    loop {
        let timeout =
            drain_deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let Some(events) = fetch_proxy_events(device, timeout)? else {
            stats.idle_drain_timed_out = true;
            finish_proxy_output(&mut composer, sink, &mut stats)?;
            return Ok(stats);
        };

        for raw in events {
            match (raw.kind, raw.code) {
                (EV_SYN, SYN_REPORT) => {
                    process_pending_proxy_frame(
                        &mut current,
                        &mut touch_state,
                        &mut engine,
                        &mut composer,
                        sink,
                        &mut stats,
                    )?;
                    stats.input_frame_boundaries += 1;
                    if stopper.observe_frame_boundary(touch_state.is_touch_down()) {
                        stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                        finish_proxy_output(&mut composer, sink, &mut stats)?;
                        return Ok(stats);
                    }
                    if stopper.is_draining() && drain_deadline.is_none() {
                        drain_deadline = drain_timeout.map(|timeout| Instant::now() + timeout);
                    }
                    stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                }
                (EV_SYN, SYN_DROPPED) => {
                    process_pending_proxy_frame(
                        &mut current,
                        &mut touch_state,
                        &mut engine,
                        &mut composer,
                        sink,
                        &mut stats,
                    )?;
                    let dropped = RawFrame::new(vec![raw]);
                    touch_state.observe_frame(&dropped);
                    process_proxy_frame(&mut engine, &mut composer, sink, &mut stats, &dropped)?;
                    stats.input_frame_boundaries += 1;
                    if stopper.observe_frame_boundary(touch_state.is_touch_down()) {
                        stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                        finish_proxy_output(&mut composer, sink, &mut stats)?;
                        return Ok(stats);
                    }
                    if stopper.is_draining() && drain_deadline.is_none() {
                        drain_deadline = drain_timeout.map(|timeout| Instant::now() + timeout);
                    }
                    stats.idle_drain_frame_boundaries = stopper.extra_frame_boundaries();
                }
                _ => current.push(raw),
            }
        }
    }
}

fn fetch_proxy_events(
    device: &mut RawDevice,
    timeout: Option<Duration>,
) -> Result<Option<Vec<RawEvent>>, String> {
    if let Some(timeout) = timeout {
        let timeout_ms = poll_timeout_ms(timeout);
        let mut poll_fd = libc::pollfd {
            fd: device.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = loop {
            let result = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
            if result >= 0 {
                break result;
            }
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                return Err(format!("failed to wait for proxy events: {err}"));
            }
        };
        if ready == 0 {
            return Ok(None);
        }
    }

    let events = device
        .fetch_events()
        .map_err(|err| format!("failed to read events from proxy device: {err}"))?
        .map(|event| RawEvent::new(event.event_type().0, event.code(), event.value()))
        .collect();
    Ok(Some(events))
}

fn poll_timeout_ms(timeout: Duration) -> i32 {
    let millis = timeout.as_millis();
    if millis == 0 {
        0
    } else {
        millis.min(i32::MAX as u128) as i32
    }
}

fn process_pending_proxy_frame<S>(
    current: &mut Vec<RawEvent>,
    touch_state: &mut PhysicalTouchState,
    engine: &mut Engine,
    composer: &mut RawOutputComposer,
    sink: &mut S,
    stats: &mut ProxyRuntimeStats,
) -> Result<(), String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
{
    if current.is_empty() {
        return Ok(());
    }

    let frame = RawFrame::new(std::mem::take(current));
    touch_state.observe_frame(&frame);
    process_proxy_frame(engine, composer, sink, stats, &frame)
}

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
    stats.raw_frames += 1;
    stats.raw_events += frame.events.len();

    let recognizer_events = extract_core_events(frame).len();
    stats.recognizer_events += recognizer_events;

    let routed = route_raw_frame(engine, frame).map_err(|err| format!("proxy failed: {err:?}"))?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopAfterFrameLimit {
    Immediately,
    WhenIdle,
}

#[derive(Debug, Clone)]
struct FrameLimitStopper {
    requested_frame_boundaries: usize,
    observed_frame_boundaries: usize,
    extra_frame_boundaries: usize,
    mode: StopAfterFrameLimit,
    draining: bool,
}

impl FrameLimitStopper {
    fn new(requested_frame_boundaries: usize, mode: StopAfterFrameLimit) -> Self {
        Self {
            requested_frame_boundaries,
            observed_frame_boundaries: 0,
            extra_frame_boundaries: 0,
            mode,
            draining: false,
        }
    }

    fn observe_frame_boundary(&mut self, physical_touch_down: bool) -> bool {
        self.observed_frame_boundaries += 1;
        if self.observed_frame_boundaries < self.requested_frame_boundaries {
            return false;
        }

        if self.observed_frame_boundaries == self.requested_frame_boundaries {
            return match self.mode {
                StopAfterFrameLimit::Immediately => true,
                StopAfterFrameLimit::WhenIdle if !physical_touch_down => true,
                StopAfterFrameLimit::WhenIdle => {
                    self.draining = true;
                    false
                }
            };
        }

        self.extra_frame_boundaries += 1;
        match self.mode {
            StopAfterFrameLimit::Immediately => true,
            StopAfterFrameLimit::WhenIdle => {
                self.draining = physical_touch_down;
                !physical_touch_down
            }
        }
    }

    fn is_draining(&self) -> bool {
        self.draining
    }

    fn extra_frame_boundaries(&self) -> usize {
        self.extra_frame_boundaries
    }
}

#[derive(Debug, Clone)]
struct PhysicalTouchState {
    current_slot: i32,
    active_slots: Vec<bool>,
    btn_touch_down: bool,
    capabilities: Capabilities,
}

impl PhysicalTouchState {
    fn new(capabilities: Capabilities) -> Self {
        let slot_count = (capabilities.slot_max - capabilities.slot_min + 1) as usize;
        Self {
            current_slot: capabilities.slot_min,
            active_slots: vec![false; slot_count],
            btn_touch_down: false,
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
        self.btn_touch_down || self.active_slots.iter().any(|active| *active)
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
        ABS_X, BTN_TOOL_DOUBLETAP, BTN_TOOL_FINGER, BTN_TOOL_QUADTAP, BTN_TOOL_QUINTTAP,
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
        assert_eq!(stats.composed_events, 10);
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
        let mut stopper = FrameLimitStopper::new(2, StopAfterFrameLimit::WhenIdle);

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
        let mut stopper = FrameLimitStopper::new(2, StopAfterFrameLimit::Immediately);

        assert!(!stopper.observe_frame_boundary(true));
        assert!(stopper.observe_frame_boundary(true));
        assert_eq!(stopper.extra_frame_boundaries(), 0);
    }

    #[test]
    fn physical_touch_snapshot_treats_active_mt_tracking_id_as_touch_down() {
        assert!(physical_touch_snapshot_is_down(false, &[42, -1, -1]));
    }

    #[test]
    fn physical_touch_snapshot_treats_all_released_slots_as_idle() {
        assert!(!physical_touch_snapshot_is_down(false, &[-1, -1, -1]));
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
    fn proxy_summary_preserves_runtime_metadata() {
        let config = ProxyRunConfig {
            device_path: PathBuf::from("/dev/input/event7"),
            frame_limit: 12,
            edge_widths: EdgeWidths::all(0.2),
            mode: ProxyMode::DryRun,
        };
        let summary = ProxyRunSummary {
            mode: config.mode,
            device_path: config.device_path.clone(),
            capabilities: test_capabilities(),
            edge_widths: config.edge_widths,
            requested_frame_boundaries: config.frame_limit,
            stats: ProxyRuntimeStats::default(),
        };

        assert_eq!(summary.mode, ProxyMode::DryRun);
        assert_eq!(summary.device_path, config.device_path);
        assert_eq!(summary.edge_widths, EdgeWidths::all(0.2));
        assert_eq!(summary.requested_frame_boundaries, 12);
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

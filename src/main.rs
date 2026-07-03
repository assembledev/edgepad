use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::LineWriter;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant};

use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine, Gesture, GestureDirection, Zone};
use edgepad::device::{discover_device_report, format_device_line, touchpad_candidates};
use edgepad::dump::{
    capabilities_from_raw_device, write_capture_header, write_fixture_events_with_limit,
    write_raw_events_with_limit, WriteEventsResult,
};
use edgepad::raw::{
    extract_core_events, parse_raw_dump_file, route_raw_frame, write_raw_output_frame, RawEvent,
    RawFrame, RawOutputComposer, RawOutputSink, RecordingRawOutputSink, ABS_MT_SLOT,
    ABS_MT_TRACKING_ID, BTN_TOUCH, EV_ABS, EV_KEY, EV_SYN, SYN_DROPPED, SYN_REPORT,
};
use edgepad::replay::{parse_replay_file, replay_stats, run_frames};
use edgepad::uinput::{build_virtual_touchpad, UinputRawOutputSink, VirtualTouchpadSpec};
use evdev::{raw_stream::RawDevice, KeyCode};

const USAGE: &str = "usage: edgepad replay <fixture.ev> | edgepad replay-raw <raw.ev> | edgepad devices [--root <input-root>] [--all] | edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw] | edgepad proxy --device <event-node> --frames N (--dry-run | --uinput --grab) [--edge-width F]";
const DUMP_USAGE: &str =
    "usage: edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw]";
const PROXY_USAGE: &str =
    "usage: edgepad proxy --device <event-node> --frames N (--dry-run | --uinput --grab) [--edge-width F]";
const UINPUT_UNGRAB_SETTLE_DELAY: Duration = Duration::from_millis(30);
const UINPUT_IDLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);
const DEFAULT_EDGE_WIDTH: f32 = 0.10;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("replay") => {
            let path = args.next().ok_or_else(|| USAGE.to_string())?;
            if args.next().is_some() {
                return Err(USAGE.to_string());
            }
            replay(Path::new(&path))
        }
        Some("replay-raw") => {
            let path = args.next().ok_or_else(|| USAGE.to_string())?;
            if args.next().is_some() {
                return Err(USAGE.to_string());
            }
            replay_raw(Path::new(&path))
        }
        Some("devices") => {
            let args = parse_devices_args(args)?;
            devices(&args.root, args.show_all)
        }
        Some("dump") => {
            let args = parse_dump_args(args)?;
            dump(&args.device, &args.out, args.frames, args.raw)
        }
        Some("proxy") => {
            let args = parse_proxy_args(args)?;
            proxy(&args)
        }
        _ => Err(USAGE.to_string()),
    }
}

struct DeviceArgs {
    root: PathBuf,
    show_all: bool,
}

struct DumpArgs {
    device: PathBuf,
    out: PathBuf,
    frames: Option<usize>,
    raw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyMode {
    DryRun,
    UinputGrab,
}

struct ProxyArgs {
    device: PathBuf,
    frames: usize,
    mode: ProxyMode,
    edge_width: f32,
}

fn parse_devices_args(mut args: impl Iterator<Item = String>) -> Result<DeviceArgs, String> {
    let mut root = PathBuf::from("/dev/input");
    let mut show_all = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().ok_or_else(|| USAGE.to_string())?.into(),
            "--all" => show_all = true,
            _ => return Err(USAGE.to_string()),
        }
    }

    Ok(DeviceArgs { root, show_all })
}

fn parse_dump_args(mut args: impl Iterator<Item = String>) -> Result<DumpArgs, String> {
    let mut device = None;
    let mut out = None;
    let mut frames = None;
    let mut raw = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => device = Some(args.next().ok_or_else(|| DUMP_USAGE.to_string())?.into()),
            "--out" => out = Some(args.next().ok_or_else(|| DUMP_USAGE.to_string())?.into()),
            "--frames" => {
                let raw_value = args.next().ok_or_else(|| DUMP_USAGE.to_string())?;
                let parsed = parse_positive_frame_limit(&raw_value)?;
                frames = Some(parsed);
            }
            "--raw" => raw = true,
            _ => return Err(DUMP_USAGE.to_string()),
        }
    }

    Ok(DumpArgs {
        device: device.ok_or_else(|| DUMP_USAGE.to_string())?,
        out: out.ok_or_else(|| DUMP_USAGE.to_string())?,
        frames,
        raw,
    })
}

fn parse_proxy_args(mut args: impl Iterator<Item = String>) -> Result<ProxyArgs, String> {
    let mut device = None;
    let mut frames = None;
    let mut dry_run = false;
    let mut uinput = false;
    let mut grab = false;
    let mut edge_width = DEFAULT_EDGE_WIDTH;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => device = Some(args.next().ok_or_else(|| PROXY_USAGE.to_string())?.into()),
            "--frames" => {
                let raw_value = args.next().ok_or_else(|| PROXY_USAGE.to_string())?;
                frames = Some(parse_positive_frame_limit(&raw_value)?);
            }
            "--dry-run" => dry_run = true,
            "--uinput" => uinput = true,
            "--grab" => grab = true,
            "--edge-width" => {
                let raw_value = args.next().ok_or_else(|| PROXY_USAGE.to_string())?;
                edge_width = parse_edge_width(&raw_value)?;
            }
            "--no-grab" => return Err("unknown proxy option --no-grab".to_string()),
            other if other.starts_with('-') => return Err(format!("unknown proxy option {other}")),
            _ => return Err(PROXY_USAGE.to_string()),
        }
    }

    let device = device.ok_or_else(|| PROXY_USAGE.to_string())?;
    let frames = frames.ok_or_else(|| PROXY_USAGE.to_string())?;
    let mode = match (dry_run, uinput, grab) {
        (true, false, false) => ProxyMode::DryRun,
        (false, true, true) => ProxyMode::UinputGrab,
        (false, true, false) => return Err("proxy --uinput requires --grab".to_string()),
        (false, false, true) => return Err("proxy --grab requires --uinput".to_string()),
        (false, false, false) => {
            return Err("proxy requires either --dry-run or --uinput --grab".to_string())
        }
        _ => return Err("proxy modes are mutually exclusive".to_string()),
    };

    Ok(ProxyArgs {
        device,
        frames,
        mode,
        edge_width,
    })
}

fn parse_positive_frame_limit(raw_value: &str) -> Result<usize, String> {
    let parsed = raw_value
        .parse::<usize>()
        .map_err(|_| "--frames must be a positive integer".to_string())?;
    if parsed == 0 {
        return Err("--frames must be a positive integer".to_string());
    }
    Ok(parsed)
}

fn parse_edge_width(raw_value: &str) -> Result<f32, String> {
    let parsed = raw_value
        .parse::<f32>()
        .map_err(|_| "--edge-width must be > 0 and < 0.5".to_string())?;
    if !(parsed > 0.0 && parsed < 0.5) {
        return Err("--edge-width must be > 0 and < 0.5".to_string());
    }
    Ok(parsed)
}

fn devices(root: &Path, show_all: bool) -> Result<(), String> {
    let report = discover_device_report(root)
        .map_err(|err| format!("failed to list {}: {err}", root.display()))?;

    if report.event_node_count == 0 {
        println!("no event devices found under {}", root.display());
        return Ok(());
    }

    if report.summaries.is_empty() {
        println!(
            "no readable event devices found under {} ({}; try sudo, group input, or seat/logind ACLs)",
            root.display(),
            event_node_count_text(report.event_node_count)
        );
        return Ok(());
    }

    if show_all {
        for summary in &report.summaries {
            println!("{}", format_device_line(summary));
        }
        return Ok(());
    }

    let candidates = touchpad_candidates(&report.summaries);
    if candidates.is_empty() {
        println!("no touchpad candidates found under {}", root.display());
        println!(
            "readable non-touchpad devices: {} (use --all to inspect)",
            report.summaries.len()
        );
        return Ok(());
    }

    for summary in candidates {
        println!("{}", format_device_line(summary));
    }

    Ok(())
}

fn event_node_count_text(count: usize) -> String {
    if count == 1 {
        "1 event node was present but could not be opened".to_string()
    } else {
        format!("{count} event nodes were present but could not be opened")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DumpFormat {
    Replay,
    Raw,
}

impl DumpFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Replay => "replay",
            Self::Raw => "raw",
        }
    }
}

fn dump(
    device_path: &Path,
    out_path: &Path,
    mut remaining_frames: Option<usize>,
    raw: bool,
) -> Result<(), String> {
    let requested_frames = remaining_frames;
    let format = if raw {
        DumpFormat::Raw
    } else {
        DumpFormat::Replay
    };
    let mut device = RawDevice::open(device_path)
        .map_err(|err| format!("failed to open device {}: {err}", device_path.display()))?;
    let file = File::create(out_path)
        .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
    let mut writer = LineWriter::new(file);
    let capabilities = capabilities_from_raw_device(&device);
    write_capture_header(&mut writer, device_path, capabilities)
        .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
    let mut total = WriteEventsResult::default();

    loop {
        let events = device.fetch_events().map_err(|err| {
            format!(
                "failed to read events from {}: {err}",
                device_path.display()
            )
        })?;
        let result = if raw {
            write_raw_events_with_limit(&mut writer, events, &mut remaining_frames)
        } else {
            write_fixture_events_with_limit(&mut writer, events, &mut remaining_frames)
        }
        .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
        total.add(result);
        if total.reached_limit {
            print_dump_summary(
                device_path,
                out_path,
                capabilities,
                requested_frames,
                total,
                format,
            );
            return Ok(());
        }
    }
}

fn print_dump_summary(
    device_path: &Path,
    out_path: &Path,
    capabilities: Option<Capabilities>,
    requested_frames: Option<usize>,
    stats: WriteEventsResult,
    format: DumpFormat,
) {
    println!("wrote: {}", out_path.display());
    println!("device: {}", device_path.display());
    println!("format: {}", format.label());
    if let Some(capabilities) = capabilities {
        println!(
            "capabilities: slots={}..={} x={}..={} y={}..={}",
            capabilities.slot_min,
            capabilities.slot_max,
            capabilities.x.min,
            capabilities.x.max,
            capabilities.y.min,
            capabilities.y.max
        );
    } else {
        println!("capabilities: unavailable");
    }
    if let Some(requested_frames) = requested_frames {
        println!("requested_frame_boundaries: {requested_frames}");
    }
    println!(
        "written_frame_boundaries: {}",
        stats.frame_boundaries_written
    );
    println!("written_events: {}", stats.events_written);
    println!("next: {}", dump_next_command(format, out_path));
}

fn dump_next_command(format: DumpFormat, out_path: &Path) -> String {
    match format {
        DumpFormat::Replay => format!("edgepad replay {}", out_path.display()),
        DumpFormat::Raw => format!("edgepad replay-raw {}", out_path.display()),
    }
}

#[derive(Debug, Default)]
struct ProxyDryRunStats {
    input_frame_boundaries: usize,
    raw_frames: usize,
    raw_events: usize,
    recognizer_events: usize,
    recognizer_passthrough_events: usize,
    passthrough_frames: usize,
    claimed_edge_frames: usize,
    empty_output_frames: usize,
    composed_frames: usize,
    composed_events: usize,
    cleanup_output_frames: usize,
    cleanup_output_events: usize,
    settle_output_frames: usize,
    settle_output_events: usize,
    idle_drain_frame_boundaries: usize,
    idle_drain_timed_out: bool,
    gestures: Vec<Gesture>,
    gesture_counts: BTreeMap<String, usize>,
    resync_required: bool,
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

fn proxy(args: &ProxyArgs) -> Result<(), String> {
    let edge_widths = EdgeWidths::all(args.edge_width);
    match args.mode {
        ProxyMode::DryRun => proxy_dry_run(&args.device, args.frames, edge_widths),
        ProxyMode::UinputGrab => proxy_uinput_grab(&args.device, args.frames, edge_widths),
    }
}

fn open_proxy_device(device_path: &Path) -> Result<(RawDevice, Capabilities), String> {
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
    device_path: &Path,
    device: &RawDevice,
) -> Result<(), String> {
    if !physical_touch_is_down(device)? {
        return Ok(());
    }

    Err(format!(
        "touchpad is already touched on {}; release all fingers and retry live proxy",
        device_path.display()
    ))
}

fn physical_touch_is_down(device: &RawDevice) -> Result<bool, String> {
    let key_state = device
        .get_key_state()
        .map_err(|err| format!("failed to read current touch state before live proxy: {err}"))?;
    Ok(key_state.contains(KeyCode::BTN_TOUCH))
}

fn proxy_dry_run(
    device_path: &Path,
    frame_limit: usize,
    edge_widths: EdgeWidths,
) -> Result<(), String> {
    let (mut device, capabilities) = open_proxy_device(device_path)?;
    let mut sink = RecordingRawOutputSink::default();
    let stats = run_proxy_loop(
        &mut device,
        capabilities,
        edge_widths,
        frame_limit,
        StopAfterFrameLimit::Immediately,
        None,
        &mut sink,
    )?;
    print_proxy_summary(
        ProxyMode::DryRun,
        device_path,
        capabilities,
        edge_widths,
        frame_limit,
        &stats,
    );
    Ok(())
}

fn proxy_uinput_grab(
    device_path: &Path,
    frame_limit: usize,
    edge_widths: EdgeWidths,
) -> Result<(), String> {
    let (mut device, capabilities) = open_proxy_device(device_path)?;
    ensure_physical_touchpad_idle_at_start(device_path, &device)?;

    let spec = VirtualTouchpadSpec::from_raw_device(&device, capabilities);
    let virtual_device = build_virtual_touchpad(&spec).map_err(|err| {
        format!("failed to create virtual touchpad via /dev/uinput before grabbing physical device: {err}")
    })?;
    let mut sink = UinputRawOutputSink::new(virtual_device);

    device
        .grab()
        .map_err(|err| format!("failed to grab device {}: {err}", device_path.display()))?;
    let mut stats = run_proxy_loop(
        &mut device,
        capabilities,
        edge_widths,
        frame_limit,
        StopAfterFrameLimit::WhenIdle,
        Some(UINPUT_IDLE_DRAIN_TIMEOUT),
        &mut sink,
    )?;
    emit_proxy_settle_output(capabilities, &mut sink, &mut stats)?;
    std::thread::sleep(UINPUT_UNGRAB_SETTLE_DELAY);
    device
        .ungrab()
        .map_err(|err| format!("failed to ungrab device {}: {err}", device_path.display()))?;
    print_proxy_summary(
        ProxyMode::UinputGrab,
        device_path,
        capabilities,
        edge_widths,
        frame_limit,
        &stats,
    );
    Ok(())
}

fn run_proxy_loop<S>(
    device: &mut RawDevice,
    capabilities: Capabilities,
    edge_widths: EdgeWidths,
    frame_limit: usize,
    stop_after_frame_limit: StopAfterFrameLimit,
    drain_timeout: Option<Duration>,
    sink: &mut S,
) -> Result<ProxyDryRunStats, String>
where
    S: RawOutputSink,
    S::Error: std::fmt::Debug,
{
    let mut engine = Engine::new(capabilities, edge_widths);
    let mut composer = RawOutputComposer::new(capabilities);
    let mut stats = ProxyDryRunStats::default();
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
    stats: &mut ProxyDryRunStats,
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
    stats: &mut ProxyDryRunStats,
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
    stats: &mut ProxyDryRunStats,
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
    stats: &mut ProxyDryRunStats,
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

fn gesture_count_key(gesture: Gesture) -> String {
    format!(
        "{}/{}",
        zone_name(gesture.zone),
        direction_name(gesture.direction)
    )
}

fn print_proxy_summary(
    mode: ProxyMode,
    device_path: &Path,
    capabilities: Capabilities,
    edge_widths: EdgeWidths,
    requested_frames: usize,
    stats: &ProxyDryRunStats,
) {
    match mode {
        ProxyMode::DryRun => println!("mode: proxy dry-run"),
        ProxyMode::UinputGrab => println!("mode: proxy uinput grab"),
    }
    println!("device: {}", device_path.display());
    println!(
        "capabilities: slots={}..={} x={}..={} y={}..={}",
        capabilities.slot_min,
        capabilities.slot_max,
        capabilities.x.min,
        capabilities.x.max,
        capabilities.y.min,
        capabilities.y.max
    );
    println!("requested_frame_boundaries: {requested_frames}");
    println!("edge_width: {:.3}", edge_widths.left);
    println!("input_frame_boundaries: {}", stats.input_frame_boundaries);
    println!("raw_frames: {}", stats.raw_frames);
    println!("raw_events: total={}", stats.raw_events);
    println!("recognizer_events: total={}", stats.recognizer_events);
    println!(
        "recognizer_passthrough_events: {}",
        stats.recognizer_passthrough_events
    );
    println!("passthrough_frames: {}", stats.passthrough_frames);
    println!("claimed_edge_frames: {}", stats.claimed_edge_frames);
    println!("empty_output_frames: {}", stats.empty_output_frames);
    println!("composed_frames: {}", stats.composed_frames);
    println!("composed_events: {}", stats.composed_events);
    println!("cleanup_output_frames: {}", stats.cleanup_output_frames);
    println!("cleanup_output_events: {}", stats.cleanup_output_events);
    println!("settle_output_frames: {}", stats.settle_output_frames);
    println!("settle_output_events: {}", stats.settle_output_events);
    println!(
        "idle_drain_frame_boundaries: {}",
        stats.idle_drain_frame_boundaries
    );
    println!("idle_drain_timed_out: {}", stats.idle_drain_timed_out);
    println!("gestures: {}", stats.gestures.len());
    if !stats.gesture_counts.is_empty() {
        println!("gesture_counts:");
        for (key, count) in &stats.gesture_counts {
            let (zone, direction) = key.split_once('/').unwrap_or((key, "unknown"));
            println!("  zone={zone} direction={direction} count={count}");
        }
    }
    for gesture in &stats.gestures {
        println!(
            "gesture slot={} tracking_id={} zone={} direction={}",
            gesture.slot,
            gesture.tracking_id,
            zone_name(gesture.zone),
            direction_name(gesture.direction)
        );
    }
    println!("resync_required: {}", stats.resync_required);
    match mode {
        ProxyMode::DryRun => println!("output: not emitted (--dry-run)"),
        ProxyMode::UinputGrab => println!("output: emitted (--uinput --grab)"),
    }
}

fn default_capabilities() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 9,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

fn replay(path: &Path) -> Result<(), String> {
    let input = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let replay = parse_replay_file(&input).map_err(|err| format!("parse failed: {err:?}"))?;

    let (capability_source, capabilities) = match replay.capabilities {
        Some(capabilities) => ("metadata", capabilities),
        None => ("defaults", default_capabilities()),
    };
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));

    let stats = replay_stats(&replay.frames);
    let outputs =
        run_frames(&mut engine, &replay.frames).map_err(|err| format!("replay failed: {err:?}"))?;
    let passthrough_events = outputs
        .iter()
        .map(|output| output.passthrough.len())
        .sum::<usize>();
    let gestures = outputs
        .iter()
        .flat_map(|output| output.gestures.iter())
        .collect::<Vec<_>>();
    let resync_required = outputs.iter().any(|output| output.resync_required);

    println!(
        "capabilities: {capability_source} slots={}..={} x={}..={} y={}..={}",
        capabilities.slot_min,
        capabilities.slot_max,
        capabilities.x.min,
        capabilities.x.max,
        capabilities.y.min,
        capabilities.y.max
    );
    println!("frames: {}", replay.frames.len());
    println!(
        "events: total={} slot={} tracking_start={} tracking_end={} x={} y={} syn_dropped={}",
        stats.total_events,
        stats.slot_events,
        stats.tracking_starts,
        stats.tracking_ends,
        stats.x_events,
        stats.y_events,
        stats.syn_dropped_events
    );
    println!(
        "contacts: started={} ended={}",
        stats.tracking_starts, stats.tracking_ends
    );
    println!("passthrough_events: {passthrough_events}");
    let gesture_count = gestures.len();
    println!("gestures: {gesture_count}");
    for gesture in gestures {
        println!(
            "gesture slot={} tracking_id={} zone={} direction={}",
            gesture.slot,
            gesture.tracking_id,
            zone_name(gesture.zone),
            direction_name(gesture.direction)
        );
    }
    println!("resync_required: {resync_required}");
    print_replay_diagnosis(&stats, passthrough_events, gesture_count);

    Ok(())
}

fn replay_raw(path: &Path) -> Result<(), String> {
    let input = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let raw_dump = parse_raw_dump_file(&input).map_err(|err| format!("parse failed: {err:?}"))?;

    let (capability_source, capabilities) = match raw_dump.capabilities {
        Some(capabilities) => ("metadata", capabilities),
        None => ("defaults", default_capabilities()),
    };
    let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
    let mut composer = RawOutputComposer::new(capabilities);

    let raw_events = raw_dump
        .frames
        .iter()
        .map(|frame| frame.events.len())
        .sum::<usize>();
    let mut recognizer_passthrough_events = 0;
    let mut gestures = Vec::new();
    let mut resync_required = false;
    let mut sink = RecordingRawOutputSink::default();

    for frame in &raw_dump.frames {
        let routed = route_raw_frame(&mut engine, frame)
            .map_err(|err| format!("raw replay failed: {err:?}"))?;
        recognizer_passthrough_events += routed.passthrough.len();
        resync_required |= routed.resync_required;
        gestures.extend(routed.gestures.iter().copied());

        write_raw_output_frame(&mut composer, &routed, &mut sink)
            .map_err(|err| format!("raw output write failed: {err:?}"))?;
    }

    let finish_frame = composer
        .finish()
        .map_err(|err| format!("raw output finish failed: {err:?}"))?;
    if !finish_frame.events.is_empty() {
        for event in finish_frame.events {
            sink.emit(event)
                .map_err(|err| format!("raw output finish emit failed: {err:?}"))?;
        }
        sink.sync()
            .map_err(|err| format!("raw output finish sync failed: {err:?}"))?;
    }

    let composed_events = sink
        .frames()
        .iter()
        .map(|frame| frame.events.len())
        .sum::<usize>();

    println!(
        "capabilities: {capability_source} slots={}..={} x={}..={} y={}..={}",
        capabilities.slot_min,
        capabilities.slot_max,
        capabilities.x.min,
        capabilities.x.max,
        capabilities.y.min,
        capabilities.y.max
    );
    println!("raw_frames: {}", raw_dump.frames.len());
    println!("raw_events: total={raw_events}");
    println!("recognizer_passthrough_events: {recognizer_passthrough_events}");
    println!("composed_events: {composed_events}");
    println!("gestures: {}", gestures.len());
    for gesture in gestures {
        println!(
            "gesture slot={} tracking_id={} zone={} direction={}",
            gesture.slot,
            gesture.tracking_id,
            zone_name(gesture.zone),
            direction_name(gesture.direction)
        );
    }
    println!("resync_required: {resync_required}");

    Ok(())
}

fn print_replay_diagnosis(
    stats: &edgepad::replay::ReplayStats,
    passthrough_events: usize,
    gestures: usize,
) {
    if stats.tracking_starts == 0 && (stats.x_events > 0 || stats.y_events > 0) {
        println!(
            "diagnosis: no contact starts found; capture likely began after a finger was already down or no new touch started"
        );
        println!(
            "diagnosis_hint: start dump before touching the pad, or use the gesture-release-then-center-finger flow for frame-limited captures"
        );
    } else if stats.tracking_starts > stats.tracking_ends {
        println!(
            "diagnosis: capture ended with active contact(s); frame budget likely stopped mid-contact"
        );
        println!(
            "diagnosis_hint: for edge gesture captures, perform the gesture, release it, then place a finger in the center until --frames finishes"
        );
    } else if stats.tracking_starts == 0 && stats.total_events == 0 {
        println!("diagnosis: no replay-relevant touch events found");
    } else if passthrough_events == 0 && gestures == 0 {
        println!(
            "diagnosis: complete contacts were parsed, but current recognizer produced no passthrough events or gestures"
        );
    }
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

fn direction_name(direction: GestureDirection) -> &'static str {
    match direction {
        GestureDirection::Up => "up",
        GestureDirection::Down => "down",
        GestureDirection::Left => "left",
        GestureDirection::Right => "right",
        GestureDirection::Tap => "tap",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_next_command_points_raw_dumps_to_replay_raw() {
        assert_eq!(
            dump_next_command(DumpFormat::Raw, Path::new("bug.raw.ev")),
            "edgepad replay-raw bug.raw.ev"
        );
    }

    #[test]
    fn dump_next_command_points_replay_dumps_to_replay() {
        assert_eq!(
            dump_next_command(DumpFormat::Replay, Path::new("bug.ev")),
            "edgepad replay bug.ev"
        );
    }

    #[test]
    fn proxy_dry_run_frame_stats_match_raw_replay_output() {
        let capabilities = default_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyDryRunStats::default();
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
        let capabilities = default_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyDryRunStats::default();
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
            ..default_capabilities()
        };
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyDryRunStats::default();

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
    fn physical_touch_state_tracks_touch_lifecycle_from_raw_frames() {
        let capabilities = default_capabilities();
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
    fn proxy_dry_run_stats_count_gestures_by_zone_and_direction() {
        let capabilities = default_capabilities();
        let mut engine = Engine::new(capabilities, EdgeWidths::all(0.10));
        let mut composer = RawOutputComposer::new(capabilities);
        let mut sink = RecordingRawOutputSink::default();
        let mut stats = ProxyDryRunStats::default();
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
        assert_eq!(stats.gesture_counts.get("right/down"), Some(&1));
    }
}

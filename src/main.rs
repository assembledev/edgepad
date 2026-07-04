use std::env;
use std::fs::{self, File};
use std::io::LineWriter;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use edgepad::config::{load_edgepad_config, DeviceConfig, EdgepadConfig};
use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine, GestureDirection, Zone};
use edgepad::device::{discover_device_report, format_device_line, touchpad_candidates};
use edgepad::dump::{
    capabilities_from_raw_device, write_capture_header, write_fixture_events_with_limit,
    write_raw_events_with_limit, WriteEventsResult,
};
use edgepad::proxy::{
    run_proxy, ProxyMode, ProxyRunConfig, ProxyRunLimit, ProxyRunSummary, StopAfterFrameLimit,
    StopToken, DEFAULT_EDGE_WIDTH,
};
use edgepad::raw::{
    parse_raw_dump_file, route_raw_frame, write_raw_output_frame, RawOutputComposer, RawOutputSink,
    RecordingRawOutputSink,
};
use edgepad::replay::{parse_replay_file, replay_stats, run_frames};
use evdev::raw_stream::RawDevice;

const USAGE: &str = "usage: edgepad replay <fixture.ev> | edgepad replay-raw <raw.ev> | edgepad devices [--root <input-root>] [--all] | edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw] | edgepad proxy --device <event-node> --frames N (--dry-run | --uinput --grab) [--edge-width F] | edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]";
const DUMP_USAGE: &str =
    "usage: edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw]";
const PROXY_USAGE: &str =
    "usage: edgepad proxy --device <event-node> --frames N (--dry-run | --uinput --grab) [--edge-width F]";
const DAEMON_USAGE: &str = "usage: edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]";
const DAEMON_IDLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);
const DAEMON_SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(50);

static DAEMON_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

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
        Some("daemon") => {
            let args = parse_daemon_args(args)?;
            daemon(&args)
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

struct ProxyArgs {
    device: PathBuf,
    frames: usize,
    mode: ProxyMode,
    edge_width: f32,
}

struct DaemonArgs {
    config: EdgepadConfig,
    input_root: PathBuf,
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

fn parse_daemon_args(mut args: impl Iterator<Item = String>) -> Result<DaemonArgs, String> {
    let mut config_path: Option<PathBuf> = None;
    let mut device_override = None;
    let mut edge_width_override = None;
    let mut input_root = PathBuf::from("/dev/input");

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config_path = Some(args.next().ok_or_else(|| DAEMON_USAGE.to_string())?.into());
            }
            "--device" => {
                let raw_value = args.next().ok_or_else(|| DAEMON_USAGE.to_string())?;
                device_override = Some(DeviceConfig::parse(&raw_value)?);
            }
            "--input-root" => {
                input_root = args.next().ok_or_else(|| DAEMON_USAGE.to_string())?.into();
            }
            "--edge-width" => {
                let raw_value = args.next().ok_or_else(|| DAEMON_USAGE.to_string())?;
                edge_width_override = Some(parse_edge_width(&raw_value)?);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown daemon option {other}"))
            }
            _ => return Err(DAEMON_USAGE.to_string()),
        }
    }

    let mut config = match config_path {
        Some(path) => load_edgepad_config(&path)?,
        None => EdgepadConfig::default(),
    };
    if let Some(device) = device_override {
        config.device = device;
    }
    if let Some(edge_width) = edge_width_override {
        config.edge_width = edge_width;
    }

    Ok(DaemonArgs { config, input_root })
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
    edgepad::config::parse_edge_width(raw_value)
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

fn proxy(args: &ProxyArgs) -> Result<(), String> {
    let summary = run_proxy(&ProxyRunConfig {
        device_path: args.device.clone(),
        limit: ProxyRunLimit::Frames {
            frame_boundaries: args.frames,
            stop_after_limit: match args.mode {
                ProxyMode::DryRun => StopAfterFrameLimit::Immediately,
                ProxyMode::UinputGrab => StopAfterFrameLimit::WhenIdle,
            },
        },
        edge_widths: EdgeWidths::all(args.edge_width),
        mode: args.mode,
    })?;
    print_proxy_summary(&summary);
    Ok(())
}

fn daemon(args: &DaemonArgs) -> Result<(), String> {
    let device_path = args.config.device.resolve(&args.input_root)?;
    let stop = StopToken::new();
    install_daemon_signal_handlers(stop.clone())?;

    eprintln!(
        "edgepad daemon: device={} edge_width={:.3} gesture_bindings={}",
        device_path.display(),
        args.config.edge_width,
        args.config.gestures.len()
    );
    eprintln!("edgepad daemon: press Ctrl+C to stop");

    let summary = run_proxy(&ProxyRunConfig {
        device_path,
        limit: ProxyRunLimit::UntilStopped {
            stop,
            idle_drain_timeout: DAEMON_IDLE_DRAIN_TIMEOUT,
        },
        edge_widths: EdgeWidths::all(args.config.edge_width),
        mode: ProxyMode::UinputGrab,
    })?;
    print_proxy_summary(&summary);
    Ok(())
}

fn install_daemon_signal_handlers(stop: StopToken) -> Result<(), String> {
    DAEMON_STOP_REQUESTED.store(false, Ordering::SeqCst);
    register_daemon_signal_handler(libc::SIGINT)?;
    register_daemon_signal_handler(libc::SIGTERM)?;
    thread::Builder::new()
        .name("edgepad-daemon-signal".to_string())
        .spawn(move || {
            while !stop.is_stopped() {
                if DAEMON_STOP_REQUESTED.load(Ordering::SeqCst) {
                    stop.stop();
                    return;
                }
                thread::sleep(DAEMON_SIGNAL_POLL_INTERVAL);
            }
        })
        .map_err(|err| format!("failed to start daemon signal watcher: {err}"))?;
    Ok(())
}

fn register_daemon_signal_handler(signal: libc::c_int) -> Result<(), String> {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = handle_daemon_signal as *const () as libc::sighandler_t;
    action.sa_flags = 0;
    let sigempty_result = unsafe { libc::sigemptyset(&mut action.sa_mask) };
    if sigempty_result != 0 {
        return Err(format!(
            "failed to initialize daemon signal mask for signal {signal}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let sigaction_result = unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) };
    if sigaction_result != 0 {
        return Err(format!(
            "failed to install daemon signal handler for signal {signal}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

extern "C" fn handle_daemon_signal(_signal: libc::c_int) {
    DAEMON_STOP_REQUESTED.store(true, Ordering::SeqCst);
}

fn print_proxy_summary(summary: &ProxyRunSummary) {
    match summary.mode {
        ProxyMode::DryRun => println!("mode: proxy dry-run"),
        ProxyMode::UinputGrab => println!("mode: proxy uinput grab"),
    }
    println!("device: {}", summary.device_path.display());
    println!(
        "capabilities: slots={}..={} x={}..={} y={}..={}",
        summary.capabilities.slot_min,
        summary.capabilities.slot_max,
        summary.capabilities.x.min,
        summary.capabilities.x.max,
        summary.capabilities.y.min,
        summary.capabilities.y.max
    );
    if let Some(requested_frame_boundaries) = summary.requested_frame_boundaries {
        println!("requested_frame_boundaries: {requested_frame_boundaries}");
    }
    println!("edge_width: {:.3}", summary.edge_widths.left);
    let stats = &summary.stats;
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
            println!(
                "  zone={} direction={} count={count}",
                zone_name(key.zone),
                direction_name(key.direction)
            );
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
    match summary.mode {
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
}

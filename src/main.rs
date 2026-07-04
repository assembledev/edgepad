use std::env;
use std::fs::{self, File};
use std::io::LineWriter;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use edgepad::actions::{ActionDispatcher, ActionDispatcherStats};
use edgepad::config::{load_edgepad_config, DeviceConfig, EdgepadConfig};
use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine, GestureDirection, Zone};
use edgepad::device::{discover_device_report, format_device_line, touchpad_candidates};
use edgepad::doctor::{run_doctor, DoctorConfig, DoctorReport};
use edgepad::dump::{
    capabilities_from_raw_device, write_capture_header, write_fixture_events_with_limit,
    write_raw_events_with_limit, WriteEventsResult,
};
use edgepad::proxy::{
    run_proxy, run_proxy_with_gesture_handler, ProxyMode, ProxyRunConfig, ProxyRunLimit,
    ProxyRunSummary, StopAfterFrameLimit, StopToken, DEFAULT_EDGE_WIDTH,
};
use edgepad::raw::{
    parse_raw_dump_file, route_raw_frame, write_raw_output_frame, RawOutputComposer, RawOutputSink,
    RecordingRawOutputSink,
};
use edgepad::replay::{parse_replay_file, replay_stats, run_frames};
use evdev::raw_stream::RawDevice;

const USAGE: &str = "usage: edgepad replay <fixture.ev> | edgepad replay-raw <raw.ev> | edgepad devices [--root <input-root>] [--all] | edgepad doctor [--device auto|<event-node>] [--input-root <input-root>] [--uinput <path>] [--service <unit>] | edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw] | edgepad proxy --device <event-node> --frames N (--dry-run | --uinput --grab) [--edge-width F] | edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]";
const HELP: &str = "\
Usage:
  edgepad <command> [options]

Commands:
  devices     List readable input devices and touchpad candidates
  doctor      Check runtime prerequisites and service health
  daemon      Run the live edge-gesture proxy
  dump        Capture touchpad events into a replay fixture
  proxy       Run a bounded live proxy session for diagnostics
  replay      Replay a parsed fixture through the recognizer
  replay-raw  Replay a raw evdev capture through routing and output composition

Global options:
  -h, --help     Show this help
  -V, --version  Show version
";
const REPLAY_USAGE: &str = "usage: edgepad replay <fixture.ev>";
const REPLAY_HELP: &str = "\
Usage:
  edgepad replay <fixture.ev>

Replay a parsed fixture through the recognizer and print summary statistics.
";
const REPLAY_RAW_USAGE: &str = "usage: edgepad replay-raw <raw.ev>";
const REPLAY_RAW_HELP: &str = "\
Usage:
  edgepad replay-raw <raw.ev>

Replay a raw evdev capture through recognizer routing and output composition.
";
const DEVICES_USAGE: &str = "usage: edgepad devices [--root <input-root>] [--all]";
const DEVICES_HELP: &str = "\
Usage:
  edgepad devices [--root <input-root>] [--all]

Options:
      --root <input-root>  Input device directory [default: /dev/input]
      --all                Show all readable event devices, not only touchpad candidates
";
const DUMP_USAGE: &str =
    "usage: edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw]";
const DUMP_HELP: &str = "\
Usage:
  edgepad dump --device <event-node> --out <file.ev> [--frames N] [--raw]

Options:
      --device <event-node>  Physical touchpad event node to read
      --out <file.ev>        Output fixture path
      --frames N             Stop after N frame boundaries
      --raw                  Write raw evdev events instead of replay fixture events
";
const PROXY_USAGE: &str =
    "usage: edgepad proxy --device <event-node> --frames N (--dry-run | --uinput --grab) [--edge-width F]";
const PROXY_HELP: &str = "\
Usage:
  edgepad proxy --device <event-node> --frames N (--dry-run | --uinput --grab) [--edge-width F]

Options:
      --device <event-node>  Physical touchpad event node to proxy
      --frames N             Stop after N frame boundaries
      --dry-run              Recognize and report without emitting output
      --uinput --grab        Grab the physical device and emit through /dev/uinput
      --edge-width F         Edge zone width as a fraction of the touchpad axis
";
const DAEMON_USAGE: &str = "usage: edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]";
const DAEMON_HELP: &str = "\
Usage:
  edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]

Options:
      --config <file>             TOML config path
      --device auto|<event-node>  Touchpad device selection [default: auto]
      --input-root <input-root>   Input device directory for auto-detect [default: /dev/input]
      --edge-width F              Edge zone width as a fraction of the touchpad axis
";
const DOCTOR_USAGE: &str = "usage: edgepad doctor [--device auto|<event-node>] [--input-root <input-root>] [--uinput <path>] [--service <unit>]";
const DOCTOR_HELP: &str = "\
Usage:
  edgepad doctor [--device auto|<event-node>] [--input-root <input-root>] [--uinput <path>] [--service <unit>]

Options:
      --device auto|<event-node>  Touchpad device selection [default: auto]
      --input-root <input-root>   Input device directory for auto-detect [default: /dev/input]
      --uinput <path>             uinput device path [default: /dev/uinput]
      --service <unit>            systemd user unit to inspect [default: edgepad.service]
";
const DAEMON_IDLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);
const DAEMON_SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(50);
const DAEMON_STARTUP_RETRY_TIMEOUT: Duration = Duration::from_secs(30);
const DAEMON_STARTUP_RETRY_INTERVAL: Duration = Duration::from_millis(500);
const DAEMON_ACTION_QUEUE_CAPACITY: usize = 32;
const DAEMON_STARTUP_RETRY_ENV: &str = "EDGEPAD_DAEMON_STARTUP_RETRY_MS";

static DAEMON_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

fn main() {
    match run() {
        Ok(exit_code) => process::exit(exit_code),
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    }
}

fn run() -> Result<i32, String> {
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        Some("--help") | Some("-h") => {
            print_help(HELP);
            Ok(0)
        }
        Some("--version") | Some("-V") => {
            println!("edgepad {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Some("replay") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(REPLAY_HELP);
                return Ok(0);
            }
            let mut args = args.into_iter();
            let path = args.next().ok_or_else(|| REPLAY_USAGE.to_string())?;
            if args.next().is_some() {
                return Err(REPLAY_USAGE.to_string());
            }
            replay(Path::new(&path)).map(|()| 0)
        }
        Some("replay-raw") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(REPLAY_RAW_HELP);
                return Ok(0);
            }
            let mut args = args.into_iter();
            let path = args.next().ok_or_else(|| REPLAY_RAW_USAGE.to_string())?;
            if args.next().is_some() {
                return Err(REPLAY_RAW_USAGE.to_string());
            }
            replay_raw(Path::new(&path)).map(|()| 0)
        }
        Some("devices") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DEVICES_HELP);
                return Ok(0);
            }
            let args = parse_devices_args(args.into_iter())?;
            devices(&args.root, args.show_all).map(|()| 0)
        }
        Some("doctor") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DOCTOR_HELP);
                return Ok(0);
            }
            let config = parse_doctor_args(args.into_iter())?;
            doctor(&config)
        }
        Some("dump") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DUMP_HELP);
                return Ok(0);
            }
            let args = parse_dump_args(args.into_iter())?;
            dump(&args.device, &args.out, args.frames, args.raw).map(|()| 0)
        }
        Some("proxy") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(PROXY_HELP);
                return Ok(0);
            }
            let args = parse_proxy_args(args.into_iter())?;
            proxy(&args).map(|()| 0)
        }
        Some("daemon") => {
            let args = args.collect::<Vec<_>>();
            if is_help_request(&args) {
                print_help(DAEMON_HELP);
                return Ok(0);
            }
            let args = parse_daemon_args(args.into_iter())?;
            daemon(&args).map(|()| 0)
        }
        _ => Err(USAGE.to_string()),
    }
}

fn is_help_request(args: &[String]) -> bool {
    args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h")
}

fn print_help(help: &str) {
    print!("{help}");
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
            "--root" => {
                root = args.next().ok_or_else(|| DEVICES_USAGE.to_string())?.into();
            }
            "--all" => show_all = true,
            _ => return Err(DEVICES_USAGE.to_string()),
        }
    }

    Ok(DeviceArgs { root, show_all })
}

fn parse_doctor_args(mut args: impl Iterator<Item = String>) -> Result<DoctorConfig, String> {
    let mut config = DoctorConfig::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => {
                let raw_value = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?;
                config.device = DeviceConfig::parse(&raw_value)?;
            }
            "--input-root" => {
                config.input_root = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?.into();
            }
            "--uinput" => {
                config.uinput_path = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?.into();
            }
            "--service" => {
                config.service_name = args.next().ok_or_else(|| DOCTOR_USAGE.to_string())?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown doctor option {other}"));
            }
            _ => return Err(DOCTOR_USAGE.to_string()),
        }
    }

    Ok(config)
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

fn doctor(config: &DoctorConfig) -> Result<i32, String> {
    let report = run_doctor(config);
    print_doctor_report(&report);
    if report.has_failures() {
        Ok(2)
    } else {
        Ok(0)
    }
}

fn print_doctor_report(report: &DoctorReport) {
    println!("edgepad doctor");
    for check in &report.checks {
        println!(
            "{:<4} {:<22} {}",
            check.status.label(),
            check.name,
            check.detail
        );
    }
    let counts = report.counts();
    println!(
        "summary: ok={} warn={} fail={}",
        counts.ok, counts.warn, counts.fail
    );
    match report.has_failures() {
        true => println!("result: problems found"),
        false => println!("result: ok"),
    }
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
    let startup_retry_timeout = daemon_startup_retry_timeout()?;
    let stop = StopToken::new();
    install_daemon_signal_handlers(stop.clone())?;
    let mut action_dispatcher =
        ActionDispatcher::new(args.config.gestures.clone(), DAEMON_ACTION_QUEUE_CAPACITY)?;
    eprintln!("edgepad daemon: press Ctrl+C to stop");

    let run_result = run_daemon_proxy_with_startup_retry(
        &args.config,
        &args.input_root,
        stop,
        &mut action_dispatcher,
        startup_retry_timeout,
    );
    let action_stats = action_dispatcher.shutdown();
    if let Some(summary) = run_result? {
        print_proxy_summary(&summary);
    }
    print_action_summary(&action_stats);
    Ok(())
}

fn daemon_startup_retry_timeout() -> Result<Duration, String> {
    match env::var(DAEMON_STARTUP_RETRY_ENV) {
        Ok(raw) => parse_daemon_startup_retry_timeout_ms(&raw),
        Err(env::VarError::NotPresent) => Ok(DAEMON_STARTUP_RETRY_TIMEOUT),
        Err(env::VarError::NotUnicode(_)) => {
            Err(format!("{DAEMON_STARTUP_RETRY_ENV} must be UTF-8"))
        }
    }
}

fn parse_daemon_startup_retry_timeout_ms(raw: &str) -> Result<Duration, String> {
    let millis = raw
        .parse::<u64>()
        .map_err(|_| format!("{DAEMON_STARTUP_RETRY_ENV} must be a non-negative integer"))?;
    Ok(Duration::from_millis(millis))
}

fn run_daemon_proxy_with_startup_retry(
    config: &EdgepadConfig,
    input_root: &Path,
    stop: StopToken,
    action_dispatcher: &mut ActionDispatcher,
    startup_retry_timeout: Duration,
) -> Result<Option<ProxyRunSummary>, String> {
    let started_at = Instant::now();
    let mut last_announced_device: Option<PathBuf> = None;
    let mut retry_announced = false;

    loop {
        if stop.is_stopped() {
            eprintln!("edgepad daemon: stopped before startup completed");
            return Ok(None);
        }

        let run_result = config.device.resolve(input_root).and_then(|device_path| {
            if last_announced_device.as_ref() != Some(&device_path) {
                eprintln!(
                    "edgepad daemon: device={} edge_width={:.3} gesture_bindings={}",
                    device_path.display(),
                    config.edge_width,
                    config.gestures.len()
                );
                last_announced_device = Some(device_path.clone());
            }

            run_proxy_with_gesture_handler(
                &ProxyRunConfig {
                    device_path,
                    limit: ProxyRunLimit::UntilStopped {
                        stop: stop.clone(),
                        idle_drain_timeout: DAEMON_IDLE_DRAIN_TIMEOUT,
                    },
                    edge_widths: EdgeWidths::all(config.edge_width),
                    mode: ProxyMode::UinputGrab,
                },
                action_dispatcher,
            )
        });

        match run_result {
            Ok(summary) => return Ok(Some(summary)),
            Err(err) if should_retry_daemon_startup_error(&err) => {
                let elapsed = started_at.elapsed();
                if startup_retry_timeout == Duration::ZERO || elapsed >= startup_retry_timeout {
                    return Err(format!(
                        "edgepad daemon startup timed out after {:.1}s waiting for device/uinput access; last error: {err}",
                        startup_retry_timeout.as_secs_f32()
                    ));
                }

                if !retry_announced {
                    eprintln!(
                        "edgepad daemon: startup not ready; retrying for up to {:.1}s: {err}",
                        startup_retry_timeout.as_secs_f32()
                    );
                    retry_announced = true;
                }

                let remaining = startup_retry_timeout.saturating_sub(elapsed);
                let retry_delay = DAEMON_STARTUP_RETRY_INTERVAL.min(remaining);
                if !wait_for_daemon_startup_retry(&stop, retry_delay) {
                    eprintln!("edgepad daemon: stopped before startup completed");
                    return Ok(None);
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn wait_for_daemon_startup_retry(stop: &StopToken, delay: Duration) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < delay {
        if stop.is_stopped() {
            return false;
        }
        let remaining = delay.saturating_sub(started_at.elapsed());
        thread::sleep(DAEMON_SIGNAL_POLL_INTERVAL.min(remaining));
    }
    !stop.is_stopped()
}

fn should_retry_daemon_startup_error(err: &str) -> bool {
    err.starts_with("device=auto found no event devices")
        || err.starts_with("device=auto found no readable event devices")
        || err.starts_with("device=auto found no touchpad candidates")
        || err.starts_with("failed to list ")
        || err.starts_with("failed to open device ")
        || err.starts_with("failed to read touchpad capabilities from ")
        || err.starts_with(
            "failed to create virtual touchpad via /dev/uinput before grabbing physical device",
        )
        || err.starts_with("failed to grab device ")
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

fn print_action_summary(stats: &ActionDispatcherStats) {
    println!(
        "actions: matched={} unmatched={} log={} queued={} dropped={} started={} succeeded={} failed={} worker_panics={} worker_shutdown_timeouts={}",
        stats.matched_gestures,
        stats.unmatched_gestures,
        stats.log_actions,
        stats.queued_commands,
        stats.dropped_commands,
        stats.started_commands,
        stats.succeeded_commands,
        stats.failed_commands,
        stats.worker_panics,
        stats.worker_shutdown_timeouts
    );
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

    #[test]
    fn doctor_args_default_to_auto_device_and_system_paths() {
        let args = parse_doctor_args(Vec::<String>::new().into_iter()).expect("doctor args parse");

        assert_eq!(args.device, DeviceConfig::Auto);
        assert_eq!(args.input_root, PathBuf::from("/dev/input"));
        assert_eq!(args.uinput_path, PathBuf::from("/dev/uinput"));
        assert_eq!(args.service_name, "edgepad.service");
    }

    #[test]
    fn doctor_args_accept_overrides() {
        let args = parse_doctor_args(
            [
                "--device",
                "/tmp/input/event7",
                "--input-root",
                "/tmp/input",
                "--uinput",
                "/tmp/uinput",
                "--service",
                "custom-edgepad.service",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("doctor args parse");

        assert_eq!(
            args.device,
            DeviceConfig::Path(PathBuf::from("/tmp/input/event7"))
        );
        assert_eq!(args.input_root, PathBuf::from("/tmp/input"));
        assert_eq!(args.uinput_path, PathBuf::from("/tmp/uinput"));
        assert_eq!(args.service_name, "custom-edgepad.service");
    }

    #[test]
    fn doctor_args_reject_unknown_option() {
        let err = parse_doctor_args(["--wat"].into_iter().map(String::from))
            .expect_err("unknown option should fail");

        assert_eq!(err, "unknown doctor option --wat");
    }

    #[test]
    fn daemon_startup_retry_timeout_env_parses_millis() {
        assert_eq!(
            parse_daemon_startup_retry_timeout_ms("250").expect("timeout should parse"),
            Duration::from_millis(250)
        );
        assert!(parse_daemon_startup_retry_timeout_ms("wat").is_err());
    }

    #[test]
    fn daemon_startup_retry_classifies_boot_race_errors() {
        assert!(should_retry_daemon_startup_error(
            "device=auto found no readable event devices under /dev/input"
        ));
        assert!(should_retry_daemon_startup_error(
            "failed to open device /dev/input/event7: Permission denied"
        ));
        assert!(should_retry_daemon_startup_error(
            "failed to create virtual touchpad via /dev/uinput before grabbing physical device: Permission denied"
        ));
        assert!(!should_retry_daemon_startup_error(
            "device=auto matched multiple touchpad candidates under /dev/input"
        ));
        assert!(!should_retry_daemon_startup_error(
            "touchpad is already touched on /dev/input/event7; release all fingers and retry live proxy"
        ));
    }
}

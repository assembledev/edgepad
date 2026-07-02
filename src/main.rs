use std::env;
use std::fs::{self, File};
use std::io::LineWriter;
use std::path::{Path, PathBuf};
use std::process;

use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine, GestureDirection, Zone};
use edgepad::device::{discover_device_report, format_device_line, touchpad_candidates};
use edgepad::dump::{
    capabilities_from_raw_device, write_capture_header, write_fixture_events_with_limit,
    WriteFixtureEventsResult,
};
use edgepad::replay::{parse_replay_file, replay_stats, run_frames};
use evdev::raw_stream::RawDevice;

const USAGE: &str = "usage: edgepad replay <fixture.ev> | edgepad devices [--root <input-root>] [--all] | edgepad dump --device <event-node> --out <file.ev> [--frames N]";
const DUMP_USAGE: &str = "usage: edgepad dump --device <event-node> --out <file.ev> [--frames N]";

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
        Some("devices") => {
            let args = parse_devices_args(args)?;
            devices(&args.root, args.show_all)
        }
        Some("dump") => {
            let args = parse_dump_args(args)?;
            dump(&args.device, &args.out, args.frames)
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

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--device" => device = Some(args.next().ok_or_else(|| DUMP_USAGE.to_string())?.into()),
            "--out" => out = Some(args.next().ok_or_else(|| DUMP_USAGE.to_string())?.into()),
            "--frames" => {
                let raw = args.next().ok_or_else(|| DUMP_USAGE.to_string())?;
                let parsed = raw
                    .parse::<usize>()
                    .map_err(|_| "--frames must be a positive integer".to_string())?;
                if parsed == 0 {
                    return Err("--frames must be a positive integer".to_string());
                }
                frames = Some(parsed);
            }
            _ => return Err(DUMP_USAGE.to_string()),
        }
    }

    Ok(DumpArgs {
        device: device.ok_or_else(|| DUMP_USAGE.to_string())?,
        out: out.ok_or_else(|| DUMP_USAGE.to_string())?,
        frames,
    })
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

fn dump(
    device_path: &Path,
    out_path: &Path,
    mut remaining_frames: Option<usize>,
) -> Result<(), String> {
    let requested_frames = remaining_frames;
    let mut device = RawDevice::open(device_path)
        .map_err(|err| format!("failed to open device {}: {err}", device_path.display()))?;
    let file = File::create(out_path)
        .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
    let mut writer = LineWriter::new(file);
    let capabilities = capabilities_from_raw_device(&device);
    write_capture_header(&mut writer, device_path, capabilities)
        .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
    let mut total = WriteFixtureEventsResult::default();

    loop {
        let events = device.fetch_events().map_err(|err| {
            format!(
                "failed to read events from {}: {err}",
                device_path.display()
            )
        })?;
        let result = write_fixture_events_with_limit(&mut writer, events, &mut remaining_frames)
            .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
        total.add(result);
        if total.reached_limit {
            print_dump_summary(device_path, out_path, capabilities, requested_frames, total);
            return Ok(());
        }
    }
}

fn print_dump_summary(
    device_path: &Path,
    out_path: &Path,
    capabilities: Option<Capabilities>,
    requested_frames: Option<usize>,
    stats: WriteFixtureEventsResult,
) {
    println!("wrote: {}", out_path.display());
    println!("device: {}", device_path.display());
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
    println!("next: edgepad replay {}", out_path.display());
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
            "diagnosis_hint: start dump before touching the pad, perform one gesture, lift the finger before capture stops, or increase --frames"
        );
    } else if stats.tracking_starts > stats.tracking_ends {
        println!(
            "diagnosis: capture ended with active contact(s); lift fingers before capture stops or increase --frames"
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

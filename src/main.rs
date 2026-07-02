use std::env;
use std::fs;
use std::path::Path;
use std::process;

use edgepad::core::{AxisRange, Capabilities, EdgeWidths, Engine, GestureDirection, Zone};
use edgepad::device::{discover_devices, format_device_line};
use edgepad::replay::{parse_frames, run_frames};

const USAGE: &str = "usage: edgepad replay <fixture.ev> | edgepad devices [--root <input-root>]";

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
            let root = parse_devices_root(args)?;
            devices(&root)
        }
        _ => Err(USAGE.to_string()),
    }
}

fn parse_devices_root(
    mut args: impl Iterator<Item = String>,
) -> Result<std::path::PathBuf, String> {
    let mut root = std::path::PathBuf::from("/dev/input");

    while let Some(arg) = args.next() {
        if arg != "--root" {
            return Err(USAGE.to_string());
        }
        root = args.next().ok_or_else(|| USAGE.to_string())?.into();
    }

    Ok(root)
}

fn devices(root: &Path) -> Result<(), String> {
    let summaries = discover_devices(root)
        .map_err(|err| format!("failed to list {}: {err}", root.display()))?;

    if summaries.is_empty() {
        println!("no readable event devices found under {}", root.display());
        return Ok(());
    }

    for summary in summaries {
        println!("{}", format_device_line(&summary));
    }

    Ok(())
}

fn replay(path: &Path) -> Result<(), String> {
    let input = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let frames = parse_frames(&input).map_err(|err| format!("parse failed: {err:?}"))?;

    // Temporary fixture defaults. Real dump/capture support will put device capabilities
    // in the capture path instead of pretending every touchpad is 1000x700.
    let mut engine = Engine::new(
        Capabilities {
            slot_min: 0,
            slot_max: 9,
            x: AxisRange { min: 0, max: 1000 },
            y: AxisRange { min: 0, max: 700 },
        },
        EdgeWidths::all(0.10),
    );

    let outputs =
        run_frames(&mut engine, &frames).map_err(|err| format!("replay failed: {err:?}"))?;
    let passthrough_events = outputs
        .iter()
        .map(|output| output.passthrough.len())
        .sum::<usize>();
    let gestures = outputs
        .iter()
        .flat_map(|output| output.gestures.iter())
        .collect::<Vec<_>>();
    let resync_required = outputs.iter().any(|output| output.resync_required);

    println!("frames: {}", frames.len());
    println!("passthrough_events: {passthrough_events}");
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

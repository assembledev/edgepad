//! Hardware-free replay commands. Parsed replay exercises the recognizer;
//! raw replay also routes events and composes the virtual device's output.
//! Both use the same profile loader and presentation labels as live commands.

use std::fs;

use edgepad::core::{AxisRange, Capabilities, Engine};
use edgepad::raw::{
    parse_raw_dump_file, route_raw_frame, route_recognition_deadline, write_raw_output_frame,
    RawOutputComposer, RawOutputSink, RecordingRawOutputSink,
};
use edgepad::replay::{parse_replay_file, replay_stats, run_frames};

use super::cli::ReplayArgs;
use super::presentation::{
    direction_name, print_recognition_profile, slider_direction_name, zone_name,
};
use super::profile::load_recognition_profile;

fn default_capabilities() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 9,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

pub(super) fn replay(args: &ReplayArgs) -> Result<(), String> {
    let profile = load_recognition_profile(&args.profile)?;
    print_recognition_profile(&profile);
    let input = fs::read_to_string(&args.path)
        .map_err(|err| format!("failed to read {}: {err}", args.path.display()))?;
    let replay = parse_replay_file(&input).map_err(|err| format!("parse failed: {err:?}"))?;

    let (capability_source, capabilities) = match replay.capabilities {
        Some(capabilities) => ("metadata", capabilities),
        None => ("defaults", default_capabilities()),
    };
    let mut engine = Engine::with_options(
        capabilities,
        profile.edge_widths,
        profile.slider_specs.clone(),
        profile.engine_options,
    );

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
    let slider_steps = outputs
        .iter()
        .flat_map(|output| output.slider_steps.iter())
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
        "events: total={} slot={} tracking_start={} tracking_end={} x={} y={} syn_dropped={} tool_type={}",
        stats.total_events,
        stats.slot_events,
        stats.tracking_starts,
        stats.tracking_ends,
        stats.x_events,
        stats.y_events,
        stats.syn_dropped_events,
        stats.tool_type_events
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
    let slider_step_count = slider_steps.len();
    println!("slider_steps: {slider_step_count}");
    for step in slider_steps {
        println!(
            "slider_step slot={} tracking_id={} zone={} direction={}",
            step.slot,
            step.tracking_id,
            zone_name(step.zone),
            slider_direction_name(step.direction)
        );
    }
    println!("resync_required: {resync_required}");
    print_replay_diagnosis(&stats, passthrough_events, gesture_count, slider_step_count);

    Ok(())
}

pub(super) fn replay_raw(args: &ReplayArgs) -> Result<(), String> {
    let profile = load_recognition_profile(&args.profile)?;
    print_recognition_profile(&profile);
    let input = fs::read_to_string(&args.path)
        .map_err(|err| format!("failed to read {}: {err}", args.path.display()))?;
    let raw_dump = parse_raw_dump_file(&input).map_err(|err| format!("parse failed: {err:?}"))?;

    let (capability_source, capabilities) = match raw_dump.capabilities {
        Some(capabilities) => ("metadata", capabilities),
        None => ("defaults", default_capabilities()),
    };
    let mut engine = Engine::with_options(
        capabilities,
        profile.edge_widths,
        profile.slider_specs.clone(),
        profile.engine_options,
    );
    let mut composer = RawOutputComposer::new(capabilities);

    let raw_events = raw_dump
        .frames
        .iter()
        .map(|frame| frame.events.len())
        .sum::<usize>();
    let mut recognizer_passthrough_events = 0;
    let mut gestures = Vec::new();
    let mut slider_steps = Vec::new();
    let mut resync_required = false;
    let mut sink = RecordingRawOutputSink::default();

    for frame in &raw_dump.frames {
        let routed = route_raw_frame(&mut engine, frame)
            .map_err(|err| format!("raw replay failed: {err:?}"))?;
        recognizer_passthrough_events += routed.passthrough.len();
        resync_required |= routed.resync_required;
        gestures.extend(routed.gestures.iter().copied());
        slider_steps.extend(routed.slider_steps.iter().copied());

        write_raw_output_frame(&mut composer, &routed, &mut sink)
            .map_err(|err| format!("raw output write failed: {err:?}"))?;
    }

    if let Some(deadline) = engine.next_deadline() {
        let routed = route_recognition_deadline(&mut engine, deadline);
        gestures.extend(routed.gestures);
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
    println!("slider_steps: {}", slider_steps.len());
    for step in slider_steps {
        println!(
            "slider_step slot={} tracking_id={} zone={} direction={}",
            step.slot,
            step.tracking_id,
            zone_name(step.zone),
            slider_direction_name(step.direction)
        );
    }
    println!("resync_required: {resync_required}");

    Ok(())
}

fn print_replay_diagnosis(
    stats: &edgepad::replay::ReplayStats,
    passthrough_events: usize,
    gestures: usize,
    slider_steps: usize,
) {
    if stats.tracking_starts == 0 && (stats.x_events > 0 || stats.y_events > 0) {
        println!(
            "diagnosis: no contact starts found; capture likely began after a finger was already down or no new touch started"
        );
        println!(
            "diagnosis_hint: start dump before touching the pad; frame-limited dump waits for active contacts to be released"
        );
    } else if stats.tracking_starts > stats.tracking_ends {
        println!(
            "diagnosis: capture ended with active contact(s); input is incomplete or truncated"
        );
        println!("diagnosis_hint: capture again and release all contacts before stopping");
    } else if stats.tracking_starts == 0 && stats.total_events == 0 {
        println!("diagnosis: no replay-relevant touch events found");
    } else if passthrough_events == 0 && gestures == 0 && slider_steps == 0 {
        println!(
            "diagnosis: complete contacts were parsed, but current recognizer produced no passthrough events, gestures, or slider steps"
        );
    }
}

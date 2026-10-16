//! Shared recognizer, proxy, and action summaries for command output.
//! Formatting stays here so daemon and diagnostic commands use the same labels.

use edgepad::actions::ActionDispatcherStats;
use edgepad::core::{EdgeWidths, GestureDirection, SliderDirection, Zone};
use edgepad::proxy::{ProxyMode, ProxyRunSummary};

use super::profile::RecognitionProfile;

pub(super) fn print_action_summary(stats: &ActionDispatcherStats) {
    println!(
        "actions: matched={} unmatched={} slider_matched={} slider_unmatched={} log={} queued={} dropped={} started={} succeeded={} failed={} worker_panics={} worker_shutdown_timeouts={}",
        stats.matched_gestures,
        stats.unmatched_gestures,
        stats.matched_slider_steps,
        stats.unmatched_slider_steps,
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

pub(super) fn print_proxy_summary(summary: &ProxyRunSummary) {
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
    println!("edge_widths: {}", edge_widths_label(summary.edge_widths));
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
    println!("recognition_reloads: {}", stats.recognition_reloads);
    println!("gestures: {}", stats.gesture_count());
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
    println!("slider_steps: {}", stats.slider_step_count());
    if !stats.slider_step_counts.is_empty() {
        println!("slider_step_counts:");
        for (key, count) in &stats.slider_step_counts {
            println!(
                "  zone={} direction={} count={count}",
                zone_name(key.zone),
                slider_direction_name(key.direction)
            );
        }
    }
    for step in &stats.slider_steps {
        println!(
            "slider_step slot={} tracking_id={} zone={} direction={}",
            step.slot,
            step.tracking_id,
            zone_name(step.zone),
            slider_direction_name(step.direction)
        );
    }
    println!("resync_required: {}", stats.resync_required);
    match summary.mode {
        ProxyMode::DryRun => println!("output: not emitted (--dry-run)"),
        ProxyMode::UinputGrab => println!("output: emitted (--uinput --grab)"),
    }
}

pub(super) fn edge_widths_label(widths: EdgeWidths) -> String {
    format!(
        "left={:.3} right={:.3} top={:.3} bottom={:.3}",
        widths.left, widths.right, widths.top, widths.bottom
    )
}

pub(super) fn print_recognition_profile(profile: &RecognitionProfile) {
    println!("profile: {}", profile.source_label());
    println!(
        "profile_settings: edge_widths={} tap_min_duration_ms={} tap_max_duration_ms={} double_tap_timeout_ms={} double_tap_max_distance={:.3} swipe_min_distance={:.3} sliders={} palm_rejection={}",
        edge_widths_label(profile.edge_widths),
        profile.engine_options.tap_min_duration.as_millis(),
        profile.engine_options.tap_max_duration.as_millis(),
        profile.engine_options.double_tap_timeout.as_millis(),
        profile.engine_options.double_tap_max_distance,
        profile.engine_options.swipe_min_distance,
        profile.slider_specs.len(),
        profile.engine_options.palm_rejection
    );
}

pub(super) fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

pub(super) fn direction_name(direction: GestureDirection) -> &'static str {
    match direction {
        GestureDirection::Up => "up",
        GestureDirection::Down => "down",
        GestureDirection::Left => "left",
        GestureDirection::Right => "right",
        GestureDirection::Tap => "tap",
        GestureDirection::DoubleTap => "double-tap",
    }
}

pub(super) fn slider_direction_name(direction: SliderDirection) -> &'static str {
    match direction {
        SliderDirection::Up => "up",
        SliderDirection::Down => "down",
        SliderDirection::Left => "left",
        SliderDirection::Right => "right",
    }
}

#[cfg(test)]
mod tests;

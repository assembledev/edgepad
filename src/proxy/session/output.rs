#[cfg(test)]
use super::super::NoopGestureHandler;
use super::super::{GestureHandler, ProxyRuntimeStats};
use crate::core::{Engine, ResyncContact};
use crate::raw::{
    extract_core_events, route_raw_frame, route_recognition_deadline, route_resync_contacts,
    RawEvent, RawFrame, RawOutputComposer, RawOutputSink, EV_KEY,
};
use std::time::Duration;

pub(super) fn process_proxy_recognition_deadline<S, H>(
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

#[cfg(test)]
pub(super) fn process_proxy_frame<S>(
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

pub(super) fn process_proxy_frame_with_handler<S, H>(
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

pub(super) fn process_proxy_resync_contacts<S, H>(
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
        stats.record_gesture(gesture);
        handler.handle_gesture(gesture);
    }
    for step in routed.slider_steps.iter().copied() {
        stats.record_slider_step(step);
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

pub(super) fn finish_proxy_output<S>(
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

#[cfg(test)]
mod tests;

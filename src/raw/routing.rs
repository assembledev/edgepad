use std::time::Duration;

use crate::core::{Engine, Event, Gesture, ResyncContact, SliderStep, SlotError};

use super::events::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutedRawFrame {
    pub passthrough: Vec<RawEvent>,
    pub physical_buttons: Vec<RawEvent>,
    pub gestures: Vec<Gesture>,
    pub slider_steps: Vec<SliderStep>,
    pub resync_required: bool,
}

pub fn extract_core_events(frame: &RawFrame) -> Vec<Event> {
    frame
        .events
        .iter()
        .filter_map(|event| core_event_for_raw_event(*event))
        .collect()
}

pub fn route_raw_frame(engine: &mut Engine, frame: &RawFrame) -> Result<RoutedRawFrame, SlotError> {
    let physical_buttons = frame
        .events
        .iter()
        .copied()
        .filter(|event| event.kind == EV_KEY && is_pointer_button_code(event.code))
        .collect::<Vec<_>>();
    let mut promoted_contacts = Vec::new();
    let mut canceled_tap_gestures = Vec::new();
    if physical_buttons.iter().any(|event| event.value != 0) {
        canceled_tap_gestures = engine.interrupt_tap_sequence();
    }
    // Treat a press as held for this entire frame, regardless of event ordering.
    // On a buttonpad, existing and newly starting contacts must pass through for
    // the click. Apply releases only after processing the frame's contacts.
    for event in physical_buttons.iter().filter(|event| event.value != 0) {
        promoted_contacts.extend(engine.update_physical_button(event.code, true));
    }

    let core_events = extract_core_events(frame);
    let output = match frame.timestamp {
        Some(timestamp) => engine.process_frame_at(&core_events, timestamp)?,
        None => engine.process_frame(&core_events)?,
    };
    for event in physical_buttons.iter().filter(|event| event.value == 0) {
        engine.update_physical_button(event.code, false);
    }
    promoted_contacts.extend(output.passthrough.iter().copied());
    let passthrough = raw_passthrough_events(promoted_contacts);

    canceled_tap_gestures.extend(output.gestures);
    Ok(RoutedRawFrame {
        passthrough,
        physical_buttons,
        gestures: canceled_tap_gestures,
        slider_steps: output.slider_steps,
        resync_required: output.resync_required,
    })
}

pub fn route_recognition_deadline(engine: &mut Engine, timestamp: Duration) -> RoutedRawFrame {
    let output = engine.advance_time(timestamp);
    RoutedRawFrame {
        passthrough: Vec::new(),
        physical_buttons: Vec::new(),
        gestures: output.gestures,
        slider_steps: Vec::new(),
        resync_required: false,
    }
}

pub fn route_resync_contacts(
    engine: &mut Engine,
    contacts: &[ResyncContact],
) -> Result<RoutedRawFrame, SlotError> {
    let output = engine.restore_passthrough_contacts(contacts)?;
    Ok(RoutedRawFrame {
        passthrough: raw_passthrough_events(output.passthrough),
        physical_buttons: Vec::new(),
        gestures: Vec::new(),
        slider_steps: Vec::new(),
        resync_required: false,
    })
}

fn raw_passthrough_events(core_passthrough: Vec<Event>) -> Vec<RawEvent> {
    core_passthrough
        .into_iter()
        .filter_map(raw_event_for_core_event)
        .collect()
}

fn raw_event_for_core_event(event: Event) -> Option<RawEvent> {
    match event {
        Event::Slot(slot) => Some(RawEvent::abs_mt_slot(slot)),
        Event::TrackingId(tracking_id) => Some(RawEvent::abs_mt_tracking_id(tracking_id)),
        Event::X(x) => Some(RawEvent::abs_mt_position_x(x)),
        Event::Y(y) => Some(RawEvent::abs_mt_position_y(y)),
        Event::ToolType(tool_type) => Some(RawEvent::new(EV_ABS, ABS_MT_TOOL_TYPE, tool_type)),
        Event::SynDropped => None,
    }
}

fn core_event_for_raw_event(event: RawEvent) -> Option<Event> {
    match (event.kind, event.code) {
        (EV_ABS, ABS_MT_SLOT) => Some(Event::slot(event.value)),
        (EV_ABS, ABS_MT_TRACKING_ID) => Some(Event::tracking_id(event.value)),
        (EV_ABS, ABS_MT_POSITION_X) => Some(Event::x(event.value)),
        (EV_ABS, ABS_MT_POSITION_Y) => Some(Event::y(event.value)),
        (EV_ABS, ABS_MT_TOOL_TYPE) => Some(Event::tool_type(event.value)),
        (EV_SYN, SYN_DROPPED) => Some(Event::syn_dropped()),
        _ => None,
    }
}

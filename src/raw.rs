use crate::core::{Engine, Event, Gesture, SlotError};

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;

pub const SYN_REPORT: u16 = 0x00;
pub const SYN_DROPPED: u16 = 0x03;

pub const BTN_TOUCH: u16 = 0x14a;

pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_MT_SLOT: u16 = 0x2f;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl RawEvent {
    pub const fn new(kind: u16, code: u16, value: i32) -> Self {
        Self { kind, code, value }
    }

    pub const fn syn_report() -> Self {
        Self::new(EV_SYN, SYN_REPORT, 0)
    }

    pub const fn syn_dropped() -> Self {
        Self::new(EV_SYN, SYN_DROPPED, 0)
    }

    pub const fn btn_touch(pressed: bool) -> Self {
        Self::new(EV_KEY, BTN_TOUCH, pressed as i32)
    }

    pub const fn abs_x(value: i32) -> Self {
        Self::new(EV_ABS, ABS_X, value)
    }

    pub const fn abs_y(value: i32) -> Self {
        Self::new(EV_ABS, ABS_Y, value)
    }

    pub const fn abs_mt_slot(slot: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_SLOT, slot)
    }

    pub const fn abs_mt_tracking_id(tracking_id: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_TRACKING_ID, tracking_id)
    }

    pub const fn abs_mt_position_x(x: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_POSITION_X, x)
    }

    pub const fn abs_mt_position_y(y: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_POSITION_Y, y)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawFrame {
    pub events: Vec<RawEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutedRawFrame {
    pub passthrough: Vec<RawEvent>,
    pub gestures: Vec<Gesture>,
    pub resync_required: bool,
}

impl RawFrame {
    pub fn new(events: Vec<RawEvent>) -> Self {
        Self { events }
    }
}

pub fn extract_core_events(frame: &RawFrame) -> Vec<Event> {
    frame
        .events
        .iter()
        .filter_map(|event| core_event_for_raw_event(*event))
        .collect()
}

pub fn route_raw_frame(engine: &mut Engine, frame: &RawFrame) -> Result<RoutedRawFrame, SlotError> {
    let core_events = extract_core_events(frame);
    let output = engine.process_frame(&core_events)?;
    let passthrough = raw_passthrough_events_for_core_passthrough(frame, &output.passthrough);

    Ok(RoutedRawFrame {
        passthrough,
        gestures: output.gestures,
        resync_required: output.resync_required,
    })
}

fn raw_passthrough_events_for_core_passthrough(
    frame: &RawFrame,
    core_passthrough: &[Event],
) -> Vec<RawEvent> {
    let mut raw_passthrough = Vec::new();
    let mut expected = core_passthrough.iter();
    let mut next_expected = expected.next();

    for raw_event in &frame.events {
        let Some(expected_event) = next_expected else {
            break;
        };
        if core_event_for_raw_event(*raw_event).is_some_and(|event| event == *expected_event) {
            raw_passthrough.push(*raw_event);
            next_expected = expected.next();
        }
    }

    raw_passthrough
}

fn core_event_for_raw_event(event: RawEvent) -> Option<Event> {
    match (event.kind, event.code) {
        (EV_ABS, ABS_MT_SLOT) => Some(Event::slot(event.value)),
        (EV_ABS, ABS_MT_TRACKING_ID) => Some(Event::tracking_id(event.value)),
        (EV_ABS, ABS_MT_POSITION_X) => Some(Event::x(event.value)),
        (EV_ABS, ABS_MT_POSITION_Y) => Some(Event::y(event.value)),
        (EV_SYN, SYN_DROPPED) => Some(Event::syn_dropped()),
        _ => None,
    }
}

use crate::core::{AxisRange, Capabilities, Engine, Event, Gesture, SlotError};

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const EV_MSC: u16 = 0x04;

pub const SYN_REPORT: u16 = 0x00;
pub const SYN_DROPPED: u16 = 0x03;
pub const MSC_TIMESTAMP: u16 = 0x05;

pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;
pub const BTN_TOOL_FINGER: u16 = 0x145;
pub const BTN_TOOL_QUINTTAP: u16 = 0x148;
pub const BTN_TOUCH: u16 = 0x14a;
pub const BTN_TOOL_DOUBLETAP: u16 = 0x14d;
pub const BTN_TOOL_TRIPLETAP: u16 = 0x14e;
pub const BTN_TOOL_QUADTAP: u16 = 0x14f;

pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_MT_SLOT: u16 = 0x2f;
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
pub const ABS_MT_TOUCH_MINOR: u16 = 0x31;
pub const ABS_MT_WIDTH_MAJOR: u16 = 0x32;
pub const ABS_MT_WIDTH_MINOR: u16 = 0x33;
pub const ABS_MT_ORIENTATION: u16 = 0x34;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TOOL_TYPE: u16 = 0x37;
pub const ABS_MT_BLOB_ID: u16 = 0x38;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
pub const ABS_MT_PRESSURE: u16 = 0x3a;
pub const ABS_MT_DISTANCE: u16 = 0x3b;
pub const ABS_MT_TOOL_X: u16 = 0x3c;
pub const ABS_MT_TOOL_Y: u16 = 0x3d;

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

    pub const fn msc_timestamp(value: i32) -> Self {
        Self::new(EV_MSC, MSC_TIMESTAMP, value)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawParseError {
    MissingField {
        line: usize,
        field: &'static str,
    },
    ExtraField {
        line: usize,
    },
    InvalidInteger {
        line: usize,
        field: &'static str,
        value: String,
    },
    UnknownEventType {
        line: usize,
        name: String,
    },
    UnknownCode {
        line: usize,
        event_type: String,
        code: String,
    },
    InvalidMetadataRange {
        line: usize,
        field: &'static str,
        value: String,
    },
    MissingMetadataField {
        field: &'static str,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawFrame {
    pub events: Vec<RawEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawDumpFile {
    pub capabilities: Option<Capabilities>,
    pub frames: Vec<RawFrame>,
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

pub fn parse_raw_dump_file(input: &str) -> Result<RawDumpFile, RawParseError> {
    Ok(RawDumpFile {
        capabilities: parse_capabilities_metadata(input)?,
        frames: parse_raw_frames(input)?,
    })
}

#[derive(Default)]
struct CapabilityMetadata {
    slots: Option<AxisRange>,
    x: Option<AxisRange>,
    y: Option<AxisRange>,
    saw_any: bool,
}

fn parse_capabilities_metadata(input: &str) -> Result<Option<Capabilities>, RawParseError> {
    let mut metadata = CapabilityMetadata::default();

    for (index, raw_line) in input.lines().enumerate() {
        let line_number = index + 1;
        let Some(comment) = raw_line.trim_start().strip_prefix('#') else {
            continue;
        };
        let Some((name, value)) = comment.trim().split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();

        match name {
            "slots" => {
                metadata.saw_any = true;
                metadata.slots = Some(parse_metadata_range(line_number, "slots", value)?);
            }
            "x" => {
                metadata.saw_any = true;
                metadata.x = Some(parse_metadata_range(line_number, "x", value)?);
            }
            "y" => {
                metadata.saw_any = true;
                metadata.y = Some(parse_metadata_range(line_number, "y", value)?);
            }
            _ => {}
        }
    }

    if !metadata.saw_any {
        return Ok(None);
    }

    let slots = metadata
        .slots
        .ok_or(RawParseError::MissingMetadataField { field: "slots" })?;
    let x = metadata
        .x
        .ok_or(RawParseError::MissingMetadataField { field: "x" })?;
    let y = metadata
        .y
        .ok_or(RawParseError::MissingMetadataField { field: "y" })?;

    Ok(Some(Capabilities {
        slot_min: slots.min,
        slot_max: slots.max,
        x,
        y,
    }))
}

fn parse_metadata_range(
    line: usize,
    field: &'static str,
    value: &str,
) -> Result<AxisRange, RawParseError> {
    let Some((min, max)) = value.split_once("..=") else {
        return Err(RawParseError::InvalidMetadataRange {
            line,
            field,
            value: value.to_string(),
        });
    };
    let min = min
        .trim()
        .parse::<i32>()
        .map_err(|_| RawParseError::InvalidMetadataRange {
            line,
            field,
            value: value.to_string(),
        })?;
    let max = max
        .trim()
        .parse::<i32>()
        .map_err(|_| RawParseError::InvalidMetadataRange {
            line,
            field,
            value: value.to_string(),
        })?;

    Ok(AxisRange { min, max })
}

pub fn parse_raw_frames(input: &str) -> Result<Vec<RawFrame>, RawParseError> {
    let mut frames = Vec::new();
    let mut current = Vec::new();

    for (index, raw_line) in input.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line
            .split_once('#')
            .map_or(raw_line, |(before_comment, _)| before_comment)
            .trim();

        if line.is_empty() {
            continue;
        }

        let event = parse_raw_event_line(line_number, line)?;
        match (event.kind, event.code) {
            (EV_SYN, SYN_REPORT) => {
                if !current.is_empty() {
                    frames.push(RawFrame::new(std::mem::take(&mut current)));
                }
            }
            (EV_SYN, SYN_DROPPED) => {
                if !current.is_empty() {
                    frames.push(RawFrame::new(std::mem::take(&mut current)));
                }
                frames.push(RawFrame::new(vec![event]));
            }
            _ => current.push(event),
        }
    }

    if !current.is_empty() {
        frames.push(RawFrame::new(current));
    }

    Ok(frames)
}

fn parse_raw_event_line(line: usize, raw_line: &str) -> Result<RawEvent, RawParseError> {
    let mut parts = raw_line.split_whitespace();
    let event_type = parts.next().ok_or(RawParseError::MissingField {
        line,
        field: "event_type",
    })?;
    let code = parts.next().ok_or(RawParseError::MissingField {
        line,
        field: "code",
    })?;
    let value = parts.next().ok_or(RawParseError::MissingField {
        line,
        field: "value",
    })?;

    if parts.next().is_some() {
        return Err(RawParseError::ExtraField { line });
    }

    let kind = parse_event_type(line, event_type)?;
    let code = parse_event_code(line, event_type, kind, code)?;
    let value = parse_i32_field(line, "value", value)?;

    Ok(RawEvent::new(kind, code, value))
}

fn parse_event_type(line: usize, name: &str) -> Result<u16, RawParseError> {
    match name {
        "EV_SYN" => Ok(EV_SYN),
        "EV_KEY" => Ok(EV_KEY),
        "EV_ABS" => Ok(EV_ABS),
        "EV_MSC" => Ok(EV_MSC),
        _ => {
            let Some(raw) = name.strip_prefix("EV_") else {
                return Err(RawParseError::UnknownEventType {
                    line,
                    name: name.to_string(),
                });
            };
            parse_u16_field(line, "event_type", raw).map_err(|_| RawParseError::UnknownEventType {
                line,
                name: name.to_string(),
            })
        }
    }
}

fn parse_event_code(
    line: usize,
    event_type_name: &str,
    event_type: u16,
    code: &str,
) -> Result<u16, RawParseError> {
    if let Ok(value) = parse_u16_field(line, "code", code) {
        return Ok(value);
    }

    let value = match event_type {
        EV_SYN => synchronization_code_for_name(code),
        EV_KEY => key_code_for_name(code),
        EV_ABS => absolute_axis_code_for_name(code),
        EV_MSC => misc_code_for_name(code),
        _ => None,
    };

    value.ok_or_else(|| RawParseError::UnknownCode {
        line,
        event_type: event_type_name.to_string(),
        code: code.to_string(),
    })
}

fn parse_u16_field(line: usize, field: &'static str, value: &str) -> Result<u16, RawParseError> {
    value
        .parse::<u16>()
        .map_err(|_| RawParseError::InvalidInteger {
            line,
            field,
            value: value.to_string(),
        })
}

fn parse_i32_field(line: usize, field: &'static str, value: &str) -> Result<i32, RawParseError> {
    value
        .parse::<i32>()
        .map_err(|_| RawParseError::InvalidInteger {
            line,
            field,
            value: value.to_string(),
        })
}

fn synchronization_code_for_name(name: &str) -> Option<u16> {
    match name {
        "SYN_REPORT" => Some(SYN_REPORT),
        "SYN_DROPPED" => Some(SYN_DROPPED),
        _ => None,
    }
}

fn misc_code_for_name(name: &str) -> Option<u16> {
    match name {
        "MSC_TIMESTAMP" => Some(MSC_TIMESTAMP),
        _ => None,
    }
}

fn key_code_for_name(name: &str) -> Option<u16> {
    match name {
        "BTN_LEFT" => Some(BTN_LEFT),
        "BTN_RIGHT" => Some(BTN_RIGHT),
        "BTN_MIDDLE" => Some(BTN_MIDDLE),
        "BTN_SIDE" => Some(BTN_SIDE),
        "BTN_EXTRA" => Some(BTN_EXTRA),
        "BTN_TOOL_FINGER" => Some(BTN_TOOL_FINGER),
        "BTN_TOOL_QUINTTAP" => Some(BTN_TOOL_QUINTTAP),
        "BTN_TOUCH" => Some(BTN_TOUCH),
        "BTN_TOOL_DOUBLETAP" => Some(BTN_TOOL_DOUBLETAP),
        "BTN_TOOL_TRIPLETAP" => Some(BTN_TOOL_TRIPLETAP),
        "BTN_TOOL_QUADTAP" => Some(BTN_TOOL_QUADTAP),
        _ => None,
    }
}

fn absolute_axis_code_for_name(name: &str) -> Option<u16> {
    match name {
        "ABS_X" => Some(ABS_X),
        "ABS_Y" => Some(ABS_Y),
        "ABS_MT_SLOT" => Some(ABS_MT_SLOT),
        "ABS_MT_TOUCH_MAJOR" => Some(ABS_MT_TOUCH_MAJOR),
        "ABS_MT_TOUCH_MINOR" => Some(ABS_MT_TOUCH_MINOR),
        "ABS_MT_WIDTH_MAJOR" => Some(ABS_MT_WIDTH_MAJOR),
        "ABS_MT_WIDTH_MINOR" => Some(ABS_MT_WIDTH_MINOR),
        "ABS_MT_ORIENTATION" => Some(ABS_MT_ORIENTATION),
        "ABS_MT_POSITION_X" => Some(ABS_MT_POSITION_X),
        "ABS_MT_POSITION_Y" => Some(ABS_MT_POSITION_Y),
        "ABS_MT_TOOL_TYPE" => Some(ABS_MT_TOOL_TYPE),
        "ABS_MT_BLOB_ID" => Some(ABS_MT_BLOB_ID),
        "ABS_MT_TRACKING_ID" => Some(ABS_MT_TRACKING_ID),
        "ABS_MT_PRESSURE" => Some(ABS_MT_PRESSURE),
        "ABS_MT_DISTANCE" => Some(ABS_MT_DISTANCE),
        "ABS_MT_TOOL_X" => Some(ABS_MT_TOOL_X),
        "ABS_MT_TOOL_Y" => Some(ABS_MT_TOOL_Y),
        _ => None,
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

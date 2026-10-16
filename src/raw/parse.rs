use std::time::Duration;

use crate::capture_metadata::{self, MetadataError};
use crate::core::Capabilities;

use super::events::*;

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
    InvalidTimestamp {
        line: usize,
        value: String,
    },
    NonMonotonicTimestamp {
        line: usize,
        previous_us: u64,
        current_us: u64,
    },
    UnterminatedFrame {
        line: usize,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawDumpFile {
    pub capabilities: Option<Capabilities>,
    pub frames: Vec<RawFrame>,
}

pub fn parse_raw_dump_file(input: &str) -> Result<RawDumpFile, RawParseError> {
    Ok(RawDumpFile {
        capabilities: capture_metadata::parse_capabilities(input).map_err(|error| match error {
            MetadataError::InvalidRange { line, field, value } => {
                RawParseError::InvalidMetadataRange { line, field, value }
            }
            MetadataError::MissingField { field } => RawParseError::MissingMetadataField { field },
        })?,
        frames: parse_raw_frames(input)?,
    })
}

pub fn parse_raw_frames(input: &str) -> Result<Vec<RawFrame>, RawParseError> {
    let mut frames = Vec::new();
    let mut current = Vec::new();
    let mut last_timestamp = None;

    for (index, raw_line) in input.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line
            .split_once('#')
            .map_or(raw_line, |(before_comment, _)| before_comment)
            .trim();

        if line.is_empty() {
            continue;
        }

        let (event, timestamp) = parse_raw_event_line(line_number, line)?;
        match (event.kind, event.code) {
            (EV_SYN, SYN_REPORT) => {
                let timestamp = parse_frame_timestamp(line_number, timestamp)?;
                validate_frame_timestamp(line_number, timestamp, &mut last_timestamp)?;
                if !current.is_empty() {
                    frames.push(RawFrame::new_at(std::mem::take(&mut current), timestamp));
                }
            }
            (EV_SYN, SYN_DROPPED) => {
                let timestamp = parse_frame_timestamp(line_number, timestamp)?;
                validate_frame_timestamp(line_number, timestamp, &mut last_timestamp)?;
                current.clear();
                frames.push(RawFrame::new_at(vec![event], timestamp));
            }
            _ => {
                if timestamp.is_some() {
                    return Err(RawParseError::ExtraField { line: line_number });
                }
                current.push(event);
            }
        }
    }

    if !current.is_empty() {
        return Err(RawParseError::UnterminatedFrame {
            line: input.lines().count().max(1),
        });
    }

    Ok(frames)
}

fn parse_raw_event_line(
    line: usize,
    raw_line: &str,
) -> Result<(RawEvent, Option<&str>), RawParseError> {
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

    let timestamp = parts.next();
    if parts.next().is_some() {
        return Err(RawParseError::ExtraField { line });
    }

    let kind = parse_event_type(line, event_type)?;
    let code = parse_event_code(line, event_type, kind, code)?;
    let value = parse_i32_field(line, "value", value)?;

    Ok((RawEvent::new(kind, code, value), timestamp))
}

fn parse_frame_timestamp(line: usize, value: Option<&str>) -> Result<Duration, RawParseError> {
    let value = value.ok_or(RawParseError::MissingField {
        line,
        field: "timestamp_us",
    })?;
    let micros = value
        .parse::<u64>()
        .map_err(|_| RawParseError::InvalidTimestamp {
            line,
            value: value.to_string(),
        })?;
    Ok(Duration::from_micros(micros))
}

fn validate_frame_timestamp(
    line: usize,
    timestamp: Duration,
    last_timestamp: &mut Option<Duration>,
) -> Result<(), RawParseError> {
    if let Some(previous) = *last_timestamp {
        if timestamp < previous {
            return Err(RawParseError::NonMonotonicTimestamp {
                line,
                previous_us: previous.as_micros() as u64,
                current_us: timestamp.as_micros() as u64,
            });
        }
    }
    *last_timestamp = Some(timestamp);
    Ok(())
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

//! Parse timestamped replay fixtures and run them through the recognizer.

use std::time::Duration;

use crate::capture_metadata::{self, MetadataError};
use crate::core::{Capabilities, Engine, Event, FrameOutput, SlotError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    UnknownEvent {
        line: usize,
        name: String,
    },
    MissingValue {
        line: usize,
        name: String,
    },
    InvalidValue {
        line: usize,
        name: String,
        value: String,
    },
    InvalidMetadata {
        line: usize,
        name: String,
        value: String,
    },
    MissingMetadataField {
        field: &'static str,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayFrame {
    pub events: Vec<Event>,
    pub timestamp: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayFile {
    pub capabilities: Option<Capabilities>,
    pub frames: Vec<ReplayFrame>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayStats {
    pub total_events: usize,
    pub slot_events: usize,
    pub tracking_starts: usize,
    pub tracking_ends: usize,
    pub x_events: usize,
    pub y_events: usize,
    pub tool_type_events: usize,
    pub syn_dropped_events: usize,
}

pub fn replay_stats(frames: &[ReplayFrame]) -> ReplayStats {
    let mut stats = ReplayStats::default();

    for frame in frames {
        for event in &frame.events {
            stats.total_events += 1;
            match event {
                Event::Slot(_) => stats.slot_events += 1,
                Event::TrackingId(id) if *id >= 0 => stats.tracking_starts += 1,
                Event::TrackingId(_) => stats.tracking_ends += 1,
                Event::X(_) => stats.x_events += 1,
                Event::Y(_) => stats.y_events += 1,
                Event::ToolType(_) => stats.tool_type_events += 1,
                Event::SynDropped => stats.syn_dropped_events += 1,
            }
        }
    }

    stats
}

pub fn parse_replay_file(input: &str) -> Result<ReplayFile, ReplayError> {
    Ok(ReplayFile {
        capabilities: capture_metadata::parse_capabilities(input).map_err(|error| match error {
            MetadataError::InvalidRange { line, field, value } => ReplayError::InvalidMetadata {
                line,
                name: field.to_string(),
                value,
            },
            MetadataError::MissingField { field } => ReplayError::MissingMetadataField { field },
        })?,
        frames: parse_frames(input)?,
    })
}

pub fn parse_frames(input: &str) -> Result<Vec<ReplayFrame>, ReplayError> {
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

        let mut parts = line.split_whitespace();
        let name = parts.next().expect("non-empty line has first token");

        match name {
            "SYN_REPORT" => {
                let timestamp = parse_timestamp(line_number, name, parts.next())?;
                validate_timestamp(line_number, timestamp, &mut last_timestamp)?;
                if !current.is_empty() {
                    frames.push(ReplayFrame {
                        events: std::mem::take(&mut current),
                        timestamp,
                    });
                }
            }
            "SYN_DROPPED" => {
                let timestamp = parse_timestamp(line_number, name, parts.next())?;
                validate_timestamp(line_number, timestamp, &mut last_timestamp)?;
                current.clear();
                frames.push(ReplayFrame {
                    events: vec![Event::syn_dropped()],
                    timestamp,
                });
            }
            "ABS_MT_SLOT" => current.push(Event::slot(parse_i32_value(
                line_number,
                name,
                parts.next(),
            )?)),
            "ABS_MT_TRACKING_ID" => {
                current.push(Event::tracking_id(parse_i32_value(
                    line_number,
                    name,
                    parts.next(),
                )?));
            }
            "ABS_MT_POSITION_X" => {
                current.push(Event::x(parse_i32_value(line_number, name, parts.next())?))
            }
            "ABS_MT_TOOL_TYPE" => current.push(Event::tool_type(parse_i32_value(
                line_number,
                name,
                parts.next(),
            )?)),
            "ABS_MT_POSITION_Y" => {
                current.push(Event::y(parse_i32_value(line_number, name, parts.next())?))
            }
            _ => {
                return Err(ReplayError::UnknownEvent {
                    line: line_number,
                    name: name.to_string(),
                });
            }
        }
    }

    if !current.is_empty() {
        return Err(ReplayError::UnterminatedFrame {
            line: input.lines().count().max(1),
        });
    }

    Ok(frames)
}

pub fn run_frames(
    engine: &mut Engine,
    frames: &[ReplayFrame],
) -> Result<Vec<FrameOutput>, SlotError> {
    let mut outputs = frames
        .iter()
        .map(|frame| engine.process_frame_at(&frame.events, frame.timestamp))
        .collect::<Result<Vec<_>, _>>()?;
    // A recording can end before a deferred single tap expires. Mirror the
    // live loop's idle timer so its last tap is resolved without another frame.
    if let Some(deadline) = engine.next_deadline() {
        let deadline_output = engine.advance_time(deadline);
        if !deadline_output.gestures.is_empty() {
            outputs.push(deadline_output);
        }
    }
    Ok(outputs)
}

fn parse_timestamp(line: usize, name: &str, value: Option<&str>) -> Result<Duration, ReplayError> {
    let value = value.ok_or_else(|| ReplayError::MissingValue {
        line,
        name: format!("{name} timestamp_us"),
    })?;
    let micros = value
        .parse::<u64>()
        .map_err(|_| ReplayError::InvalidValue {
            line,
            name: format!("{name} timestamp_us"),
            value: value.to_string(),
        })?;

    Ok(Duration::from_micros(micros))
}

fn validate_timestamp(
    line: usize,
    timestamp: Duration,
    last_timestamp: &mut Option<Duration>,
) -> Result<(), ReplayError> {
    if let Some(previous) = *last_timestamp {
        if timestamp < previous {
            return Err(ReplayError::NonMonotonicTimestamp {
                line,
                previous_us: previous.as_micros() as u64,
                current_us: timestamp.as_micros() as u64,
            });
        }
    }
    *last_timestamp = Some(timestamp);
    Ok(())
}

fn parse_i32_value(line: usize, name: &str, value: Option<&str>) -> Result<i32, ReplayError> {
    let value = value.ok_or_else(|| ReplayError::MissingValue {
        line,
        name: name.to_string(),
    })?;

    value.parse::<i32>().map_err(|_| ReplayError::InvalidValue {
        line,
        name: name.to_string(),
        value: value.to_string(),
    })
}

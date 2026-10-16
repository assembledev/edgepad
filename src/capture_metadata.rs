//! Capability headers shared by parsed and raw event captures.

use crate::core::{AxisRange, Capabilities};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MetadataError {
    InvalidRange {
        line: usize,
        field: &'static str,
        value: String,
    },
    MissingField {
        field: &'static str,
    },
}

#[derive(Default)]
struct CapabilityMetadata {
    slots: Option<AxisRange>,
    x: Option<AxisRange>,
    y: Option<AxisRange>,
    saw_any: bool,
}

pub(crate) fn parse_capabilities(input: &str) -> Result<Option<Capabilities>, MetadataError> {
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
                metadata.slots = Some(parse_range(line_number, "slots", value)?);
            }
            "x" => {
                metadata.saw_any = true;
                metadata.x = Some(parse_range(line_number, "x", value)?);
            }
            "y" => {
                metadata.saw_any = true;
                metadata.y = Some(parse_range(line_number, "y", value)?);
            }
            _ => {}
        }
    }

    if !metadata.saw_any {
        return Ok(None);
    }

    let slots = metadata
        .slots
        .ok_or(MetadataError::MissingField { field: "slots" })?;
    let x = metadata
        .x
        .ok_or(MetadataError::MissingField { field: "x" })?;
    let y = metadata
        .y
        .ok_or(MetadataError::MissingField { field: "y" })?;

    Ok(Some(Capabilities {
        slot_min: slots.min,
        slot_max: slots.max,
        x,
        y,
    }))
}

fn parse_range(line: usize, field: &'static str, value: &str) -> Result<AxisRange, MetadataError> {
    let invalid = || MetadataError::InvalidRange {
        line,
        field,
        value: value.to_string(),
    };
    let (min, max) = value.split_once("..=").ok_or_else(invalid)?;
    let min = min.trim().parse::<i32>().map_err(|_| invalid())?;
    let max = max.trim().parse::<i32>().map_err(|_| invalid())?;

    if min > max {
        return Err(invalid());
    }

    Ok(AxisRange { min, max })
}

use std::io::{self, Write};
use std::path::Path;

use crate::core::{AxisRange, Capabilities};
use evdev::raw_stream::RawDevice;
use evdev::{AbsoluteAxisCode, EventSummary, InputEvent, KeyCode, MiscCode, SynchronizationCode};

pub fn write_capture_header(
    mut writer: impl Write,
    device_path: &Path,
    capabilities: Option<Capabilities>,
) -> io::Result<()> {
    writeln!(writer, "# edgepad .ev dump")?;
    writeln!(writer, "# device: {}", device_path.display())?;
    if let Some(capabilities) = capabilities {
        writeln!(
            writer,
            "# slots: {}..={}",
            capabilities.slot_min, capabilities.slot_max
        )?;
        writeln!(
            writer,
            "# x: {}..={}",
            capabilities.x.min, capabilities.x.max
        )?;
        writeln!(
            writer,
            "# y: {}..={}",
            capabilities.y.min, capabilities.y.max
        )?;
    } else {
        writeln!(writer, "# capabilities: unavailable")?;
    }
    writeln!(writer)?;
    Ok(())
}

pub fn capabilities_from_raw_device(device: &RawDevice) -> Option<Capabilities> {
    let slot = axis_info(device, AbsoluteAxisCode::ABS_MT_SLOT)?;
    let x = axis_info(device, AbsoluteAxisCode::ABS_MT_POSITION_X)?;
    let y = axis_info(device, AbsoluteAxisCode::ABS_MT_POSITION_Y)?;

    Some(Capabilities {
        slot_min: slot.min,
        slot_max: slot.max,
        x,
        y,
    })
}

fn axis_info(device: &RawDevice, wanted: AbsoluteAxisCode) -> Option<AxisRange> {
    device.get_absinfo().ok()?.find_map(|(axis, info)| {
        (axis == wanted).then_some(AxisRange {
            min: info.minimum(),
            max: info.maximum(),
        })
    })
}

pub fn fixture_line_for_event(event: InputEvent) -> Option<String> {
    match event.destructure() {
        EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_SLOT, value) => {
            Some(format!("ABS_MT_SLOT {value}"))
        }
        EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_TRACKING_ID, value) => {
            Some(format!("ABS_MT_TRACKING_ID {value}"))
        }
        EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_POSITION_X, value) => {
            Some(format!("ABS_MT_POSITION_X {value}"))
        }
        EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_POSITION_Y, value) => {
            Some(format!("ABS_MT_POSITION_Y {value}"))
        }
        EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
            Some("SYN_REPORT".to_string())
        }
        EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
            Some("SYN_DROPPED".to_string())
        }
        _ => None,
    }
}

pub fn raw_line_for_event(event: InputEvent) -> String {
    match event.destructure() {
        EventSummary::Synchronization(_, code, value) => {
            format!("EV_SYN {} {value}", synchronization_code_name(code))
        }
        EventSummary::Key(_, code, value) => format!("EV_KEY {} {value}", key_code_name(code)),
        EventSummary::AbsoluteAxis(_, code, value) => {
            format!("EV_ABS {} {value}", absolute_axis_code_name(code))
        }
        EventSummary::Misc(_, code, value) => format!("EV_MSC {} {value}", misc_code_name(code)),
        _ => format!(
            "EV_{} {} {}",
            event.event_type().0,
            event.code(),
            event.value()
        ),
    }
}

fn synchronization_code_name(code: SynchronizationCode) -> String {
    if code == SynchronizationCode::SYN_REPORT {
        "SYN_REPORT".to_string()
    } else if code == SynchronizationCode::SYN_DROPPED {
        "SYN_DROPPED".to_string()
    } else {
        code.0.to_string()
    }
}

fn misc_code_name(code: MiscCode) -> String {
    if code == MiscCode::MSC_TIMESTAMP {
        "MSC_TIMESTAMP".to_string()
    } else {
        code.0.to_string()
    }
}

fn absolute_axis_code_name(code: AbsoluteAxisCode) -> String {
    let name = if code == AbsoluteAxisCode::ABS_X {
        Some("ABS_X")
    } else if code == AbsoluteAxisCode::ABS_Y {
        Some("ABS_Y")
    } else if code == AbsoluteAxisCode::ABS_MT_SLOT {
        Some("ABS_MT_SLOT")
    } else if code == AbsoluteAxisCode::ABS_MT_TOUCH_MAJOR {
        Some("ABS_MT_TOUCH_MAJOR")
    } else if code == AbsoluteAxisCode::ABS_MT_TOUCH_MINOR {
        Some("ABS_MT_TOUCH_MINOR")
    } else if code == AbsoluteAxisCode::ABS_MT_WIDTH_MAJOR {
        Some("ABS_MT_WIDTH_MAJOR")
    } else if code == AbsoluteAxisCode::ABS_MT_WIDTH_MINOR {
        Some("ABS_MT_WIDTH_MINOR")
    } else if code == AbsoluteAxisCode::ABS_MT_ORIENTATION {
        Some("ABS_MT_ORIENTATION")
    } else if code == AbsoluteAxisCode::ABS_MT_POSITION_X {
        Some("ABS_MT_POSITION_X")
    } else if code == AbsoluteAxisCode::ABS_MT_POSITION_Y {
        Some("ABS_MT_POSITION_Y")
    } else if code == AbsoluteAxisCode::ABS_MT_TOOL_TYPE {
        Some("ABS_MT_TOOL_TYPE")
    } else if code == AbsoluteAxisCode::ABS_MT_BLOB_ID {
        Some("ABS_MT_BLOB_ID")
    } else if code == AbsoluteAxisCode::ABS_MT_TRACKING_ID {
        Some("ABS_MT_TRACKING_ID")
    } else if code == AbsoluteAxisCode::ABS_MT_PRESSURE {
        Some("ABS_MT_PRESSURE")
    } else if code == AbsoluteAxisCode::ABS_MT_DISTANCE {
        Some("ABS_MT_DISTANCE")
    } else if code == AbsoluteAxisCode::ABS_MT_TOOL_X {
        Some("ABS_MT_TOOL_X")
    } else if code == AbsoluteAxisCode::ABS_MT_TOOL_Y {
        Some("ABS_MT_TOOL_Y")
    } else {
        None
    };

    name.map_or_else(|| code.0.to_string(), str::to_string)
}

fn key_code_name(code: KeyCode) -> String {
    let name = if code == KeyCode::BTN_LEFT {
        Some("BTN_LEFT")
    } else if code == KeyCode::BTN_RIGHT {
        Some("BTN_RIGHT")
    } else if code == KeyCode::BTN_MIDDLE {
        Some("BTN_MIDDLE")
    } else if code == KeyCode::BTN_SIDE {
        Some("BTN_SIDE")
    } else if code == KeyCode::BTN_EXTRA {
        Some("BTN_EXTRA")
    } else if code == KeyCode::BTN_TOUCH {
        Some("BTN_TOUCH")
    } else if code == KeyCode::BTN_TOOL_FINGER {
        Some("BTN_TOOL_FINGER")
    } else if code == KeyCode::BTN_TOOL_DOUBLETAP {
        Some("BTN_TOOL_DOUBLETAP")
    } else if code == KeyCode::BTN_TOOL_TRIPLETAP {
        Some("BTN_TOOL_TRIPLETAP")
    } else if code == KeyCode::BTN_TOOL_QUADTAP {
        Some("BTN_TOOL_QUADTAP")
    } else if code == KeyCode::BTN_TOOL_QUINTTAP {
        Some("BTN_TOOL_QUINTTAP")
    } else {
        None
    };

    name.map_or_else(|| code.0.to_string(), str::to_string)
}

pub fn write_fixture_event(mut writer: impl Write, event: InputEvent) -> io::Result<bool> {
    let Some(line) = fixture_line_for_event(event) else {
        return Ok(false);
    };

    write_event_line(&mut writer, &line)?;
    Ok(true)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteEventsResult {
    pub reached_limit: bool,
    pub events_written: usize,
    pub frame_boundaries_written: usize,
}

impl WriteEventsResult {
    pub fn add(&mut self, other: Self) {
        self.reached_limit |= other.reached_limit;
        self.events_written += other.events_written;
        self.frame_boundaries_written += other.frame_boundaries_written;
    }
}

pub type WriteFixtureEventsResult = WriteEventsResult;

pub fn write_fixture_events_with_limit(
    mut writer: impl Write,
    events: impl IntoIterator<Item = InputEvent>,
    remaining_frames: &mut Option<usize>,
) -> io::Result<WriteEventsResult> {
    let mut result = WriteEventsResult::default();

    if matches!(remaining_frames, Some(0)) {
        result.reached_limit = true;
        return Ok(result);
    }

    for event in events {
        let Some(line) = fixture_line_for_event(event) else {
            continue;
        };
        let is_sync_boundary = is_sync_boundary(&line);
        write_event_line(&mut writer, &line)?;
        result.events_written += 1;

        if is_sync_boundary {
            result.frame_boundaries_written += 1;
            if let Some(remaining) = remaining_frames.as_mut() {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    result.reached_limit = true;
                    return Ok(result);
                }
            }
        }
    }

    Ok(result)
}

pub fn write_raw_events_with_limit(
    mut writer: impl Write,
    events: impl IntoIterator<Item = InputEvent>,
    remaining_frames: &mut Option<usize>,
) -> io::Result<WriteEventsResult> {
    let mut result = WriteEventsResult::default();

    if matches!(remaining_frames, Some(0)) {
        result.reached_limit = true;
        return Ok(result);
    }

    for event in events {
        let line = raw_line_for_event(event);
        let is_sync_boundary = is_sync_boundary(&line);
        write_event_line(&mut writer, &line)?;
        result.events_written += 1;

        if is_sync_boundary {
            result.frame_boundaries_written += 1;
            if let Some(remaining) = remaining_frames.as_mut() {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    result.reached_limit = true;
                    return Ok(result);
                }
            }
        }
    }

    Ok(result)
}

fn write_event_line(mut writer: impl Write, line: &str) -> io::Result<()> {
    writeln!(writer, "{line}")?;
    if is_sync_boundary(line) {
        writeln!(writer)?;
        writer.flush()?;
    }
    Ok(())
}

fn is_sync_boundary(line: &str) -> bool {
    matches!(
        line,
        "SYN_REPORT" | "SYN_DROPPED" | "EV_SYN SYN_REPORT 0" | "EV_SYN SYN_DROPPED 0"
    )
}

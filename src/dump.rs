use std::io::{self, Write};
use std::path::Path;

use crate::core::{AxisRange, Capabilities};
use evdev::raw_stream::RawDevice;
use evdev::{AbsoluteAxisCode, EventSummary, InputEvent, SynchronizationCode};

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

pub fn write_fixture_event(mut writer: impl Write, event: InputEvent) -> io::Result<bool> {
    let Some(line) = fixture_line_for_event(event) else {
        return Ok(false);
    };

    write_fixture_line(&mut writer, &line)?;
    Ok(true)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteFixtureEventsResult {
    pub reached_limit: bool,
    pub events_written: usize,
    pub frame_boundaries_written: usize,
}

impl WriteFixtureEventsResult {
    pub fn add(&mut self, other: Self) {
        self.reached_limit |= other.reached_limit;
        self.events_written += other.events_written;
        self.frame_boundaries_written += other.frame_boundaries_written;
    }
}

pub fn write_fixture_events_with_limit(
    mut writer: impl Write,
    events: impl IntoIterator<Item = InputEvent>,
    remaining_frames: &mut Option<usize>,
) -> io::Result<WriteFixtureEventsResult> {
    let mut result = WriteFixtureEventsResult::default();

    if matches!(remaining_frames, Some(0)) {
        result.reached_limit = true;
        return Ok(result);
    }

    for event in events {
        let Some(line) = fixture_line_for_event(event) else {
            continue;
        };
        let is_sync_boundary = is_sync_boundary(&line);
        write_fixture_line(&mut writer, &line)?;
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

fn write_fixture_line(mut writer: impl Write, line: &str) -> io::Result<()> {
    writeln!(writer, "{line}")?;
    if is_sync_boundary(line) {
        writeln!(writer)?;
        writer.flush()?;
    }
    Ok(())
}

fn is_sync_boundary(line: &str) -> bool {
    line == "SYN_REPORT" || line == "SYN_DROPPED"
}

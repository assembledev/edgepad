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

    let is_sync_boundary = line == "SYN_REPORT" || line == "SYN_DROPPED";
    writeln!(writer, "{line}")?;
    if is_sync_boundary {
        writeln!(writer)?;
        writer.flush()?;
    }

    Ok(true)
}

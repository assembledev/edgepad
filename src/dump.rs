use std::io::{self, Write};

use evdev::{AbsoluteAxisCode, EventSummary, InputEvent, SynchronizationCode};

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

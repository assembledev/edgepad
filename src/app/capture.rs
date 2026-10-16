//! Live capture, including waiting for input and draining contacts after a budget.
//! The dump library owns frame accounting and encoding; this command owns device
//! access and progress messages. A budget ends capture only at an idle boundary.

use std::fs::File;
use std::io::LineWriter;
use std::path::Path;
use std::time::Duration;

use edgepad::core::Capabilities;
use edgepad::device::{physical_touch_is_down, wait_for_raw_device_events};
use edgepad::dump::{
    capabilities_from_raw_device, write_capture_header, write_fixture_events_with_budget,
    write_raw_events_with_budget, DumpCaptureDecision, DumpFrameBudget, WriteEventsResult,
};
use evdev::raw_stream::RawDevice;

const DUMP_FIRST_EVENT_WARNING_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DumpFormat {
    Replay,
    Raw,
}

impl DumpFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Replay => "replay",
            Self::Raw => "raw",
        }
    }
}

pub(super) fn dump(
    device_path: &Path,
    out_path: &Path,
    requested_frames: Option<usize>,
    raw: bool,
) -> Result<(), String> {
    let mut frame_budget = DumpFrameBudget::new(requested_frames);
    let format = if raw {
        DumpFormat::Raw
    } else {
        DumpFormat::Replay
    };
    let mut device = RawDevice::open(device_path)
        .map_err(|err| format!("failed to open device {}: {err}", device_path.display()))?;
    let file = File::create(out_path)
        .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
    let mut writer = LineWriter::new(file);
    let capabilities = capabilities_from_raw_device(&device);
    write_capture_header(&mut writer, device_path, capabilities)
        .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
    let mut total = WriteEventsResult::default();

    eprintln!("edgepad dump: device={}", device_path.display());
    eprintln!("edgepad dump: waiting for input events");
    let mut no_events_warning_emitted =
        !wait_for_raw_device_events(&device, DUMP_FIRST_EVENT_WARNING_TIMEOUT).map_err(|err| {
            format!(
                "failed to wait for events from {}: {err}",
                device_path.display()
            )
        })?;
    if no_events_warning_emitted {
        eprintln!("{}", dump_no_events_warning(device_path));
    }

    loop {
        let mut events = device
            .fetch_events()
            .map_err(|err| {
                format!(
                    "failed to read events from {}: {err}",
                    device_path.display()
                )
            })?
            .peekable();
        if no_events_warning_emitted && events.peek().is_some() {
            eprintln!("edgepad dump: input events received; capture started");
            no_events_warning_emitted = false;
        }
        let result = if raw {
            write_raw_events_with_budget(&mut writer, events, &mut frame_budget)
        } else {
            write_fixture_events_with_budget(&mut writer, events, &mut frame_budget)
        }
        .map_err(|err| format!("failed to write {}: {err}", out_path.display()))?;
        let can_finish_after_batch =
            result.frame_boundaries_written > 0 && result.ends_at_frame_boundary;
        total.add(result);
        if frame_budget.is_reached() && can_finish_after_batch {
            let touch_down = physical_touch_is_down(&device, capabilities).map_err(|err| {
                format!(
                    "failed to read current touch state from {}: {err}",
                    device_path.display()
                )
            })?;
            match frame_budget.decide_after_batch(touch_down) {
                DumpCaptureDecision::Continue => {}
                DumpCaptureDecision::DrainStarted => {
                    eprintln!("edgepad dump: frame budget reached; release all contacts to finish");
                }
                DumpCaptureDecision::Finish => {
                    print_dump_summary(
                        device_path,
                        out_path,
                        capabilities,
                        requested_frames,
                        total,
                        format,
                    );
                    return Ok(());
                }
            }
        }
    }
}

fn dump_no_events_warning(device_path: &Path) -> String {
    format!(
        "edgepad dump: no events received from {} after {}s; device may be grabbed; continuing to wait (stop edgepad.service before capturing)",
        device_path.display(),
        DUMP_FIRST_EVENT_WARNING_TIMEOUT.as_secs()
    )
}

fn print_dump_summary(
    device_path: &Path,
    out_path: &Path,
    capabilities: Option<Capabilities>,
    requested_frames: Option<usize>,
    stats: WriteEventsResult,
    format: DumpFormat,
) {
    println!("wrote: {}", out_path.display());
    println!("device: {}", device_path.display());
    println!("format: {}", format.label());
    if let Some(capabilities) = capabilities {
        println!(
            "capabilities: slots={}..={} x={}..={} y={}..={}",
            capabilities.slot_min,
            capabilities.slot_max,
            capabilities.x.min,
            capabilities.x.max,
            capabilities.y.min,
            capabilities.y.max
        );
    } else {
        println!("capabilities: unavailable");
    }
    if let Some(requested_frames) = requested_frames {
        println!("requested_frame_boundaries: {requested_frames}");
        println!(
            "additional_frame_boundaries_after_budget: {}",
            stats
                .frame_boundaries_written
                .saturating_sub(requested_frames)
        );
    }
    println!(
        "written_frame_boundaries: {}",
        stats.frame_boundaries_written
    );
    println!("written_events: {}", stats.events_written);
    println!("next: {}", dump_next_command(format, out_path));
}

fn dump_next_command(format: DumpFormat, out_path: &Path) -> String {
    match format {
        DumpFormat::Replay => format!("edgepad replay {}", out_path.display()),
        DumpFormat::Raw => format!("edgepad replay-raw {}", out_path.display()),
    }
}

#[cfg(test)]
mod tests;

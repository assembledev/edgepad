use std::fs;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use evdev::raw_stream::RawDevice;
use evdev::{AbsoluteAxisCode, Device, PropType};

use crate::uinput::DEFAULT_VIRTUAL_TOUCHPAD_NAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisInfo {
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Touchpad,
    Touchscreen,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventDeviceCapabilities {
    pub has_mt_slot: bool,
    pub has_mt_tracking_id: bool,
    pub has_mt_position_x: bool,
    pub has_mt_position_y: bool,
    pub has_abs_x: bool,
    pub has_abs_y: bool,
    pub is_pointer: bool,
    pub is_direct: bool,
    pub is_buttonpad: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSummary {
    pub path: PathBuf,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub kind: DeviceKind,
    pub slot_range: Option<AxisInfo>,
    pub x_range: Option<AxisInfo>,
    pub y_range: Option<AxisInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub summaries: Vec<DeviceSummary>,
    pub event_node_count: usize,
    pub unreadable_count: usize,
}

pub fn classify_device(caps: &EventDeviceCapabilities) -> DeviceKind {
    let has_type_b_multitouch = caps.has_mt_slot
        && caps.has_mt_tracking_id
        && caps.has_mt_position_x
        && caps.has_mt_position_y;

    if has_type_b_multitouch && caps.is_direct {
        DeviceKind::Touchscreen
    } else if has_type_b_multitouch && caps.is_pointer {
        DeviceKind::Touchpad
    } else {
        DeviceKind::Other
    }
}

pub fn event_device_paths(input_root: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(input_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };

    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            event_number(&path).map(|number| (number, path))
        })
        .collect::<Vec<_>>();

    paths.sort_by_key(|(number, _)| *number);

    Ok(paths.into_iter().map(|(_, path)| path).collect())
}

pub fn discover_devices(input_root: &Path) -> io::Result<Vec<DeviceSummary>> {
    Ok(discover_device_report(input_root)?.summaries)
}

pub fn discover_device_report(input_root: &Path) -> io::Result<DiscoveryReport> {
    let paths = event_device_paths(input_root)?;
    let event_node_count = paths.len();
    let mut summaries = Vec::new();
    let mut unreadable_count = 0;

    for path in paths {
        match Device::open(&path) {
            Ok(device) => summaries.push(summary_from_device(path, &device)),
            Err(_) => unreadable_count += 1,
        }
    }

    Ok(DiscoveryReport {
        summaries,
        event_node_count,
        unreadable_count,
    })
}

pub fn wait_for_raw_device_events(device: &RawDevice, timeout: Duration) -> io::Result<bool> {
    wait_for_fd_readable(device.as_raw_fd(), timeout)
}

fn wait_for_fd_readable(fd: RawFd, timeout: Duration) -> io::Result<bool> {
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout_ms = poll_timeout_ms(timeout);

    loop {
        let result = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
        if result >= 0 {
            return Ok(result > 0);
        }

        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

fn poll_timeout_ms(timeout: Duration) -> i32 {
    let millis = timeout.as_millis();
    if millis == 0 {
        0
    } else {
        millis.min(i32::MAX as u128) as i32
    }
}

pub fn touchpad_candidates(summaries: &[DeviceSummary]) -> Vec<&DeviceSummary> {
    summaries
        .iter()
        .filter(|summary| {
            summary.kind == DeviceKind::Touchpad && !is_edgepad_virtual_touchpad(summary)
        })
        .collect()
}

fn is_edgepad_virtual_touchpad(summary: &DeviceSummary) -> bool {
    summary.name == DEFAULT_VIRTUAL_TOUCHPAD_NAME
}

pub fn format_device_line(summary: &DeviceSummary) -> String {
    format!(
        "{} kind={} name={:?} id={:04x}:{:04x} slots={} x={} y={}",
        summary.path.display(),
        kind_name(summary.kind),
        summary.name,
        summary.vendor,
        summary.product,
        format_axis(summary.slot_range),
        format_axis(summary.x_range),
        format_axis(summary.y_range)
    )
}

fn summary_from_device(path: PathBuf, device: &Device) -> DeviceSummary {
    let caps = capabilities_from_device(device);
    let input_id = device.input_id();

    DeviceSummary {
        path,
        name: device.name().unwrap_or("unknown").to_string(),
        vendor: input_id.vendor(),
        product: input_id.product(),
        kind: classify_device(&caps),
        slot_range: axis_info(device, AbsoluteAxisCode::ABS_MT_SLOT),
        x_range: axis_info(device, AbsoluteAxisCode::ABS_MT_POSITION_X),
        y_range: axis_info(device, AbsoluteAxisCode::ABS_MT_POSITION_Y),
    }
}

fn capabilities_from_device(device: &Device) -> EventDeviceCapabilities {
    let axes = device.supported_absolute_axes();
    let props = device.properties();

    EventDeviceCapabilities {
        has_mt_slot: axes.is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_SLOT)),
        has_mt_tracking_id: axes
            .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_TRACKING_ID)),
        has_mt_position_x: axes
            .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_POSITION_X)),
        has_mt_position_y: axes
            .is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_POSITION_Y)),
        has_abs_x: axes.is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_X)),
        has_abs_y: axes.is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_Y)),
        is_pointer: props.contains(PropType::POINTER),
        is_direct: props.contains(PropType::DIRECT),
        is_buttonpad: props.contains(PropType::BUTTONPAD),
    }
}

fn axis_info(device: &Device, wanted: AbsoluteAxisCode) -> Option<AxisInfo> {
    device.get_absinfo().ok()?.find_map(|(axis, info)| {
        (axis == wanted).then_some(AxisInfo {
            min: info.minimum(),
            max: info.maximum(),
        })
    })
}

fn event_number(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let number = name.strip_prefix("event")?;
    if number.is_empty() {
        return None;
    }
    number.parse::<u32>().ok()
}

fn kind_name(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Touchpad => "touchpad",
        DeviceKind::Touchscreen => "touchscreen",
        DeviceKind::Other => "other",
    }
}

fn format_axis(axis: Option<AxisInfo>) -> String {
    match axis {
        Some(axis) => format!("{}..={}", axis.min, axis.max),
        None => "n/a".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn wait_for_fd_readable_reports_timeout_and_ready_data() {
        let (reader, mut writer) = UnixStream::pair().expect("socket pair should be created");

        assert!(!wait_for_fd_readable(reader.as_raw_fd(), Duration::ZERO)
            .expect("empty socket should be polled"));

        writer
            .write_all(b"x")
            .expect("socket should accept test data");

        assert!(
            wait_for_fd_readable(reader.as_raw_fd(), Duration::from_millis(100))
                .expect("readable socket should be polled")
        );
    }
}

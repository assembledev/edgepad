use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use evdev::{AbsoluteAxisCode, Device, PropType};

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
    let mut summaries = Vec::new();

    for path in event_device_paths(input_root)? {
        let Ok(device) = Device::open(&path) else {
            continue;
        };
        summaries.push(summary_from_device(path, &device));
    }

    Ok(summaries)
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

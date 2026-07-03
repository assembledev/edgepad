use crate::core::Capabilities;
use crate::raw::{
    RawEvent, RawOutputSink, ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_SLOT, ABS_MT_TRACKING_ID,
    ABS_X, ABS_Y, BTN_TOOL_DOUBLETAP, BTN_TOOL_FINGER, BTN_TOOL_QUADTAP, BTN_TOOL_QUINTTAP,
    BTN_TOOL_TRIPLETAP, BTN_TOUCH,
};
use evdev::{
    raw_stream::RawDevice, uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AttributeSet,
    InputEvent, KeyCode, PropType, UinputAbsSetup,
};
use std::io;

const DEFAULT_VIRTUAL_TOUCHPAD_NAME: &str = "edgepad virtual touchpad";
const TRACKING_ID_MAX: i32 = 65_535;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualAbsAxis {
    pub code: u16,
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

impl VirtualAbsAxis {
    pub const fn new(code: u16, min: i32, max: i32) -> Self {
        Self {
            code,
            value: min,
            min,
            max,
            fuzz: 0,
            flat: 0,
            resolution: 0,
        }
    }

    fn to_uinput_abs_setup(&self) -> UinputAbsSetup {
        UinputAbsSetup::new(
            AbsoluteAxisCode(self.code),
            AbsInfo::new(
                self.value,
                self.min,
                self.max,
                self.fuzz,
                self.flat,
                self.resolution,
            ),
        )
    }

    fn with_code(&self, code: u16) -> Self {
        Self { code, ..*self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalTouchpadAbsInfo {
    pub abs_x: Option<VirtualAbsAxis>,
    pub abs_y: Option<VirtualAbsAxis>,
    pub mt_slot: VirtualAbsAxis,
    pub mt_tracking_id: Option<VirtualAbsAxis>,
    pub mt_position_x: VirtualAbsAxis,
    pub mt_position_y: VirtualAbsAxis,
}

impl PhysicalTouchpadAbsInfo {
    pub fn from_capabilities(capabilities: Capabilities) -> Self {
        Self {
            abs_x: Some(VirtualAbsAxis::new(
                ABS_X,
                capabilities.x.min,
                capabilities.x.max,
            )),
            abs_y: Some(VirtualAbsAxis::new(
                ABS_Y,
                capabilities.y.min,
                capabilities.y.max,
            )),
            mt_slot: VirtualAbsAxis::new(ABS_MT_SLOT, capabilities.slot_min, capabilities.slot_max),
            mt_tracking_id: Some(VirtualAbsAxis::new(ABS_MT_TRACKING_ID, 0, TRACKING_ID_MAX)),
            mt_position_x: VirtualAbsAxis::new(
                ABS_MT_POSITION_X,
                capabilities.x.min,
                capabilities.x.max,
            ),
            mt_position_y: VirtualAbsAxis::new(
                ABS_MT_POSITION_Y,
                capabilities.y.min,
                capabilities.y.max,
            ),
        }
    }

    pub fn from_raw_device(device: &RawDevice, capabilities: Capabilities) -> Self {
        let fallback = Self::from_capabilities(capabilities);
        let axes = device
            .get_absinfo()
            .map(|infos| infos.collect::<Vec<_>>())
            .unwrap_or_default();

        Self {
            abs_x: axis_from_abs_infos(&axes, ABS_X).or(fallback.abs_x),
            abs_y: axis_from_abs_infos(&axes, ABS_Y).or(fallback.abs_y),
            mt_slot: axis_from_abs_infos(&axes, ABS_MT_SLOT).unwrap_or(fallback.mt_slot),
            mt_tracking_id: axis_from_abs_infos(&axes, ABS_MT_TRACKING_ID)
                .or(fallback.mt_tracking_id),
            mt_position_x: axis_from_abs_infos(&axes, ABS_MT_POSITION_X)
                .unwrap_or(fallback.mt_position_x),
            mt_position_y: axis_from_abs_infos(&axes, ABS_MT_POSITION_Y)
                .unwrap_or(fallback.mt_position_y),
        }
    }
}

fn axis_from_abs_infos(
    axes: &[(AbsoluteAxisCode, AbsInfo)],
    wanted_code: u16,
) -> Option<VirtualAbsAxis> {
    axes.iter()
        .find(|(axis, _)| axis.0 == wanted_code)
        .map(|(_, info)| VirtualAbsAxis {
            code: wanted_code,
            value: info.value(),
            min: info.minimum(),
            max: info.maximum(),
            fuzz: info.fuzz(),
            flat: info.flat(),
            resolution: info.resolution(),
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualTouchpadSpec {
    pub name: String,
    pub properties: Vec<u16>,
    pub keys: Vec<u16>,
    pub absolute_axes: Vec<VirtualAbsAxis>,
    pub misc: Vec<u16>,
}

impl VirtualTouchpadSpec {
    pub fn from_capabilities(capabilities: Capabilities) -> Self {
        Self::named(capabilities, DEFAULT_VIRTUAL_TOUCHPAD_NAME)
    }

    pub fn from_raw_device(device: &RawDevice, capabilities: Capabilities) -> Self {
        Self::from_physical_abs_info(
            PhysicalTouchpadAbsInfo::from_raw_device(device, capabilities),
            DEFAULT_VIRTUAL_TOUCHPAD_NAME,
        )
    }

    pub fn named(capabilities: Capabilities, name: impl Into<String>) -> Self {
        Self::from_physical_abs_info(
            PhysicalTouchpadAbsInfo::from_capabilities(capabilities),
            name,
        )
    }

    pub fn from_physical_abs_info(
        abs_info: PhysicalTouchpadAbsInfo,
        name: impl Into<String>,
    ) -> Self {
        let abs_x = abs_info
            .abs_x
            .unwrap_or_else(|| abs_info.mt_position_x.with_code(ABS_X));
        let abs_y = abs_info
            .abs_y
            .unwrap_or_else(|| abs_info.mt_position_y.with_code(ABS_Y));
        Self {
            name: name.into(),
            properties: vec![PropType::POINTER.0],
            keys: vec![
                BTN_TOUCH,
                BTN_TOOL_FINGER,
                BTN_TOOL_DOUBLETAP,
                BTN_TOOL_TRIPLETAP,
                BTN_TOOL_QUADTAP,
                BTN_TOOL_QUINTTAP,
            ],
            absolute_axes: vec![
                abs_x,
                abs_y,
                abs_info.mt_slot,
                abs_info
                    .mt_tracking_id
                    .unwrap_or_else(|| VirtualAbsAxis::new(ABS_MT_TRACKING_ID, 0, TRACKING_ID_MAX)),
                abs_info.mt_position_x,
                abs_info.mt_position_y,
            ],
            misc: Vec::new(),
        }
    }
}

pub fn build_virtual_touchpad(spec: &VirtualTouchpadSpec) -> io::Result<VirtualDevice> {
    let mut properties = AttributeSet::<PropType>::new();
    for property in &spec.properties {
        properties.insert(PropType(*property));
    }

    let mut keys = AttributeSet::<KeyCode>::new();
    for key in &spec.keys {
        keys.insert(KeyCode(*key));
    }

    let mut builder = VirtualDevice::builder()?
        .name(&spec.name)
        .with_properties(&properties)?
        .with_keys(&keys)?;

    for axis in &spec.absolute_axes {
        builder = builder.with_absolute_axis(&axis.to_uinput_abs_setup())?;
    }

    builder.build()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UinputRawSinkError<E> {
    Emit(E),
}

pub trait UinputEventWriter {
    type Error;

    fn emit_events(&mut self, events: &[InputEvent]) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone)]
pub struct UinputRawOutputSink<W> {
    writer: W,
    current: Vec<InputEvent>,
}

impl<W> UinputRawOutputSink<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            current: Vec::new(),
        }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W> RawOutputSink for UinputRawOutputSink<W>
where
    W: UinputEventWriter,
{
    type Error = UinputRawSinkError<W::Error>;

    fn emit(&mut self, event: RawEvent) -> Result<(), Self::Error> {
        self.current
            .push(InputEvent::new(event.kind, event.code, event.value));
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Self::Error> {
        if self.current.is_empty() {
            return Ok(());
        }

        self.writer
            .emit_events(&self.current)
            .map_err(UinputRawSinkError::Emit)?;
        self.current.clear();
        Ok(())
    }
}

impl UinputEventWriter for evdev::uinput::VirtualDevice {
    type Error = std::io::Error;

    fn emit_events(&mut self, events: &[InputEvent]) -> Result<(), Self::Error> {
        self.emit(events)
    }
}

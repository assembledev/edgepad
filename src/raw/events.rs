use std::time::Duration;

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const EV_MSC: u16 = 0x04;

pub const SYN_REPORT: u16 = 0x00;
pub const SYN_DROPPED: u16 = 0x03;
pub const MSC_TIMESTAMP: u16 = 0x05;

pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;
pub const BTN_FORWARD: u16 = 0x115;
pub const BTN_BACK: u16 = 0x116;
pub const BTN_TASK: u16 = 0x117;
pub const BTN_TOOL_FINGER: u16 = 0x145;
pub const BTN_TOOL_QUINTTAP: u16 = 0x148;
pub const BTN_TOUCH: u16 = 0x14a;
pub const BTN_TOOL_DOUBLETAP: u16 = 0x14d;
pub const BTN_TOOL_TRIPLETAP: u16 = 0x14e;
pub const BTN_TOOL_QUADTAP: u16 = 0x14f;

pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_MT_SLOT: u16 = 0x2f;
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
pub const ABS_MT_TOUCH_MINOR: u16 = 0x31;
pub const ABS_MT_WIDTH_MAJOR: u16 = 0x32;
pub const ABS_MT_WIDTH_MINOR: u16 = 0x33;
pub const ABS_MT_ORIENTATION: u16 = 0x34;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TOOL_TYPE: u16 = 0x37;
pub const ABS_MT_BLOB_ID: u16 = 0x38;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
pub const ABS_MT_PRESSURE: u16 = 0x3a;
pub const ABS_MT_DISTANCE: u16 = 0x3b;
pub const ABS_MT_TOOL_X: u16 = 0x3c;
pub const ABS_MT_TOOL_Y: u16 = 0x3d;

pub const fn is_pointer_button_code(code: u16) -> bool {
    matches!(code, BTN_LEFT..=BTN_TASK)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl RawEvent {
    pub const fn new(kind: u16, code: u16, value: i32) -> Self {
        Self { kind, code, value }
    }

    pub const fn syn_report() -> Self {
        Self::new(EV_SYN, SYN_REPORT, 0)
    }

    pub const fn syn_dropped() -> Self {
        Self::new(EV_SYN, SYN_DROPPED, 0)
    }

    pub const fn btn_touch(pressed: bool) -> Self {
        Self::new(EV_KEY, BTN_TOUCH, pressed as i32)
    }

    pub const fn btn_tool_finger(pressed: bool) -> Self {
        Self::new(EV_KEY, BTN_TOOL_FINGER, pressed as i32)
    }

    pub const fn btn_tool_doubletap(pressed: bool) -> Self {
        Self::new(EV_KEY, BTN_TOOL_DOUBLETAP, pressed as i32)
    }

    pub const fn btn_tool_tripletap(pressed: bool) -> Self {
        Self::new(EV_KEY, BTN_TOOL_TRIPLETAP, pressed as i32)
    }

    pub const fn btn_tool_quadtap(pressed: bool) -> Self {
        Self::new(EV_KEY, BTN_TOOL_QUADTAP, pressed as i32)
    }

    pub const fn btn_tool_quinttap(pressed: bool) -> Self {
        Self::new(EV_KEY, BTN_TOOL_QUINTTAP, pressed as i32)
    }

    pub const fn abs_x(value: i32) -> Self {
        Self::new(EV_ABS, ABS_X, value)
    }

    pub const fn abs_y(value: i32) -> Self {
        Self::new(EV_ABS, ABS_Y, value)
    }

    pub const fn msc_timestamp(value: i32) -> Self {
        Self::new(EV_MSC, MSC_TIMESTAMP, value)
    }

    pub const fn abs_mt_slot(slot: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_SLOT, slot)
    }

    pub const fn abs_mt_tracking_id(tracking_id: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_TRACKING_ID, tracking_id)
    }

    pub const fn abs_mt_position_x(x: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_POSITION_X, x)
    }

    pub const fn abs_mt_position_y(y: i32) -> Self {
        Self::new(EV_ABS, ABS_MT_POSITION_Y, y)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawFrame {
    pub events: Vec<RawEvent>,
    pub timestamp: Option<Duration>,
}

impl RawFrame {
    pub fn new(events: Vec<RawEvent>) -> Self {
        Self {
            events,
            timestamp: None,
        }
    }

    pub fn new_at(events: Vec<RawEvent>, timestamp: Duration) -> Self {
        Self {
            events,
            timestamp: Some(timestamp),
        }
    }
}

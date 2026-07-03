pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;

pub const SYN_REPORT: u16 = 0x00;
pub const SYN_DROPPED: u16 = 0x03;

pub const BTN_TOUCH: u16 = 0x14a;

pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_MT_SLOT: u16 = 0x2f;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;

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

    pub const fn abs_x(value: i32) -> Self {
        Self::new(EV_ABS, ABS_X, value)
    }

    pub const fn abs_y(value: i32) -> Self {
        Self::new(EV_ABS, ABS_Y, value)
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
}

impl RawFrame {
    pub fn new(events: Vec<RawEvent>) -> Self {
        Self { events }
    }
}

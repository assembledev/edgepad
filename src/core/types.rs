//! Public input, output, and recognition settings.

use std::time::Duration;

pub const MT_TOOL_PALM: i32 = 2;
pub const DEFAULT_PALM_REJECTION: bool = true;

pub const DEFAULT_TAP_MIN_DURATION_MS: u64 = 40;
pub const DEFAULT_TAP_MAX_DURATION_MS: u64 = 180;
pub const DEFAULT_DOUBLE_TAP_TIMEOUT_MS: u64 = 300;
pub const DEFAULT_DOUBLE_TAP_MAX_DISTANCE: f32 = 0.04;
pub const DEFAULT_SWIPE_MIN_DISTANCE: f32 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisRange {
    pub min: i32,
    pub max: i32,
}

impl AxisRange {
    pub(super) fn normalize(self, value: i32) -> f32 {
        let span = (self.max - self.min).max(1) as f32;
        (value - self.min) as f32 / span
    }

    pub(super) fn normalize_delta(self, start: i32, end: i32) -> f32 {
        let span = (self.max - self.min).max(1) as f32;
        (end - start) as f32 / span
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub slot_min: i32,
    pub slot_max: i32,
    pub x: AxisRange,
    pub y: AxisRange,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeWidths {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl EdgeWidths {
    pub fn all(width: f32) -> Self {
        Self {
            left: width,
            right: width,
            top: width,
            bottom: width,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Slot(i32),
    TrackingId(i32),
    X(i32),
    Y(i32),
    ToolType(i32),
    SynDropped,
}

impl Event {
    pub fn slot(slot: i32) -> Self {
        Self::Slot(slot)
    }

    pub fn tracking_id(tracking_id: i32) -> Self {
        Self::TrackingId(tracking_id)
    }

    pub fn x(x: i32) -> Self {
        Self::X(x)
    }

    pub fn y(y: i32) -> Self {
        Self::Y(y)
    }

    pub fn tool_type(tool_type: i32) -> Self {
        Self::ToolType(tool_type)
    }

    pub fn syn_dropped() -> Self {
        Self::SynDropped
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GestureDirection {
    Up,
    Down,
    Left,
    Right,
    Tap,
    DoubleTap,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ZoneSet(u8);

impl ZoneSet {
    pub const fn all() -> Self {
        Self(0b1111)
    }

    pub fn from_zones(zones: impl IntoIterator<Item = Zone>) -> Self {
        let mut set = Self::default();
        for zone in zones {
            set.0 |= zone_bit(zone);
        }
        set
    }

    pub const fn contains(self, zone: Zone) -> bool {
        self.0 & zone_bit(zone) != 0
    }
}

const fn zone_bit(zone: Zone) -> u8 {
    match zone {
        Zone::Left => 1 << 0,
        Zone::Right => 1 << 1,
        Zone::Top => 1 << 2,
        Zone::Bottom => 1 << 3,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SliderDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gesture {
    pub zone: Zone,
    pub direction: GestureDirection,
    pub slot: i32,
    pub tracking_id: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderSpec {
    pub zone: Zone,
    pub axis: SliderAxis,
    pub step: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineOptions {
    pub palm_rejection: bool,
    pub tap_min_duration: Duration,
    pub tap_max_duration: Duration,
    pub swipe_min_distance: f32,
    pub double_tap_timeout: Duration,
    pub double_tap_max_distance: f32,
    pub double_tap_zones: ZoneSet,
    pub single_tap_zones: ZoneSet,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            palm_rejection: DEFAULT_PALM_REJECTION,
            tap_min_duration: Duration::from_millis(DEFAULT_TAP_MIN_DURATION_MS),
            tap_max_duration: Duration::from_millis(DEFAULT_TAP_MAX_DURATION_MS),
            swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
            double_tap_timeout: Duration::from_millis(DEFAULT_DOUBLE_TAP_TIMEOUT_MS),
            double_tap_max_distance: DEFAULT_DOUBLE_TAP_MAX_DISTANCE,
            double_tap_zones: ZoneSet::default(),
            single_tap_zones: ZoneSet::all(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderStep {
    pub zone: Zone,
    pub direction: SliderDirection,
    pub slot: i32,
    pub tracking_id: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResyncContact {
    pub slot: i32,
    pub tracking_id: i32,
    pub x: i32,
    pub y: i32,
    pub tool_type: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlotSnapshot {
    pub slot: i32,
    pub x: i32,
    pub y: i32,
    pub tool_type: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameOutput {
    pub passthrough: Vec<Event>,
    pub gestures: Vec<Gesture>,
    pub slider_steps: Vec<SliderStep>,
    pub resync_required: bool,
}

impl FrameOutput {
    pub(super) fn empty() -> Self {
        Self {
            passthrough: Vec::new(),
            gestures: Vec::new(),
            slider_steps: Vec::new(),
            resync_required: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotError {
    InvalidSlotRange {
        min: i32,
        max: i32,
    },
    SlotOutOfRange {
        slot: i32,
        min: i32,
        max: i32,
    },
    SlotAlreadyActive {
        slot: i32,
        active_tracking_id: i32,
        new_tracking_id: i32,
    },
}

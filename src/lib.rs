//! edgepad core library.
//!
//! The first production surface is built test-first around Type-B
//! multi-touch slot lifecycle and edge ownership invariants.

pub mod config;
pub mod device;
pub mod dump;
pub mod proxy;
pub mod raw;
pub mod uinput;

pub mod core {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AxisRange {
        pub min: i32,
        pub max: i32,
    }

    impl AxisRange {
        fn normalize(self, value: i32) -> f32 {
            let span = (self.max - self.min).max(1) as f32;
            (value - self.min) as f32 / span
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
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Gesture {
        pub zone: Zone,
        pub direction: GestureDirection,
        pub slot: i32,
        pub tracking_id: i32,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FrameOutput {
        pub passthrough: Vec<Event>,
        pub gestures: Vec<Gesture>,
        pub resync_required: bool,
    }

    impl FrameOutput {
        fn empty() -> Self {
            Self {
                passthrough: Vec::new(),
                gestures: Vec::new(),
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Ownership {
        Unknown,
        Claimed(Zone),
        Passthrough,
    }

    #[derive(Debug, Clone)]
    struct SlotState {
        active: bool,
        tracking_id: Option<i32>,
        ownership: Ownership,
        start_x: Option<i32>,
        start_y: Option<i32>,
        current_x: Option<i32>,
        current_y: Option<i32>,
        held_events: Vec<Event>,
    }

    impl Default for SlotState {
        fn default() -> Self {
            Self {
                active: false,
                tracking_id: None,
                ownership: Ownership::Unknown,
                start_x: None,
                start_y: None,
                current_x: None,
                current_y: None,
                held_events: Vec::new(),
            }
        }
    }

    impl SlotState {
        fn reset(&mut self) {
            *self = Self::default();
        }
    }

    #[derive(Debug, Clone)]
    pub struct Engine {
        caps: Capabilities,
        edges: EdgeWidths,
        current_slot: i32,
        slots: Vec<SlotState>,
    }

    impl Engine {
        pub fn new(caps: Capabilities, edges: EdgeWidths) -> Self {
            assert!(
                caps.slot_min <= caps.slot_max,
                "invalid slot range: {}..={} ",
                caps.slot_min,
                caps.slot_max
            );
            let slot_count = (caps.slot_max - caps.slot_min + 1) as usize;
            Self {
                current_slot: caps.slot_min,
                caps,
                edges,
                slots: vec![SlotState::default(); slot_count],
            }
        }

        pub fn process_frame(&mut self, frame: &[Event]) -> Result<FrameOutput, SlotError> {
            let mut output = FrameOutput::empty();

            for event in frame.iter().copied() {
                match event {
                    Event::SynDropped => {
                        self.reset_for_resync();
                        output.resync_required = true;
                    }
                    Event::Slot(slot) => {
                        self.ensure_slot(slot)?;
                        self.current_slot = slot;
                        self.route_event_for_current_slot(event, &mut output)?;
                    }
                    Event::TrackingId(tracking_id) if tracking_id >= 0 => {
                        let slot = self.current_slot;
                        let slot_state = self.slot_mut(slot)?;
                        if slot_state.active {
                            return Err(SlotError::SlotAlreadyActive {
                                slot,
                                active_tracking_id: slot_state.tracking_id.unwrap_or_default(),
                                new_tracking_id: tracking_id,
                            });
                        }
                        slot_state.active = true;
                        slot_state.tracking_id = Some(tracking_id);
                        slot_state.ownership = Ownership::Unknown;
                        slot_state.held_events.push(event);
                    }
                    Event::TrackingId(-1) => {
                        self.release_current_slot(event, &mut output)?;
                    }
                    Event::TrackingId(_) => {
                        self.release_current_slot(event, &mut output)?;
                    }
                    Event::X(x) => {
                        let slot_state = self.slot_mut(self.current_slot)?;
                        slot_state.current_x = Some(x);
                        if slot_state.active && slot_state.start_x.is_none() {
                            slot_state.start_x = Some(x);
                        }
                        self.route_event_for_current_slot(event, &mut output)?;
                    }
                    Event::Y(y) => {
                        let slot_state = self.slot_mut(self.current_slot)?;
                        slot_state.current_y = Some(y);
                        if slot_state.active && slot_state.start_y.is_none() {
                            slot_state.start_y = Some(y);
                        }
                        self.route_event_for_current_slot(event, &mut output)?;
                    }
                }

                self.decide_ownership_if_ready(&mut output)?;
            }

            Ok(output)
        }

        fn release_current_slot(
            &mut self,
            event: Event,
            output: &mut FrameOutput,
        ) -> Result<(), SlotError> {
            let slot = self.current_slot;
            let slot_state = self.slot_mut(slot)?;

            match slot_state.ownership {
                Ownership::Claimed(zone) => {
                    if let Some(gesture) = classify_gesture(slot, zone, slot_state) {
                        output.gestures.push(gesture);
                    }
                }
                Ownership::Passthrough => output.passthrough.push(event),
                Ownership::Unknown => slot_state.held_events.push(event),
            }

            if slot_state.active {
                slot_state.reset();
            }
            Ok(())
        }

        fn route_event_for_current_slot(
            &mut self,
            event: Event,
            output: &mut FrameOutput,
        ) -> Result<(), SlotError> {
            let slot_state = self.slot_mut(self.current_slot)?;
            match slot_state.ownership {
                Ownership::Passthrough => output.passthrough.push(event),
                Ownership::Unknown => slot_state.held_events.push(event),
                Ownership::Claimed(_) => {}
            }
            Ok(())
        }

        fn decide_ownership_if_ready(&mut self, output: &mut FrameOutput) -> Result<(), SlotError> {
            let slot = self.current_slot;
            let Some(zone) = self.zone_for_current_slot()? else {
                let slot_state = self.slot_mut(slot)?;
                if slot_state.active
                    && matches!(slot_state.ownership, Ownership::Unknown)
                    && slot_state.start_x.is_some()
                    && slot_state.start_y.is_some()
                {
                    slot_state.ownership = Ownership::Passthrough;
                    output.passthrough.append(&mut slot_state.held_events);
                }
                return Ok(());
            };

            let slot_state = self.slot_mut(slot)?;
            if slot_state.active
                && matches!(slot_state.ownership, Ownership::Unknown)
                && slot_state.start_x.is_some()
                && slot_state.start_y.is_some()
            {
                slot_state.ownership = Ownership::Claimed(zone);
                slot_state.held_events.clear();
            }
            Ok(())
        }

        fn zone_for_current_slot(&self) -> Result<Option<Zone>, SlotError> {
            let slot_state = self.slot(self.current_slot)?;
            if !slot_state.active || slot_state.start_x.is_none() || slot_state.start_y.is_none() {
                return Ok(None);
            }
            let x = self.caps.x.normalize(slot_state.start_x.unwrap());
            let y = self.caps.y.normalize(slot_state.start_y.unwrap());

            let zone = if x < self.edges.left {
                Some(Zone::Left)
            } else if x > 1.0 - self.edges.right {
                Some(Zone::Right)
            } else if y < self.edges.top {
                Some(Zone::Top)
            } else if y > 1.0 - self.edges.bottom {
                Some(Zone::Bottom)
            } else {
                None
            };
            Ok(zone)
        }

        fn ensure_slot(&self, slot: i32) -> Result<(), SlotError> {
            if slot < self.caps.slot_min || slot > self.caps.slot_max {
                return Err(SlotError::SlotOutOfRange {
                    slot,
                    min: self.caps.slot_min,
                    max: self.caps.slot_max,
                });
            }
            Ok(())
        }

        fn reset_for_resync(&mut self) {
            for slot in &mut self.slots {
                slot.reset();
            }
            self.current_slot = self.caps.slot_min;
        }

        fn slot_index(&self, slot: i32) -> Result<usize, SlotError> {
            self.ensure_slot(slot)?;
            Ok((slot - self.caps.slot_min) as usize)
        }

        fn slot(&self, slot: i32) -> Result<&SlotState, SlotError> {
            let index = self.slot_index(slot)?;
            Ok(&self.slots[index])
        }

        fn slot_mut(&mut self, slot: i32) -> Result<&mut SlotState, SlotError> {
            let index = self.slot_index(slot)?;
            Ok(&mut self.slots[index])
        }
    }

    fn classify_gesture(slot: i32, zone: Zone, state: &SlotState) -> Option<Gesture> {
        let start_x = state.start_x?;
        let start_y = state.start_y?;
        let current_x = state.current_x.unwrap_or(start_x);
        let current_y = state.current_y.unwrap_or(start_y);
        let dx = current_x - start_x;
        let dy = current_y - start_y;

        let direction = if dx.abs() < 20 && dy.abs() < 20 {
            GestureDirection::Tap
        } else if dx.abs() >= dy.abs() {
            if dx >= 0 {
                GestureDirection::Right
            } else {
                GestureDirection::Left
            }
        } else if dy >= 0 {
            GestureDirection::Down
        } else {
            GestureDirection::Up
        };

        Some(Gesture {
            zone,
            direction,
            slot,
            tracking_id: state.tracking_id?,
        })
    }
}

pub mod replay {
    use crate::core::{AxisRange, Capabilities, Engine, Event, FrameOutput, SlotError};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ReplayError {
        UnknownEvent {
            line: usize,
            name: String,
        },
        MissingValue {
            line: usize,
            name: String,
        },
        InvalidValue {
            line: usize,
            name: String,
            value: String,
        },
        InvalidMetadata {
            line: usize,
            name: String,
            value: String,
        },
        MissingMetadataField {
            field: &'static str,
        },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ReplayFile {
        pub capabilities: Option<Capabilities>,
        pub frames: Vec<Vec<Event>>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct ReplayStats {
        pub total_events: usize,
        pub slot_events: usize,
        pub tracking_starts: usize,
        pub tracking_ends: usize,
        pub x_events: usize,
        pub y_events: usize,
        pub syn_dropped_events: usize,
    }

    pub fn replay_stats(frames: &[Vec<Event>]) -> ReplayStats {
        let mut stats = ReplayStats::default();

        for event in frames.iter().flatten() {
            stats.total_events += 1;
            match event {
                Event::Slot(_) => stats.slot_events += 1,
                Event::TrackingId(id) if *id >= 0 => stats.tracking_starts += 1,
                Event::TrackingId(_) => stats.tracking_ends += 1,
                Event::X(_) => stats.x_events += 1,
                Event::Y(_) => stats.y_events += 1,
                Event::SynDropped => stats.syn_dropped_events += 1,
            }
        }

        stats
    }

    pub fn parse_replay_file(input: &str) -> Result<ReplayFile, ReplayError> {
        Ok(ReplayFile {
            capabilities: parse_capabilities_metadata(input)?,
            frames: parse_frames(input)?,
        })
    }

    pub fn parse_frames(input: &str) -> Result<Vec<Vec<Event>>, ReplayError> {
        let mut frames = Vec::new();
        let mut current = Vec::new();

        for (index, raw_line) in input.lines().enumerate() {
            let line_number = index + 1;
            let line = raw_line
                .split_once('#')
                .map_or(raw_line, |(before_comment, _)| before_comment)
                .trim();

            if line.is_empty() {
                continue;
            }

            let mut parts = line.split_whitespace();
            let name = parts.next().expect("non-empty line has first token");

            match name {
                "SYN_REPORT" => {
                    if !current.is_empty() {
                        frames.push(std::mem::take(&mut current));
                    }
                }
                "SYN_DROPPED" => {
                    if !current.is_empty() {
                        frames.push(std::mem::take(&mut current));
                    }
                    frames.push(vec![Event::syn_dropped()]);
                }
                "ABS_MT_SLOT" => current.push(Event::slot(parse_i32_value(
                    line_number,
                    name,
                    parts.next(),
                )?)),
                "ABS_MT_TRACKING_ID" => {
                    current.push(Event::tracking_id(parse_i32_value(
                        line_number,
                        name,
                        parts.next(),
                    )?));
                }
                "ABS_MT_POSITION_X" => {
                    current.push(Event::x(parse_i32_value(line_number, name, parts.next())?))
                }
                "ABS_MT_POSITION_Y" => {
                    current.push(Event::y(parse_i32_value(line_number, name, parts.next())?))
                }
                _ => {
                    return Err(ReplayError::UnknownEvent {
                        line: line_number,
                        name: name.to_string(),
                    });
                }
            }
        }

        if !current.is_empty() {
            frames.push(current);
        }

        Ok(frames)
    }

    #[derive(Default)]
    struct CapabilityMetadata {
        slots: Option<AxisRange>,
        x: Option<AxisRange>,
        y: Option<AxisRange>,
        saw_any: bool,
    }

    fn parse_capabilities_metadata(input: &str) -> Result<Option<Capabilities>, ReplayError> {
        let mut metadata = CapabilityMetadata::default();

        for (index, raw_line) in input.lines().enumerate() {
            let line_number = index + 1;
            let Some(comment) = raw_line.trim_start().strip_prefix('#') else {
                continue;
            };
            let Some((name, value)) = comment.trim().split_once(':') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();

            match name {
                "slots" => {
                    metadata.saw_any = true;
                    metadata.slots = Some(parse_metadata_range(line_number, name, value)?);
                }
                "x" => {
                    metadata.saw_any = true;
                    metadata.x = Some(parse_metadata_range(line_number, name, value)?);
                }
                "y" => {
                    metadata.saw_any = true;
                    metadata.y = Some(parse_metadata_range(line_number, name, value)?);
                }
                _ => {}
            }
        }

        if !metadata.saw_any {
            return Ok(None);
        }

        let slots = metadata
            .slots
            .ok_or(ReplayError::MissingMetadataField { field: "slots" })?;
        let x = metadata
            .x
            .ok_or(ReplayError::MissingMetadataField { field: "x" })?;
        let y = metadata
            .y
            .ok_or(ReplayError::MissingMetadataField { field: "y" })?;

        Ok(Some(Capabilities {
            slot_min: slots.min,
            slot_max: slots.max,
            x,
            y,
        }))
    }

    fn parse_metadata_range(
        line: usize,
        name: &str,
        value: &str,
    ) -> Result<AxisRange, ReplayError> {
        let Some((min, max)) = value.split_once("..=") else {
            return Err(invalid_metadata(line, name, value));
        };
        let min = min
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid_metadata(line, name, value))?;
        let max = max
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid_metadata(line, name, value))?;

        if min > max {
            return Err(invalid_metadata(line, name, value));
        }

        Ok(AxisRange { min, max })
    }

    fn invalid_metadata(line: usize, name: &str, value: &str) -> ReplayError {
        ReplayError::InvalidMetadata {
            line,
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    pub fn run_frames(
        engine: &mut Engine,
        frames: &[Vec<Event>],
    ) -> Result<Vec<FrameOutput>, SlotError> {
        frames
            .iter()
            .map(|frame| engine.process_frame(frame))
            .collect()
    }

    fn parse_i32_value(line: usize, name: &str, value: Option<&str>) -> Result<i32, ReplayError> {
        let value = value.ok_or_else(|| ReplayError::MissingValue {
            line,
            name: name.to_string(),
        })?;

        value.parse::<i32>().map_err(|_| ReplayError::InvalidValue {
            line,
            name: name.to_string(),
            value: value.to_string(),
        })
    }
}

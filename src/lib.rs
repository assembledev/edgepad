//! edgepad core library.
//!
//! The first production surface is built test-first around Type-B
//! multi-touch slot lifecycle and edge ownership invariants.

pub mod actions;
pub mod config;
pub mod device;
pub mod doctor;
pub mod dump;
pub mod notify;
pub mod proxy;
pub mod raw;
pub mod status;
pub mod uinput;

pub mod core {
    use std::time::Duration;

    pub const DEFAULT_TAP_MIN_DURATION_MS: u64 = 80;
    pub const DEFAULT_SWIPE_MIN_DISTANCE: f32 = 0.02;

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

        fn normalize_delta(self, start: i32, end: i32) -> f32 {
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
        pub tap_min_duration: Duration,
        pub swipe_min_distance: f32,
    }

    impl Default for EngineOptions {
        fn default() -> Self {
            Self {
                tap_min_duration: Duration::from_millis(DEFAULT_TAP_MIN_DURATION_MS),
                swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
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
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FrameOutput {
        pub passthrough: Vec<Event>,
        pub gestures: Vec<Gesture>,
        pub slider_steps: Vec<SliderStep>,
        pub resync_required: bool,
    }

    impl FrameOutput {
        fn empty() -> Self {
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
        started_at: Option<Duration>,
        slider_anchor: Option<f32>,
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
                started_at: None,
                slider_anchor: None,
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
        options: EngineOptions,
        sliders: Vec<SliderSpec>,
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
                options: EngineOptions::default(),
                sliders: Vec::new(),
                slots: vec![SlotState::default(); slot_count],
            }
        }

        pub fn with_sliders(
            caps: Capabilities,
            edges: EdgeWidths,
            sliders: Vec<SliderSpec>,
        ) -> Self {
            for slider in &sliders {
                assert!(
                    slider.step.is_finite() && slider.step > 0.0 && slider.step <= 1.0,
                    "invalid slider step for {:?}: {}",
                    slider.zone,
                    slider.step
                );
            }
            let mut engine = Self::new(caps, edges);
            engine.sliders = sliders;
            engine
        }

        pub fn with_options(
            caps: Capabilities,
            edges: EdgeWidths,
            sliders: Vec<SliderSpec>,
            options: EngineOptions,
        ) -> Self {
            let mut engine = Self::with_sliders(caps, edges, sliders);
            engine.options = options;
            engine
        }

        pub fn process_frame(&mut self, frame: &[Event]) -> Result<FrameOutput, SlotError> {
            self.process_frame_with_time(frame, None)
        }

        pub fn process_frame_at(
            &mut self,
            frame: &[Event],
            timestamp: Duration,
        ) -> Result<FrameOutput, SlotError> {
            self.process_frame_with_time(frame, Some(timestamp))
        }

        pub fn restore_passthrough_contacts(
            &mut self,
            contacts: &[ResyncContact],
        ) -> Result<FrameOutput, SlotError> {
            self.reset_for_resync();
            let mut output = FrameOutput::empty();

            for contact in contacts {
                self.ensure_slot(contact.slot)?;
                self.current_slot = contact.slot;
                let slot = self.slot_mut(contact.slot)?;
                slot.active = true;
                slot.tracking_id = Some(contact.tracking_id);
                slot.ownership = Ownership::Passthrough;
                slot.start_x = Some(contact.x);
                slot.start_y = Some(contact.y);
                slot.current_x = Some(contact.x);
                slot.current_y = Some(contact.y);

                output.passthrough.extend([
                    Event::slot(contact.slot),
                    Event::tracking_id(contact.tracking_id),
                    Event::x(contact.x),
                    Event::y(contact.y),
                ]);
            }

            Ok(output)
        }

        fn process_frame_with_time(
            &mut self,
            frame: &[Event],
            timestamp: Option<Duration>,
        ) -> Result<FrameOutput, SlotError> {
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
                        slot_state.started_at = timestamp;
                        slot_state.held_events.push(event);
                    }
                    Event::TrackingId(-1) => {
                        self.release_current_slot(event, timestamp, &mut output)?;
                    }
                    Event::TrackingId(_) => {
                        self.release_current_slot(event, timestamp, &mut output)?;
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
                self.emit_slider_steps_if_ready(&mut output)?;
            }

            Ok(output)
        }

        fn release_current_slot(
            &mut self,
            event: Event,
            timestamp: Option<Duration>,
            output: &mut FrameOutput,
        ) -> Result<(), SlotError> {
            let slot = self.current_slot;
            match self.slot(slot)?.ownership {
                Ownership::Claimed(zone) => {
                    let releases_slider_zone = self.slider_for_zone(zone).is_some();
                    let options = self.options;
                    let capabilities = self.caps;
                    let slot_state = self.slot_mut(slot)?;
                    if let Some(gesture) =
                        classify_gesture(capabilities, slot, zone, slot_state, options, timestamp)
                    {
                        if !releases_slider_zone || gesture.direction == GestureDirection::Tap {
                            output.gestures.push(gesture);
                        }
                    }
                    if slot_state.active {
                        slot_state.reset();
                    }
                }
                Ownership::Passthrough => {
                    self.push_passthrough_event_for_current_slot(event, output);
                    if self.slot(slot)?.active {
                        self.slot_mut(slot)?.reset();
                    }
                }
                Ownership::Unknown => {
                    let slot_state = self.slot_mut(slot)?;
                    slot_state.held_events.push(event);
                    if slot_state.active {
                        slot_state.reset();
                    }
                }
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
                Ownership::Passthrough => {
                    self.push_passthrough_event_for_current_slot(event, output)
                }
                Ownership::Unknown => slot_state.held_events.push(event),
                Ownership::Claimed(_) => {}
            }
            Ok(())
        }

        fn push_passthrough_event_for_current_slot(&self, event: Event, output: &mut FrameOutput) {
            self.push_passthrough_event_for_slot(self.current_slot, event, output);
        }

        fn push_passthrough_event_for_slot(
            &self,
            slot: i32,
            event: Event,
            output: &mut FrameOutput,
        ) {
            match event {
                Event::Slot(_) | Event::SynDropped => output.passthrough.push(event),
                Event::TrackingId(_) | Event::X(_) | Event::Y(_) => {
                    if last_passthrough_slot(&output.passthrough) != Some(slot) {
                        output.passthrough.push(Event::slot(slot));
                    }
                    output.passthrough.push(event);
                }
            }
        }

        fn decide_ownership_if_ready(&mut self, output: &mut FrameOutput) -> Result<(), SlotError> {
            let slot = self.current_slot;
            let Some(zone) = self.zone_for_current_slot()? else {
                let held_events = {
                    let slot_state = self.slot_mut(slot)?;
                    if slot_state.active
                        && matches!(slot_state.ownership, Ownership::Unknown)
                        && slot_state.start_x.is_some()
                        && slot_state.start_y.is_some()
                    {
                        slot_state.ownership = Ownership::Passthrough;
                        std::mem::take(&mut slot_state.held_events)
                    } else {
                        Vec::new()
                    }
                };
                for event in held_events {
                    self.push_passthrough_event_for_slot(slot, event, output);
                }
                return Ok(());
            };

            let slider_anchor = {
                let slot_state = self.slot(slot)?;
                if slot_state.active
                    && matches!(slot_state.ownership, Ownership::Unknown)
                    && slot_state.start_x.is_some()
                    && slot_state.start_y.is_some()
                {
                    self.slider_for_zone(zone)
                        .and_then(|spec| slider_position(self.caps, spec.axis, slot_state))
                } else {
                    None
                }
            };

            let slot_state = self.slot_mut(slot)?;
            if slot_state.active
                && matches!(slot_state.ownership, Ownership::Unknown)
                && slot_state.start_x.is_some()
                && slot_state.start_y.is_some()
            {
                slot_state.ownership = Ownership::Claimed(zone);
                slot_state.slider_anchor = slider_anchor;
                slot_state.held_events.clear();
            }
            Ok(())
        }

        fn emit_slider_steps_if_ready(
            &mut self,
            output: &mut FrameOutput,
        ) -> Result<(), SlotError> {
            let slot = self.current_slot;
            let (zone, spec, position, tracking_id) = {
                let slot_state = self.slot(slot)?;
                let Ownership::Claimed(zone) = slot_state.ownership else {
                    return Ok(());
                };
                let Some(spec) = self.slider_for_zone(zone) else {
                    return Ok(());
                };
                let Some(position) = slider_position(self.caps, spec.axis, slot_state) else {
                    return Ok(());
                };
                let Some(tracking_id) = slot_state.tracking_id else {
                    return Ok(());
                };
                (zone, spec, position, tracking_id)
            };

            let slot_state = self.slot_mut(slot)?;
            let Some(mut anchor) = slot_state.slider_anchor else {
                slot_state.slider_anchor = Some(position);
                return Ok(());
            };

            while position - anchor >= spec.step {
                output.slider_steps.push(SliderStep {
                    zone,
                    direction: positive_slider_direction(spec.axis),
                    slot,
                    tracking_id,
                });
                anchor += spec.step;
            }

            while anchor - position >= spec.step {
                output.slider_steps.push(SliderStep {
                    zone,
                    direction: negative_slider_direction(spec.axis),
                    slot,
                    tracking_id,
                });
                anchor -= spec.step;
            }

            slot_state.slider_anchor = Some(anchor);
            Ok(())
        }

        fn slider_for_zone(&self, zone: Zone) -> Option<SliderSpec> {
            self.sliders
                .iter()
                .copied()
                .find(|slider| slider.zone == zone)
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

    fn last_passthrough_slot(events: &[Event]) -> Option<i32> {
        events.iter().rev().find_map(|event| match event {
            Event::Slot(slot) => Some(*slot),
            _ => None,
        })
    }

    fn classify_gesture(
        capabilities: Capabilities,
        slot: i32,
        zone: Zone,
        state: &SlotState,
        options: EngineOptions,
        released_at: Option<Duration>,
    ) -> Option<Gesture> {
        let start_x = state.start_x?;
        let start_y = state.start_y?;
        let current_x = state.current_x.unwrap_or(start_x);
        let current_y = state.current_y.unwrap_or(start_y);
        let dx = capabilities.x.normalize_delta(start_x, current_x);
        let dy = capabilities.y.normalize_delta(start_y, current_y);

        let direction =
            if dx.abs() < options.swipe_min_distance && dy.abs() < options.swipe_min_distance {
                if !tap_duration_is_valid(state.started_at, released_at, options.tap_min_duration) {
                    return None;
                }
                GestureDirection::Tap
            } else if dx.abs() >= dy.abs() {
                if dx >= 0.0 {
                    GestureDirection::Right
                } else {
                    GestureDirection::Left
                }
            } else if dy >= 0.0 {
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

    fn tap_duration_is_valid(
        started_at: Option<Duration>,
        released_at: Option<Duration>,
        min_duration: Duration,
    ) -> bool {
        if min_duration.is_zero() {
            return true;
        }

        match (started_at, released_at) {
            (Some(started_at), Some(released_at)) => released_at
                .checked_sub(started_at)
                .is_some_and(|duration| duration >= min_duration),
            _ => true,
        }
    }

    fn slider_position(caps: Capabilities, axis: SliderAxis, state: &SlotState) -> Option<f32> {
        match axis {
            SliderAxis::Horizontal => state.current_x.map(|x| caps.x.normalize(x)),
            SliderAxis::Vertical => state.current_y.map(|y| caps.y.normalize(y)),
        }
    }

    fn positive_slider_direction(axis: SliderAxis) -> SliderDirection {
        match axis {
            SliderAxis::Horizontal => SliderDirection::Right,
            SliderAxis::Vertical => SliderDirection::Down,
        }
    }

    fn negative_slider_direction(axis: SliderAxis) -> SliderDirection {
        match axis {
            SliderAxis::Horizontal => SliderDirection::Left,
            SliderAxis::Vertical => SliderDirection::Up,
        }
    }
}

pub mod replay {
    use std::time::Duration;

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
        NonMonotonicTimestamp {
            line: usize,
            previous_us: u64,
            current_us: u64,
        },
        UnterminatedFrame {
            line: usize,
        },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ReplayFrame {
        pub events: Vec<Event>,
        pub timestamp: Duration,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ReplayFile {
        pub capabilities: Option<Capabilities>,
        pub frames: Vec<ReplayFrame>,
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

    pub fn replay_stats(frames: &[ReplayFrame]) -> ReplayStats {
        let mut stats = ReplayStats::default();

        for frame in frames {
            for event in &frame.events {
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
        }

        stats
    }

    pub fn parse_replay_file(input: &str) -> Result<ReplayFile, ReplayError> {
        Ok(ReplayFile {
            capabilities: parse_capabilities_metadata(input)?,
            frames: parse_frames(input)?,
        })
    }

    pub fn parse_frames(input: &str) -> Result<Vec<ReplayFrame>, ReplayError> {
        let mut frames = Vec::new();
        let mut current = Vec::new();
        let mut last_timestamp = None;

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
                    let timestamp = parse_timestamp(line_number, name, parts.next())?;
                    validate_timestamp(line_number, timestamp, &mut last_timestamp)?;
                    if !current.is_empty() {
                        frames.push(ReplayFrame {
                            events: std::mem::take(&mut current),
                            timestamp,
                        });
                    }
                }
                "SYN_DROPPED" => {
                    let timestamp = parse_timestamp(line_number, name, parts.next())?;
                    validate_timestamp(line_number, timestamp, &mut last_timestamp)?;
                    current.clear();
                    frames.push(ReplayFrame {
                        events: vec![Event::syn_dropped()],
                        timestamp,
                    });
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
            return Err(ReplayError::UnterminatedFrame {
                line: input.lines().count().max(1),
            });
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
        frames: &[ReplayFrame],
    ) -> Result<Vec<FrameOutput>, SlotError> {
        frames
            .iter()
            .map(|frame| engine.process_frame_at(&frame.events, frame.timestamp))
            .collect()
    }

    fn parse_timestamp(
        line: usize,
        name: &str,
        value: Option<&str>,
    ) -> Result<Duration, ReplayError> {
        let value = value.ok_or_else(|| ReplayError::MissingValue {
            line,
            name: format!("{name} timestamp_us"),
        })?;
        let micros = value
            .parse::<u64>()
            .map_err(|_| ReplayError::InvalidValue {
                line,
                name: format!("{name} timestamp_us"),
                value: value.to_string(),
            })?;

        Ok(Duration::from_micros(micros))
    }

    fn validate_timestamp(
        line: usize,
        timestamp: Duration,
        last_timestamp: &mut Option<Duration>,
    ) -> Result<(), ReplayError> {
        if let Some(previous) = *last_timestamp {
            if timestamp < previous {
                return Err(ReplayError::NonMonotonicTimestamp {
                    line,
                    previous_us: previous.as_micros() as u64,
                    current_us: timestamp.as_micros() as u64,
                });
            }
        }
        *last_timestamp = Some(timestamp);
        Ok(())
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

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
    use std::collections::BTreeSet;
    use std::time::Duration;

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
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct SlotPosition {
        pub slot: i32,
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ContactPhase {
        TapCandidate,
        Motion,
        ConsumedBySlider,
    }

    #[derive(Debug, Clone)]
    struct SlotState {
        active: bool,
        tracking_id: Option<i32>,
        ownership: Ownership,
        contact_phase: ContactPhase,
        start_x: Option<i32>,
        start_y: Option<i32>,
        current_x: Option<i32>,
        current_y: Option<i32>,
        retained_x: Option<i32>,
        retained_y: Option<i32>,
        began_in_frame: bool,
        started_at: Option<Duration>,
        tap_sequence_eligible: bool,
        tap_sequence_match: bool,
        slider_anchor: Option<f32>,
        held_events: Vec<Event>,
    }

    impl Default for SlotState {
        fn default() -> Self {
            Self {
                active: false,
                tracking_id: None,
                ownership: Ownership::Unknown,
                contact_phase: ContactPhase::TapCandidate,
                start_x: None,
                start_y: None,
                current_x: None,
                current_y: None,
                retained_x: None,
                retained_y: None,
                began_in_frame: false,
                started_at: None,
                tap_sequence_eligible: true,
                tap_sequence_match: false,
                slider_anchor: None,
                held_events: Vec::new(),
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct PendingTap {
        gesture: Gesture,
        released_at: Duration,
        x: i32,
        y: i32,
        emit_single: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct ClassifiedContact {
        gesture: Gesture,
        sequence_match: bool,
        sequence_eligible: bool,
        x: Option<i32>,
        y: Option<i32>,
    }

    impl SlotState {
        fn finish_contact(&mut self) {
            // Type-B slots retain axis state after TRACKING_ID -1. Linux may
            // suppress an unchanged X or Y when the next contact begins, so
            // keep those values while clearing all contact-owned state.
            let retained_x = self.retained_x;
            let retained_y = self.retained_y;
            *self = Self {
                retained_x,
                retained_y,
                ..Self::default()
            };
        }

        fn observe_position(&mut self, capabilities: Capabilities, swipe_min_distance: f32) {
            if self.contact_phase != ContactPhase::TapCandidate {
                return;
            }

            let moved_x = match (self.start_x, self.current_x) {
                (Some(start), Some(current)) => {
                    capabilities.x.normalize_delta(start, current).abs() >= swipe_min_distance
                }
                _ => false,
            };
            let moved_y = match (self.start_y, self.current_y) {
                (Some(start), Some(current)) => {
                    capabilities.y.normalize_delta(start, current).abs() >= swipe_min_distance
                }
                _ => false,
            };

            if moved_x || moved_y {
                self.contact_phase = ContactPhase::Motion;
            }
        }

        fn consume_by_slider(&mut self) {
            self.contact_phase = ContactPhase::ConsumedBySlider;
        }
    }

    #[derive(Debug, Clone)]
    pub struct Engine {
        caps: Capabilities,
        edges: EdgeWidths,
        options: EngineOptions,
        sliders: Vec<SliderSpec>,
        buttonpad: bool,
        pressed_physical_buttons: BTreeSet<u16>,
        current_slot: i32,
        slots: Vec<SlotState>,
        pending_tap: Option<PendingTap>,
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
                buttonpad: false,
                pressed_physical_buttons: BTreeSet::new(),
                slots: vec![SlotState::default(); slot_count],
                pending_tap: None,
            }
        }

        pub fn with_sliders(
            caps: Capabilities,
            edges: EdgeWidths,
            sliders: Vec<SliderSpec>,
        ) -> Self {
            validate_slider_specs(&sliders);
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
            validate_engine_options(options);
            let mut engine = Self::with_sliders(caps, edges, sliders);
            engine.options = options;
            engine
        }

        pub(crate) fn reconfigure_recognition(
            &mut self,
            edges: EdgeWidths,
            sliders: Vec<SliderSpec>,
            options: EngineOptions,
        ) {
            assert!(
                self.slots.iter().all(|slot| !slot.active) && self.pending_tap.is_none(),
                "recognition reconfiguration requires an idle engine"
            );
            validate_slider_specs(&sliders);
            validate_engine_options(options);
            self.edges = edges;
            self.sliders = sliders;
            self.options = options;
        }

        pub(crate) fn seed_slot_positions(
            &mut self,
            positions: &[SlotPosition],
        ) -> Result<(), SlotError> {
            for position in positions {
                let slot = self.slot_mut(position.slot)?;
                slot.retained_x = Some(position.x);
                slot.retained_y = Some(position.y);
            }
            Ok(())
        }

        pub fn set_buttonpad(&mut self, buttonpad: bool) {
            self.buttonpad = buttonpad;
            if !buttonpad {
                self.pressed_physical_buttons.clear();
            }
        }

        pub fn update_physical_button(&mut self, code: u16, pressed: bool) -> Vec<Event> {
            if !self.buttonpad {
                return Vec::new();
            }

            if !pressed {
                self.pressed_physical_buttons.remove(&code);
                return Vec::new();
            }

            self.pressed_physical_buttons.insert(code);
            self.promote_claimed_contacts()
        }

        pub fn restore_pressed_physical_buttons(&mut self, codes: &[u16]) {
            self.pressed_physical_buttons.clear();
            if self.buttonpad {
                self.pressed_physical_buttons.extend(codes.iter().copied());
            }
        }

        #[cfg(test)]
        pub(crate) fn pressed_physical_buttons(&self) -> Vec<u16> {
            self.pressed_physical_buttons.iter().copied().collect()
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

        pub fn next_deadline(&self) -> Option<Duration> {
            let pending = self.pending_tap?;
            if let Some(second_tap_deadline) = self.matching_contact_deadline() {
                return Some(second_tap_deadline);
            }
            Some(
                pending
                    .released_at
                    .saturating_add(self.options.double_tap_timeout),
            )
        }

        pub fn advance_time(&mut self, timestamp: Duration) -> FrameOutput {
            let mut output = FrameOutput::empty();
            self.expire_pending_tap(timestamp, &mut output);
            output
        }

        pub fn is_recognition_idle(&self) -> bool {
            self.pending_tap.is_none()
        }

        pub fn interrupt_tap_sequence(&mut self) -> Vec<Gesture> {
            // A physical interaction can arrive after a matching second
            // contact is already down. Keep separate-button edge ownership
            // intact, but do not let that contact seed another temporal pair.
            for slot in self.slots.iter_mut().filter(|slot| slot.active) {
                slot.tap_sequence_eligible = false;
                slot.tap_sequence_match = false;
            }
            let mut output = FrameOutput::empty();
            self.flush_pending_tap(&mut output);
            output.gestures
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
                slot.retained_x = Some(contact.x);
                slot.retained_y = Some(contact.y);

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
            if let Some(timestamp) = timestamp {
                self.expire_pending_tap(timestamp, &mut output);
            }

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
                        if self.slot(slot)?.active {
                            return Err(SlotError::SlotAlreadyActive {
                                slot,
                                active_tracking_id: self
                                    .slot(slot)?
                                    .tracking_id
                                    .unwrap_or_default(),
                                new_tracking_id: tracking_id,
                            });
                        }
                        let another_contact_active = self.slots.iter().any(|slot| slot.active);
                        if another_contact_active {
                            for active_slot in self.slots.iter_mut().filter(|slot| slot.active) {
                                active_slot.tap_sequence_eligible = false;
                                active_slot.tap_sequence_match = false;
                            }
                            self.flush_pending_tap(&mut output);
                        }
                        let slot_state = self.slot_mut(slot)?;
                        slot_state.active = true;
                        slot_state.tracking_id = Some(tracking_id);
                        slot_state.ownership = Ownership::Unknown;
                        slot_state.contact_phase = ContactPhase::TapCandidate;
                        slot_state.began_in_frame = true;
                        slot_state.started_at = timestamp;
                        slot_state.tap_sequence_eligible = !another_contact_active;
                        slot_state.tap_sequence_match = false;
                        slot_state.held_events.push(event);
                    }
                    Event::TrackingId(-1) => {
                        self.release_current_slot(event, timestamp, &mut output)?;
                    }
                    Event::TrackingId(_) => {
                        self.release_current_slot(event, timestamp, &mut output)?;
                    }
                    Event::X(x) => {
                        let capabilities = self.caps;
                        let swipe_min_distance = self.options.swipe_min_distance;
                        let sequence_invalidated = {
                            let slot_state = self.slot_mut(self.current_slot)?;
                            slot_state.retained_x = Some(x);
                            slot_state.current_x = Some(x);
                            if slot_state.active && slot_state.start_x.is_none() {
                                slot_state.start_x = Some(x);
                            }
                            slot_state.observe_position(capabilities, swipe_min_distance);
                            slot_state.tap_sequence_match
                                && slot_state.contact_phase != ContactPhase::TapCandidate
                        };
                        if sequence_invalidated {
                            self.flush_pending_tap(&mut output);
                        }
                        self.route_event_for_current_slot(event, &mut output)?;
                    }
                    Event::Y(y) => {
                        let capabilities = self.caps;
                        let swipe_min_distance = self.options.swipe_min_distance;
                        let sequence_invalidated = {
                            let slot_state = self.slot_mut(self.current_slot)?;
                            slot_state.retained_y = Some(y);
                            slot_state.current_y = Some(y);
                            if slot_state.active && slot_state.start_y.is_none() {
                                slot_state.start_y = Some(y);
                            }
                            slot_state.observe_position(capabilities, swipe_min_distance);
                            slot_state.tap_sequence_match
                                && slot_state.contact_phase != ContactPhase::TapCandidate
                        };
                        if sequence_invalidated {
                            self.flush_pending_tap(&mut output);
                        }
                        self.route_event_for_current_slot(event, &mut output)?;
                    }
                }

                if !self.slot(self.current_slot)?.began_in_frame {
                    self.decide_ownership_if_ready(&mut output)?;
                    self.emit_slider_steps_if_ready(&mut output)?;
                }
            }

            self.finalize_contacts_started_in_frame(&mut output)?;

            Ok(output)
        }

        fn finalize_contacts_started_in_frame(
            &mut self,
            output: &mut FrameOutput,
        ) -> Result<(), SlotError> {
            let previous_slot = self.current_slot;
            for index in 0..self.slots.len() {
                let slot_number = self.caps.slot_min + index as i32;
                let should_finalize = {
                    let slot = &mut self.slots[index];
                    if !slot.active || !slot.began_in_frame {
                        false
                    } else {
                        // Resolve any omitted axes only after the whole SYN
                        // frame. Doing it at TRACKING_ID would briefly classify
                        // a moved contact from the preceding contact's position.
                        slot.current_x = slot.current_x.or(slot.retained_x);
                        slot.current_y = slot.current_y.or(slot.retained_y);
                        slot.start_x = slot.start_x.or(slot.current_x);
                        slot.start_y = slot.start_y.or(slot.current_y);
                        slot.began_in_frame = false;
                        true
                    }
                };
                if should_finalize {
                    self.current_slot = slot_number;
                    self.decide_ownership_if_ready(output)?;
                    self.emit_slider_steps_if_ready(output)?;
                }
            }
            self.current_slot = previous_slot;
            Ok(())
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
                    let classified = {
                        let slot_state = self.slot(slot)?;
                        classify_gesture(capabilities, slot, zone, slot_state, options, timestamp)
                            .map(|gesture| ClassifiedContact {
                                gesture,
                                sequence_match: slot_state.tap_sequence_match,
                                sequence_eligible: slot_state.tap_sequence_eligible,
                                x: slot_state.current_x.or(slot_state.start_x),
                                y: slot_state.current_y.or(slot_state.start_y),
                            })
                    };
                    if self.slot(slot)?.active {
                        self.slot_mut(slot)?.finish_contact();
                    }

                    if let Some(classified) = classified {
                        let allowed_for_slider = !releases_slider_zone
                            || matches!(
                                classified.gesture.direction,
                                GestureDirection::Tap | GestureDirection::DoubleTap
                            );
                        if allowed_for_slider {
                            self.handle_classified_gesture(classified, timestamp, output);
                        } else {
                            self.flush_pending_tap(output);
                        }
                    } else if self.pending_tap.is_some() {
                        self.flush_pending_tap(output);
                    }
                }
                Ownership::Passthrough => {
                    self.push_passthrough_event_for_current_slot(event, output);
                    if self.slot(slot)?.active {
                        self.slot_mut(slot)?.finish_contact();
                    }
                }
                Ownership::Unknown => {
                    let slot_state = self.slot_mut(slot)?;
                    slot_state.held_events.push(event);
                    if slot_state.active {
                        slot_state.finish_contact();
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
            let zone = if self.buttonpad && !self.pressed_physical_buttons.is_empty() {
                None
            } else {
                self.zone_for_current_slot()?
            };
            let Some(zone) = zone else {
                let (held_events, became_passthrough) = {
                    let slot_state = self.slot_mut(slot)?;
                    if slot_state.active
                        && matches!(slot_state.ownership, Ownership::Unknown)
                        && slot_state.start_x.is_some()
                        && slot_state.start_y.is_some()
                    {
                        slot_state.ownership = Ownership::Passthrough;
                        (std::mem::take(&mut slot_state.held_events), true)
                    } else {
                        (Vec::new(), false)
                    }
                };
                if became_passthrough {
                    self.flush_pending_tap(output);
                }
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

            let sequence_match = self.current_contact_matches_pending_tap(slot, zone)?;
            if self.pending_tap.is_some() && !sequence_match {
                self.flush_pending_tap(output);
            }

            let slot_state = self.slot_mut(slot)?;
            if slot_state.active
                && matches!(slot_state.ownership, Ownership::Unknown)
                && slot_state.start_x.is_some()
                && slot_state.start_y.is_some()
            {
                slot_state.ownership = Ownership::Claimed(zone);
                slot_state.slider_anchor = slider_anchor;
                slot_state.tap_sequence_match = sequence_match;
                slot_state.held_events.clear();
            }
            Ok(())
        }

        fn promote_claimed_contacts(&mut self) -> Vec<Event> {
            let mut events = Vec::new();
            for index in 0..self.slots.len() {
                let slot_number = self.caps.slot_min + index as i32;
                let snapshot = {
                    let slot = &mut self.slots[index];
                    if !slot.active || !matches!(slot.ownership, Ownership::Claimed(_)) {
                        None
                    } else {
                        match (slot.tracking_id, slot.current_x, slot.current_y) {
                            (Some(tracking_id), Some(x), Some(y)) => {
                                slot.ownership = Ownership::Passthrough;
                                slot.slider_anchor = None;
                                Some((tracking_id, x, y))
                            }
                            _ => None,
                        }
                    }
                };

                if let Some((tracking_id, x, y)) = snapshot {
                    events.extend([
                        Event::slot(slot_number),
                        Event::tracking_id(tracking_id),
                        Event::x(x),
                        Event::y(y),
                    ]);
                }
            }
            events
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

            if (position - anchor).abs() >= spec.step {
                self.flush_pending_tap(output);
            }

            let slot_state = self.slot_mut(slot)?;

            while position - anchor >= spec.step {
                push_slider_step(
                    slot_state,
                    output,
                    SliderStep {
                        zone,
                        direction: positive_slider_direction(spec.axis),
                        slot,
                        tracking_id,
                    },
                );
                anchor += spec.step;
            }

            while anchor - position >= spec.step {
                push_slider_step(
                    slot_state,
                    output,
                    SliderStep {
                        zone,
                        direction: negative_slider_direction(spec.axis),
                        slot,
                        tracking_id,
                    },
                );
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
                *slot = SlotState::default();
            }
            self.pressed_physical_buttons.clear();
            self.current_slot = self.caps.slot_min;
            self.pending_tap = None;
        }

        fn handle_classified_gesture(
            &mut self,
            classified: ClassifiedContact,
            released_at: Option<Duration>,
            output: &mut FrameOutput,
        ) {
            let ClassifiedContact {
                gesture,
                sequence_match,
                sequence_eligible,
                x,
                y,
            } = classified;
            if gesture.direction != GestureDirection::Tap {
                self.flush_pending_tap(output);
                output.gestures.push(gesture);
                return;
            }

            if sequence_match && self.pending_tap.take().is_some() {
                output.gestures.push(Gesture {
                    direction: GestureDirection::DoubleTap,
                    ..gesture
                });
                return;
            }

            if !sequence_eligible {
                self.flush_pending_tap(output);
                if self.options.single_tap_zones.contains(gesture.zone) {
                    output.gestures.push(gesture);
                }
                return;
            }

            let can_start_sequence = self.options.double_tap_zones.contains(gesture.zone)
                && released_at.is_some()
                && x.is_some()
                && y.is_some();
            if !can_start_sequence {
                output.gestures.push(gesture);
                return;
            }

            self.flush_pending_tap(output);
            self.pending_tap = Some(PendingTap {
                gesture,
                released_at: released_at.unwrap(),
                x: x.unwrap(),
                y: y.unwrap(),
                emit_single: self.options.single_tap_zones.contains(gesture.zone),
            });
        }

        fn current_contact_matches_pending_tap(
            &self,
            slot: i32,
            zone: Zone,
        ) -> Result<bool, SlotError> {
            let Some(pending) = self.pending_tap else {
                return Ok(false);
            };
            let state = self.slot(slot)?;
            if !state.tap_sequence_eligible || pending.gesture.zone != zone {
                return Ok(false);
            }
            let (Some(started_at), Some(x), Some(y)) =
                (state.started_at, state.start_x, state.start_y)
            else {
                return Ok(false);
            };
            let Some(gap) = started_at.checked_sub(pending.released_at) else {
                return Ok(false);
            };
            if gap >= self.options.double_tap_timeout {
                return Ok(false);
            }

            let dx = self.caps.x.normalize_delta(pending.x, x);
            let dy = self.caps.y.normalize_delta(pending.y, y);
            let max_distance = self.options.double_tap_max_distance;
            Ok(dx * dx + dy * dy <= max_distance * max_distance)
        }

        fn matching_contact_deadline(&self) -> Option<Duration> {
            self.slots
                .iter()
                .find(|slot| slot.active && slot.tap_sequence_match)
                .and_then(|slot| slot.started_at)
                .map(|started_at| started_at.saturating_add(self.options.tap_max_duration))
        }

        fn expire_pending_tap(&mut self, timestamp: Duration, output: &mut FrameOutput) {
            let Some(pending) = self.pending_tap else {
                return;
            };

            let deadline = if let Some(second_tap_deadline) = self.matching_contact_deadline() {
                second_tap_deadline
            } else {
                pending
                    .released_at
                    .saturating_add(self.options.double_tap_timeout)
            };
            if timestamp >= deadline {
                self.flush_pending_tap(output);
            }
        }

        fn flush_pending_tap(&mut self, output: &mut FrameOutput) {
            for slot in &mut self.slots {
                slot.tap_sequence_match = false;
            }
            if let Some(pending) = self.pending_tap.take() {
                if pending.emit_single {
                    output.gestures.push(pending.gesture);
                }
            }
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

    fn push_slider_step(slot_state: &mut SlotState, output: &mut FrameOutput, step: SliderStep) {
        slot_state.consume_by_slider();
        output.slider_steps.push(step);
    }

    fn validate_slider_specs(sliders: &[SliderSpec]) {
        for slider in sliders {
            assert!(
                slider.step.is_finite() && slider.step > 0.0 && slider.step <= 1.0,
                "invalid slider step for {:?}: {}",
                slider.zone,
                slider.step
            );
        }
    }

    fn validate_engine_options(options: EngineOptions) {
        assert!(
            options.tap_min_duration < options.tap_max_duration,
            "tap minimum duration must be below maximum duration"
        );
        assert!(
            !options.double_tap_timeout.is_zero(),
            "double-tap timeout must be positive"
        );
        assert!(
            options.double_tap_max_distance.is_finite()
                && options.double_tap_max_distance > 0.0
                && options.double_tap_max_distance <= 1.0,
            "double-tap distance must be > 0 and <= 1"
        );
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

        let final_direction = direction_for_displacement(dx, dy, options.swipe_min_distance);
        let direction = match state.contact_phase {
            ContactPhase::TapCandidate => match final_direction {
                Some(direction) => direction,
                None => {
                    if !tap_duration_is_valid(
                        state.started_at,
                        released_at,
                        options.tap_min_duration,
                        options.tap_max_duration,
                    ) {
                        return None;
                    }
                    GestureDirection::Tap
                }
            },
            ContactPhase::Motion => final_direction?,
            ContactPhase::ConsumedBySlider => return None,
        };

        Some(Gesture {
            zone,
            direction,
            slot,
            tracking_id: state.tracking_id?,
        })
    }

    fn direction_for_displacement(
        dx: f32,
        dy: f32,
        swipe_min_distance: f32,
    ) -> Option<GestureDirection> {
        if dx.abs() < swipe_min_distance && dy.abs() < swipe_min_distance {
            return None;
        }

        Some(if dx.abs() >= dy.abs() {
            if dx >= 0.0 {
                GestureDirection::Right
            } else {
                GestureDirection::Left
            }
        } else if dy >= 0.0 {
            GestureDirection::Down
        } else {
            GestureDirection::Up
        })
    }

    fn tap_duration_is_valid(
        started_at: Option<Duration>,
        released_at: Option<Duration>,
        min_duration: Duration,
        max_duration: Duration,
    ) -> bool {
        match (started_at, released_at) {
            (Some(started_at), Some(released_at)) => released_at
                .checked_sub(started_at)
                .is_some_and(|duration| duration >= min_duration && duration < max_duration),
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
        let mut outputs = frames
            .iter()
            .map(|frame| engine.process_frame_at(&frame.events, frame.timestamp))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(deadline) = engine.next_deadline() {
            let deadline_output = engine.advance_time(deadline);
            if !deadline_output.gestures.is_empty() {
                outputs.push(deadline_output);
            }
        }
        Ok(outputs)
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

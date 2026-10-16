use super::*;
use crate::core::AxisRange;
use crate::uinput::UinputEventWriter;

pub(super) fn test_capabilities() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 9,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

#[derive(Debug, Default)]
pub(super) struct ReloadingGestureHandler {
    pub(super) pending: Option<ProxyRecognitionConfig>,
    pub(super) reload_polls: usize,
}

impl GestureHandler for ReloadingGestureHandler {
    fn handle_gesture(&mut self, _gesture: Gesture) {}

    fn take_recognition_config_reload(&mut self) -> Option<ProxyRecognitionConfig> {
        self.reload_polls += 1;
        self.pending.take()
    }
}

#[derive(Debug, Default)]
pub(super) struct RecordingUinputWriter {
    pub(super) batches: Vec<Vec<evdev::InputEvent>>,
}

#[derive(Debug, Default)]
pub(super) struct RecordingGestureHandler {
    pub(super) gestures: Vec<Gesture>,
    pub(super) slider_steps: Vec<SliderStep>,
}

impl GestureHandler for RecordingGestureHandler {
    fn handle_gesture(&mut self, gesture: Gesture) {
        self.gestures.push(gesture);
    }

    fn handle_slider_step(&mut self, step: SliderStep) {
        self.slider_steps.push(step);
    }
}

impl UinputEventWriter for RecordingUinputWriter {
    type Error = std::convert::Infallible;

    fn emit_events(&mut self, events: &[evdev::InputEvent]) -> Result<(), Self::Error> {
        self.batches.push(events.to_vec());
        Ok(())
    }
}

pub(super) fn input_event_triples(events: &[evdev::InputEvent]) -> Vec<(u16, u16, i32)> {
    events
        .iter()
        .map(|event| (event.event_type().0, event.code(), event.value()))
        .collect()
}

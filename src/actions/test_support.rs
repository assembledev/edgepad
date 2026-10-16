use super::{ActionCommandRunner, ActionCommandStatus, ActionShutdownToken};
use crate::config::{
    CommandActionConfig, GestureActionConfig, GestureBindingConfig, SliderBindingConfig,
};
use crate::core::{Gesture, GestureDirection, SliderAxis, SliderDirection, SliderStep, Zone};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct RecordingRunner {
    pub sender: mpsc::Sender<Vec<String>>,
    pub status: ActionCommandStatus,
}

impl ActionCommandRunner for RecordingRunner {
    fn run(
        &mut self,
        argv: &[String],
        _shutdown: &ActionShutdownToken,
    ) -> Result<ActionCommandStatus, String> {
        self.sender
            .send(argv.to_vec())
            .expect("recording runner receiver should be alive");
        Ok(self.status)
    }
}

pub(super) fn binding(
    zone: Zone,
    direction: GestureDirection,
    action: GestureActionConfig,
) -> GestureBindingConfig {
    GestureBindingConfig {
        zone,
        direction,
        action,
    }
}

pub(super) fn gesture(zone: Zone, direction: GestureDirection) -> Gesture {
    Gesture {
        zone,
        direction,
        slot: 0,
        tracking_id: 42,
    }
}

pub(super) fn slider_binding(
    zone: Zone,
    negative_direction: SliderDirection,
    negative: Vec<&str>,
    positive_direction: SliderDirection,
    positive: Vec<&str>,
) -> SliderBindingConfig {
    let axis = match (negative_direction, positive_direction) {
        (SliderDirection::Up, SliderDirection::Down) => SliderAxis::Vertical,
        (SliderDirection::Left, SliderDirection::Right) => SliderAxis::Horizontal,
        _ => panic!("invalid slider directions"),
    };
    SliderBindingConfig {
        zone,
        axis,
        step: 0.04,
        negative: command_action(negative),
        positive: command_action(positive),
    }
}

pub(super) fn command_action(argv: Vec<&str>) -> CommandActionConfig {
    CommandActionConfig::new(argv).expect("test command should be valid")
}

pub(super) fn slider_step(zone: Zone, direction: SliderDirection) -> SliderStep {
    SliderStep {
        zone,
        direction,
        slot: 0,
        tracking_id: 42,
    }
}

pub(super) struct GatedRunner {
    pub started: mpsc::Sender<Vec<String>>,
    pub release: mpsc::Receiver<()>,
}

impl ActionCommandRunner for GatedRunner {
    fn run(
        &mut self,
        argv: &[String],
        shutdown: &ActionShutdownToken,
    ) -> Result<ActionCommandStatus, String> {
        self.started.send(argv.to_vec()).unwrap();
        loop {
            if shutdown.is_requested() {
                return Err("test command canceled".to_string());
            }
            match self.release.recv_timeout(Duration::from_millis(5)) {
                Ok(()) => return Ok(ActionCommandStatus::success()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("test gate closed".to_string())
                }
            }
        }
    }
}

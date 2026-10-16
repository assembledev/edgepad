//! Run configured commands off the input thread.
//!
//! A single worker preserves command order. Its bounded queue drops new actions
//! when full, so a slow command cannot block touchpad forwarding or grow an
//! unlimited backlog of slider steps.

use crate::config::{GestureActionConfig, GestureBindingConfig, SliderBindingConfig};
use crate::core::{Gesture, SliderStep};
use crate::proxy::GestureHandler;

mod command;
mod process;
mod stats;
#[cfg(test)]
mod test_support;
mod worker;

pub use command::{ActionCommandRunner, ActionCommandStatus, ActionShutdownToken};
pub use process::ProcessCommandRunner;
pub use stats::ActionDispatcherStats;

use command::ActionSource;
use worker::ActionWorker;

/// Dropping the dispatcher requests cancellation and waits up to one second
/// for its worker, just as `shutdown` does.
#[derive(Debug)]
pub struct ActionDispatcher {
    bindings: Vec<GestureBindingConfig>,
    sliders: Vec<SliderBindingConfig>,
    worker: ActionWorker,
}

impl ActionDispatcher {
    pub fn new(bindings: Vec<GestureBindingConfig>, queue_capacity: usize) -> Result<Self, String> {
        Self::with_runner(bindings, queue_capacity, ProcessCommandRunner)
    }

    pub fn new_with_sliders(
        bindings: Vec<GestureBindingConfig>,
        sliders: Vec<SliderBindingConfig>,
        queue_capacity: usize,
    ) -> Result<Self, String> {
        Self::with_runner_and_sliders(bindings, sliders, queue_capacity, ProcessCommandRunner)
    }

    pub fn with_runner<R>(
        bindings: Vec<GestureBindingConfig>,
        queue_capacity: usize,
        runner: R,
    ) -> Result<Self, String>
    where
        R: ActionCommandRunner,
    {
        Self::with_runner_and_sliders(bindings, Vec::new(), queue_capacity, runner)
    }

    pub fn with_runner_and_sliders<R>(
        bindings: Vec<GestureBindingConfig>,
        sliders: Vec<SliderBindingConfig>,
        queue_capacity: usize,
        runner: R,
    ) -> Result<Self, String>
    where
        R: ActionCommandRunner,
    {
        Ok(Self {
            bindings,
            sliders,
            worker: ActionWorker::start(queue_capacity, runner)?,
        })
    }

    pub fn dispatch_gesture(&mut self, gesture: Gesture) {
        let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| binding.matches(&gesture))
        else {
            self.worker.stats.increment_unmatched_gestures();
            eprintln!(
                "edgepad action unmatched: {}",
                ActionSource::Gesture(gesture)
            );
            return;
        };

        self.worker.stats.increment_matched_gestures();
        match &binding.action {
            GestureActionConfig::Log => {
                self.worker.stats.increment_log_actions();
                eprintln!("edgepad action log: {}", ActionSource::Gesture(gesture));
            }
            GestureActionConfig::Command { argv } => self
                .worker
                .enqueue(ActionSource::Gesture(gesture), argv.clone()),
        }
    }

    pub fn dispatch_slider_step(&mut self, step: SliderStep) {
        let Some(slider) = self
            .sliders
            .iter()
            .find(|slider| slider.zone == step.zone && slider.direction_is_valid(step.direction))
        else {
            self.worker.stats.increment_unmatched_slider_steps();
            eprintln!(
                "edgepad slider unmatched: {}",
                ActionSource::SliderStep(step)
            );
            return;
        };

        self.worker.stats.increment_matched_slider_steps();
        self.worker.enqueue(
            ActionSource::SliderStep(step),
            slider.action_for(step.direction).argv.clone(),
        );
    }

    pub fn stats(&self) -> ActionDispatcherStats {
        self.worker.stats.snapshot()
    }

    /// Replace bindings for future dispatches. Already queued commands keep
    /// their original argv, so a reload does not reinterpret an earlier gesture.
    pub fn reconfigure(
        &mut self,
        bindings: Vec<GestureBindingConfig>,
        sliders: Vec<SliderBindingConfig>,
    ) {
        self.bindings = bindings;
        self.sliders = sliders;
    }

    pub fn shutdown(self) -> ActionDispatcherStats {
        self.worker.shutdown()
    }
}

impl GestureHandler for ActionDispatcher {
    fn handle_gesture(&mut self, gesture: Gesture) {
        self.dispatch_gesture(gesture);
    }

    fn handle_slider_step(&mut self, step: SliderStep) {
        self.dispatch_slider_step(step);
    }
}

#[cfg(test)]
mod tests;

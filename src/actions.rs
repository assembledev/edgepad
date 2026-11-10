//! Run configured commands off the input thread.
//!
//! A single worker preserves command order. Its bounded queue drops new actions
//! when full, so a slow command cannot block touchpad forwarding or grow an
//! unlimited backlog of slider steps.

use crate::config::{GestureActionConfig, GestureBindingConfig, SliderBindingConfig};
use crate::core::{Gesture, SliderEnd, SliderEndKind, SliderStep, Zone};
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
use worker::{ActionWorker, QueuePermit};

#[derive(Debug)]
struct SliderSession {
    zone: Zone,
    slot: i32,
    tracking_id: i32,
    on_release: Option<Vec<String>>,
    on_cancel: Option<Vec<String>>,
    terminal: Option<QueuePermit>,
}

/// Dropping the dispatcher requests cancellation and waits up to one second
/// for its worker, just as `shutdown` does.
#[derive(Debug)]
pub struct ActionDispatcher {
    bindings: Vec<GestureBindingConfig>,
    sliders: Vec<SliderBindingConfig>,
    worker: ActionWorker,
    sessions: Vec<SliderSession>,
    lifecycle_used: bool,
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
            sessions: Vec::new(),
            lifecycle_used: false,
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
            GestureActionConfig::Command { argv } => {
                self.worker
                    .enqueue(ActionSource::Gesture(gesture), argv.clone());
            }
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
        if slider.on_release.is_some() || slider.on_cancel.is_some() {
            let existing = self.sessions.iter().find(|session| {
                session.slot == step.slot && session.tracking_id == step.tracking_id
            });
            if let Some(session) = existing {
                if session.terminal.is_none() {
                    return;
                }
            } else {
                let occupied = self
                    .sessions
                    .iter()
                    .any(|session| session.zone == step.zone && session.terminal.is_some());
                let mut terminal = if occupied {
                    None
                } else {
                    self.worker.reserve()
                };
                if !occupied && terminal.is_none() {
                    self.worker.stats.increment_dropped_commands();
                }
                if terminal.is_some()
                    && !self.worker.enqueue(
                        ActionSource::SliderStep(step),
                        slider.action_for(step.direction).argv.clone(),
                    )
                {
                    terminal = None;
                }
                self.lifecycle_used = true;
                self.sessions.push(SliderSession {
                    zone: step.zone,
                    slot: step.slot,
                    tracking_id: step.tracking_id,
                    on_release: slider.on_release.as_ref().map(|action| action.argv.clone()),
                    on_cancel: slider.on_cancel.as_ref().map(|action| action.argv.clone()),
                    terminal,
                });
                return;
            }
        }
        self.worker.enqueue(
            ActionSource::SliderStep(step),
            slider.action_for(step.direction).argv.clone(),
        );
    }

    pub fn dispatch_slider_end(&mut self, end: SliderEnd) {
        let Some(index) = self.sessions.iter().position(|session| {
            session.zone == end.zone
                && session.slot == end.slot
                && session.tracking_id == end.tracking_id
        }) else {
            return;
        };
        let session = self.sessions.remove(index);
        let action = match end.kind {
            SliderEndKind::Release => session.on_release,
            SliderEndKind::Cancel => session.on_cancel,
        };
        if let (Some(argv), Some(permit)) = (action, session.terminal) {
            self.worker
                .enqueue_reserved(ActionSource::SliderEnd(end), argv, permit);
        }
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

    pub fn shutdown(mut self) -> ActionDispatcherStats {
        while let Some(session) = self.sessions.last() {
            self.dispatch_slider_end(SliderEnd {
                zone: session.zone,
                slot: session.slot,
                tracking_id: session.tracking_id,
                kind: SliderEndKind::Cancel,
            });
        }
        if self.lifecycle_used {
            self.worker.drain();
        }
        self.worker.shutdown()
    }
}

impl GestureHandler for ActionDispatcher {
    fn handle_slider_end(&mut self, end: SliderEnd) {
        self.dispatch_slider_end(end);
    }

    fn handle_gesture(&mut self, gesture: Gesture) {
        self.dispatch_gesture(gesture);
    }

    fn handle_slider_step(&mut self, step: SliderStep) {
        self.dispatch_slider_step(step);
    }
}

#[cfg(test)]
mod tests;

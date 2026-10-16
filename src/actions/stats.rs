use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionDispatcherStats {
    pub matched_gestures: usize,
    pub unmatched_gestures: usize,
    pub matched_slider_steps: usize,
    pub unmatched_slider_steps: usize,
    pub log_actions: usize,
    pub queued_commands: usize,
    pub dropped_commands: usize,
    pub started_commands: usize,
    pub succeeded_commands: usize,
    pub failed_commands: usize,
    pub worker_panics: usize,
    pub worker_shutdown_timeouts: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SharedActionStats {
    stats: Arc<Mutex<ActionDispatcherStats>>,
}

impl SharedActionStats {
    pub(super) fn snapshot(&self) -> ActionDispatcherStats {
        self.stats
            .lock()
            .expect("action stats mutex should not be poisoned")
            .clone()
    }

    fn update(&self, update: impl FnOnce(&mut ActionDispatcherStats)) {
        let mut stats = self
            .stats
            .lock()
            .expect("action stats mutex should not be poisoned");
        update(&mut stats);
    }

    pub(super) fn increment_matched_gestures(&self) {
        self.update(|stats| stats.matched_gestures += 1);
    }

    pub(super) fn increment_unmatched_gestures(&self) {
        self.update(|stats| stats.unmatched_gestures += 1);
    }

    pub(super) fn increment_matched_slider_steps(&self) {
        self.update(|stats| stats.matched_slider_steps += 1);
    }

    pub(super) fn increment_unmatched_slider_steps(&self) {
        self.update(|stats| stats.unmatched_slider_steps += 1);
    }

    pub(super) fn increment_log_actions(&self) {
        self.update(|stats| stats.log_actions += 1);
    }

    pub(super) fn increment_queued_commands(&self) {
        self.update(|stats| stats.queued_commands += 1);
    }

    pub(super) fn increment_dropped_commands(&self) {
        self.update(|stats| stats.dropped_commands += 1);
    }

    pub(super) fn increment_started_commands(&self) {
        self.update(|stats| stats.started_commands += 1);
    }

    pub(super) fn increment_succeeded_commands(&self) {
        self.update(|stats| stats.succeeded_commands += 1);
    }

    pub(super) fn increment_failed_commands(&self) {
        self.update(|stats| stats.failed_commands += 1);
    }

    pub(super) fn increment_worker_panics(&self) {
        self.update(|stats| stats.worker_panics += 1);
    }

    pub(super) fn increment_worker_shutdown_timeouts(&self) {
        self.update(|stats| stats.worker_shutdown_timeouts += 1);
    }
}

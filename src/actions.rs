use std::process::Command;
use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};

use crate::config::{GestureActionConfig, GestureBindingConfig};
use crate::core::{Gesture, GestureDirection, Zone};
use crate::proxy::GestureHandler;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionDispatcherStats {
    pub matched_gestures: usize,
    pub unmatched_gestures: usize,
    pub log_actions: usize,
    pub queued_commands: usize,
    pub dropped_commands: usize,
    pub started_commands: usize,
    pub succeeded_commands: usize,
    pub failed_commands: usize,
    pub worker_panics: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionCommandStatus {
    pub success: bool,
}

impl ActionCommandStatus {
    pub fn success() -> Self {
        Self { success: true }
    }

    pub fn failure() -> Self {
        Self { success: false }
    }
}

pub trait ActionCommandRunner: Send + 'static {
    fn run(&mut self, argv: &[String]) -> Result<ActionCommandStatus, String>;
}

#[derive(Debug, Default)]
pub struct ProcessCommandRunner;

impl ActionCommandRunner for ProcessCommandRunner {
    fn run(&mut self, argv: &[String]) -> Result<ActionCommandStatus, String> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "command action argv must not be empty".to_string())?;
        let mut child = Command::new(program)
            .args(args)
            .spawn()
            .map_err(|err| format!("failed to spawn action command {program:?}: {err}"))?;

        // Always wait for the child in the worker. Dropping Child after spawn would
        // leave short-lived commands as zombies until the daemon exits.
        let status = child
            .wait()
            .map_err(|err| format!("failed to wait for action command {program:?}: {err}"))?;
        Ok(ActionCommandStatus {
            success: status.success(),
        })
    }
}

#[derive(Debug)]
pub struct ActionDispatcher {
    bindings: Vec<GestureBindingConfig>,
    sender: Option<SyncSender<ActionCommand>>,
    stats: SharedActionStats,
    worker: Option<JoinHandle<()>>,
}

impl ActionDispatcher {
    pub fn new(bindings: Vec<GestureBindingConfig>, queue_capacity: usize) -> Result<Self, String> {
        Self::with_runner(bindings, queue_capacity, ProcessCommandRunner)
    }

    pub fn with_runner<R>(
        bindings: Vec<GestureBindingConfig>,
        queue_capacity: usize,
        runner: R,
    ) -> Result<Self, String>
    where
        R: ActionCommandRunner,
    {
        let (sender, receiver) = sync_channel(queue_capacity);
        let stats = SharedActionStats::default();
        let worker_stats = stats.clone();
        let worker = thread::Builder::new()
            .name("edgepad-action-worker".to_string())
            .spawn(move || run_action_worker(receiver, runner, worker_stats))
            .map_err(|err| format!("failed to start edgepad action worker: {err}"))?;

        Ok(Self {
            bindings,
            sender: Some(sender),
            stats,
            worker: Some(worker),
        })
    }

    pub fn dispatch_gesture(&mut self, gesture: Gesture) {
        let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| binding.matches(&gesture))
        else {
            self.stats.increment_unmatched_gestures();
            return;
        };

        self.stats.increment_matched_gestures();
        match &binding.action {
            GestureActionConfig::Log => {
                self.stats.increment_log_actions();
                eprintln!(
                    "edgepad action log: zone={} direction={} slot={} tracking_id={}",
                    zone_name(gesture.zone),
                    direction_name(gesture.direction),
                    gesture.slot,
                    gesture.tracking_id
                );
            }
            GestureActionConfig::Command { argv } => self.enqueue_command(argv.clone()),
        }
    }

    pub fn stats(&self) -> ActionDispatcherStats {
        self.stats.snapshot()
    }

    pub fn shutdown(mut self) -> ActionDispatcherStats {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                self.stats.increment_worker_panics();
            }
        }
        self.stats.snapshot()
    }

    fn enqueue_command(&self, argv: Vec<String>) {
        let Some(sender) = &self.sender else {
            self.stats.increment_dropped_commands();
            return;
        };

        match sender.try_send(ActionCommand { argv }) {
            Ok(()) => self.stats.increment_queued_commands(),
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.stats.increment_dropped_commands();
            }
        }
    }
}

impl GestureHandler for ActionDispatcher {
    fn handle_gesture(&mut self, gesture: Gesture) {
        self.dispatch_gesture(gesture);
    }
}

#[derive(Debug, Clone)]
struct ActionCommand {
    argv: Vec<String>,
}

fn run_action_worker<R>(receiver: Receiver<ActionCommand>, mut runner: R, stats: SharedActionStats)
where
    R: ActionCommandRunner,
{
    while let Ok(command) = receiver.recv() {
        stats.increment_started_commands();
        match runner.run(&command.argv) {
            Ok(status) if status.success => stats.increment_succeeded_commands(),
            Ok(_) => {
                stats.increment_failed_commands();
                eprintln!(
                    "edgepad action command exited unsuccessfully: {:?}",
                    command.argv
                );
            }
            Err(err) => {
                stats.increment_failed_commands();
                eprintln!("edgepad action command failed: {err}");
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SharedActionStats {
    stats: Arc<Mutex<ActionDispatcherStats>>,
}

impl SharedActionStats {
    fn snapshot(&self) -> ActionDispatcherStats {
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

    fn increment_matched_gestures(&self) {
        self.update(|stats| stats.matched_gestures += 1);
    }

    fn increment_unmatched_gestures(&self) {
        self.update(|stats| stats.unmatched_gestures += 1);
    }

    fn increment_log_actions(&self) {
        self.update(|stats| stats.log_actions += 1);
    }

    fn increment_queued_commands(&self) {
        self.update(|stats| stats.queued_commands += 1);
    }

    fn increment_dropped_commands(&self) {
        self.update(|stats| stats.dropped_commands += 1);
    }

    fn increment_started_commands(&self) {
        self.update(|stats| stats.started_commands += 1);
    }

    fn increment_succeeded_commands(&self) {
        self.update(|stats| stats.succeeded_commands += 1);
    }

    fn increment_failed_commands(&self) {
        self.update(|stats| stats.failed_commands += 1);
    }

    fn increment_worker_panics(&self) {
        self.update(|stats| stats.worker_panics += 1);
    }
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Left => "left",
        Zone::Right => "right",
        Zone::Top => "top",
        Zone::Bottom => "bottom",
    }
}

fn direction_name(direction: GestureDirection) -> &'static str {
    match direction {
        GestureDirection::Up => "up",
        GestureDirection::Down => "down",
        GestureDirection::Left => "left",
        GestureDirection::Right => "right",
        GestureDirection::Tap => "tap",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[derive(Debug)]
    struct RecordingRunner {
        sender: mpsc::Sender<Vec<String>>,
        status: ActionCommandStatus,
    }

    impl ActionCommandRunner for RecordingRunner {
        fn run(&mut self, argv: &[String]) -> Result<ActionCommandStatus, String> {
            self.sender
                .send(argv.to_vec())
                .expect("recording runner receiver should be alive");
            Ok(self.status)
        }
    }

    #[test]
    fn matching_command_action_is_queued_and_waited_by_worker_runner() {
        let (sender, receiver) = mpsc::channel();
        let mut dispatcher = ActionDispatcher::with_runner(
            vec![binding(
                Zone::Left,
                GestureDirection::Right,
                GestureActionConfig::Command {
                    argv: vec![
                        "notify-send".to_string(),
                        "edgepad".to_string(),
                        "left right".to_string(),
                    ],
                },
            )],
            4,
            RecordingRunner {
                sender,
                status: ActionCommandStatus::success(),
            },
        )
        .expect("dispatcher should start");

        dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Right));
        let argv = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker should run queued action");
        let stats = dispatcher.shutdown();

        assert_eq!(
            argv,
            vec![
                "notify-send".to_string(),
                "edgepad".to_string(),
                "left right".to_string()
            ]
        );
        assert_eq!(stats.matched_gestures, 1);
        assert_eq!(stats.queued_commands, 1);
        assert_eq!(stats.started_commands, 1);
        assert_eq!(stats.succeeded_commands, 1);
        assert_eq!(stats.failed_commands, 0);
    }

    #[test]
    fn unmatched_gesture_does_not_queue_action() {
        let (sender, receiver) = mpsc::channel();
        let mut dispatcher = ActionDispatcher::with_runner(
            vec![binding(
                Zone::Left,
                GestureDirection::Right,
                GestureActionConfig::Command {
                    argv: vec!["notify-send".to_string(), "edgepad".to_string()],
                },
            )],
            4,
            RecordingRunner {
                sender,
                status: ActionCommandStatus::success(),
            },
        )
        .expect("dispatcher should start");

        dispatcher.dispatch_gesture(gesture(Zone::Right, GestureDirection::Down));
        let stats = dispatcher.shutdown();

        assert!(receiver.try_recv().is_err());
        assert_eq!(stats.unmatched_gestures, 1);
        assert_eq!(stats.queued_commands, 0);
        assert_eq!(stats.started_commands, 0);
    }

    #[test]
    fn log_action_is_counted_without_queueing_command() {
        let (sender, receiver) = mpsc::channel();
        let mut dispatcher = ActionDispatcher::with_runner(
            vec![binding(
                Zone::Top,
                GestureDirection::Down,
                GestureActionConfig::Log,
            )],
            4,
            RecordingRunner {
                sender,
                status: ActionCommandStatus::success(),
            },
        )
        .expect("dispatcher should start");

        dispatcher.dispatch_gesture(gesture(Zone::Top, GestureDirection::Down));
        let stats = dispatcher.shutdown();

        assert!(receiver.try_recv().is_err());
        assert_eq!(stats.matched_gestures, 1);
        assert_eq!(stats.log_actions, 1);
        assert_eq!(stats.queued_commands, 0);
    }

    #[test]
    fn failed_command_status_is_recorded() {
        let (sender, receiver) = mpsc::channel();
        let mut dispatcher = ActionDispatcher::with_runner(
            vec![binding(
                Zone::Bottom,
                GestureDirection::Up,
                GestureActionConfig::Command {
                    argv: vec!["false".to_string()],
                },
            )],
            4,
            RecordingRunner {
                sender,
                status: ActionCommandStatus::failure(),
            },
        )
        .expect("dispatcher should start");

        dispatcher.dispatch_gesture(gesture(Zone::Bottom, GestureDirection::Up));
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker should run queued action");
        let stats = dispatcher.shutdown();

        assert_eq!(stats.started_commands, 1);
        assert_eq!(stats.succeeded_commands, 0);
        assert_eq!(stats.failed_commands, 1);
    }

    fn binding(
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

    fn gesture(zone: Zone, direction: GestureDirection) -> Gesture {
        Gesture {
            zone,
            direction,
            slot: 0,
            tracking_id: 42,
        }
    }
}

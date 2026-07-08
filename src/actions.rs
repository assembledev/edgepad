use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::config::CommandActionConfig;
use crate::config::{GestureActionConfig, GestureBindingConfig, SliderBindingConfig};
#[cfg(test)]
use crate::core::SliderAxis;
use crate::core::{Gesture, GestureDirection, SliderDirection, SliderStep, Zone};
use crate::proxy::GestureHandler;

const ACTION_COMMAND_INITIAL_POLL_INTERVAL: Duration = Duration::from_millis(1);
const ACTION_COMMAND_MAX_POLL_INTERVAL: Duration = Duration::from_millis(10);
const ACTION_WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const ACTION_WORKER_SHUTDOWN_JOIN_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    fn run(
        &mut self,
        argv: &[String],
        shutdown: &ActionShutdownToken,
    ) -> Result<ActionCommandStatus, String>;
}

#[derive(Debug, Clone, Default)]
pub struct ActionShutdownToken {
    requested: Arc<AtomicBool>,
}

impl ActionShutdownToken {
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }
}

#[derive(Debug, Default)]
pub struct ProcessCommandRunner;

impl ActionCommandRunner for ProcessCommandRunner {
    fn run(
        &mut self,
        argv: &[String],
        shutdown: &ActionShutdownToken,
    ) -> Result<ActionCommandStatus, String> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "command action argv must not be empty".to_string())?;
        let mut child = spawn_action_command(program, args)?;
        let mut poll_interval = ACTION_COMMAND_INITIAL_POLL_INTERVAL;

        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|err| format!("failed to poll action command {program:?}: {err}"))?
            {
                return Ok(ActionCommandStatus {
                    success: status.success(),
                });
            }

            if shutdown.is_requested() {
                if let Some(status) = child.try_wait().map_err(|err| {
                    format!("failed to poll action command {program:?} before shutdown kill: {err}")
                })? {
                    return Ok(ActionCommandStatus {
                        success: status.success(),
                    });
                }

                match kill_action_child(&mut child, program) {
                    Ok(true) => {
                        child.wait().map_err(|err| {
                            format!("failed to wait for killed action command {program:?}: {err}")
                        })?;
                        return Err(format!(
                            "action command {program:?} interrupted by daemon shutdown"
                        ));
                    }
                    Ok(false) => {
                        let status = child.wait().map_err(|err| {
                            format!("failed to wait for action command {program:?}: {err}")
                        })?;
                        return Ok(ActionCommandStatus {
                            success: status.success(),
                        });
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }
            }

            thread::sleep(poll_interval);
            poll_interval = next_action_command_poll_interval(poll_interval);
        }
    }
}

fn next_action_command_poll_interval(current: Duration) -> Duration {
    std::cmp::min(current.saturating_mul(2), ACTION_COMMAND_MAX_POLL_INTERVAL)
}

fn spawn_action_command(program: &str, args: &[String]) -> Result<Child, String> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(unix)]
    command.process_group(0);
    command
        .spawn()
        .map_err(|err| format!("failed to spawn action command {program:?}: {err}"))
}

fn kill_action_child(child: &mut Child, program: &str) -> Result<bool, String> {
    #[cfg(unix)]
    {
        let process_group = child.id() as libc::pid_t;
        let result = unsafe { libc::kill(-process_group, libc::SIGKILL) };
        if result == 0 {
            return Ok(true);
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }

        Err(format!(
            "failed to kill action command process group {program:?} during daemon shutdown: {err}"
        ))
    }

    #[cfg(not(unix))]
    {
        match child.kill() {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::InvalidInput => Ok(false),
            Err(err) => Err(format!(
                "failed to kill action command {program:?} during daemon shutdown: {err}"
            )),
        }
    }
}

#[derive(Debug)]
pub struct ActionDispatcher {
    bindings: Vec<GestureBindingConfig>,
    sliders: Vec<SliderBindingConfig>,
    sender: Option<SyncSender<ActionCommand>>,
    stats: SharedActionStats,
    worker: Option<JoinHandle<()>>,
    shutdown: ActionShutdownToken,
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
        let (sender, receiver) = sync_channel(queue_capacity);
        let stats = SharedActionStats::default();
        let worker_stats = stats.clone();
        let shutdown = ActionShutdownToken::default();
        let worker_shutdown = shutdown.clone();
        let worker = thread::Builder::new()
            .name("edgepad-action-worker".to_string())
            .spawn(move || run_action_worker(receiver, runner, worker_stats, worker_shutdown))
            .map_err(|err| format!("failed to start edgepad action worker: {err}"))?;

        Ok(Self {
            bindings,
            sliders,
            sender: Some(sender),
            stats,
            worker: Some(worker),
            shutdown,
        })
    }

    pub fn dispatch_gesture(&mut self, gesture: Gesture) {
        let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| binding.matches(&gesture))
        else {
            self.stats.increment_unmatched_gestures();
            eprintln!("edgepad action unmatched: {}", gesture_context(gesture));
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
            GestureActionConfig::Command { argv } => self.enqueue_command(gesture, argv.clone()),
        }
    }

    pub fn dispatch_slider_step(&mut self, step: SliderStep) {
        let Some(slider) = self
            .sliders
            .iter()
            .find(|slider| slider.zone == step.zone && slider.direction_is_valid(step.direction))
        else {
            self.stats.increment_unmatched_slider_steps();
            eprintln!("edgepad slider unmatched: {}", slider_step_context(step));
            return;
        };

        self.stats.increment_matched_slider_steps();
        self.enqueue_slider_command(step, slider.action_for(step.direction).argv.clone());
    }

    pub fn stats(&self) -> ActionDispatcherStats {
        self.stats.snapshot()
    }

    pub fn shutdown(mut self) -> ActionDispatcherStats {
        self.shutdown.request();
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            if wait_for_action_worker_shutdown(&worker, ACTION_WORKER_SHUTDOWN_TIMEOUT) {
                if worker.join().is_err() {
                    self.stats.increment_worker_panics();
                }
            } else {
                self.stats.increment_worker_shutdown_timeouts();
                eprintln!(
                    "edgepad action worker did not stop within {:.1}s; daemon shutdown will continue",
                    ACTION_WORKER_SHUTDOWN_TIMEOUT.as_secs_f32()
                );
            }
        }
        self.stats.snapshot()
    }

    fn enqueue_command(&self, gesture: Gesture, argv: Vec<String>) {
        self.enqueue_action_command(ActionSource::Gesture(gesture), argv);
    }

    fn enqueue_slider_command(&self, step: SliderStep, argv: Vec<String>) {
        self.enqueue_action_command(ActionSource::SliderStep(step), argv);
    }

    fn enqueue_action_command(&self, source: ActionSource, argv: Vec<String>) {
        let Some(sender) = &self.sender else {
            self.stats.increment_dropped_commands();
            return;
        };

        match sender.try_send(ActionCommand { source, argv }) {
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

    fn handle_slider_step(&mut self, step: SliderStep) {
        self.dispatch_slider_step(step);
    }
}

#[derive(Debug, Clone)]
struct ActionCommand {
    source: ActionSource,
    argv: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum ActionSource {
    Gesture(Gesture),
    SliderStep(SliderStep),
}

fn run_action_worker<R>(
    receiver: Receiver<ActionCommand>,
    mut runner: R,
    stats: SharedActionStats,
    shutdown: ActionShutdownToken,
) where
    R: ActionCommandRunner,
{
    while !shutdown.is_requested() {
        let Ok(command) = receiver.recv() else {
            break;
        };
        if shutdown.is_requested() {
            break;
        }

        stats.increment_started_commands();
        match runner.run(&command.argv, &shutdown) {
            Ok(status) if status.success => stats.increment_succeeded_commands(),
            Ok(_) => {
                stats.increment_failed_commands();
                eprintln!(
                    "edgepad action command exited unsuccessfully: {} argv={:?}",
                    action_command_context(&command),
                    command.argv
                );
            }
            Err(err) => {
                stats.increment_failed_commands();
                eprintln!(
                    "edgepad action command failed: {} argv={:?}: {err}",
                    action_command_context(&command),
                    command.argv
                );
            }
        }
    }
}

fn action_command_context(command: &ActionCommand) -> String {
    action_source_context(command.source)
}

fn action_source_context(source: ActionSource) -> String {
    match source {
        ActionSource::Gesture(gesture) => gesture_context(gesture),
        ActionSource::SliderStep(step) => slider_step_context(step),
    }
}

fn gesture_context(gesture: Gesture) -> String {
    format!(
        "zone={} direction={} slot={} tracking_id={}",
        zone_name(gesture.zone),
        direction_name(gesture.direction),
        gesture.slot,
        gesture.tracking_id
    )
}

fn slider_step_context(step: SliderStep) -> String {
    format!(
        "zone={} direction={} slot={} tracking_id={}",
        zone_name(step.zone),
        slider_direction_name(step.direction),
        step.slot,
        step.tracking_id
    )
}

fn wait_for_action_worker_shutdown(worker: &JoinHandle<()>, timeout: Duration) -> bool {
    let started_at = Instant::now();
    while !worker.is_finished() {
        if started_at.elapsed() >= timeout {
            return false;
        }

        let remaining = timeout.saturating_sub(started_at.elapsed());
        thread::sleep(ACTION_WORKER_SHUTDOWN_JOIN_POLL_INTERVAL.min(remaining));
    }
    true
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

    fn increment_matched_slider_steps(&self) {
        self.update(|stats| stats.matched_slider_steps += 1);
    }

    fn increment_unmatched_slider_steps(&self) {
        self.update(|stats| stats.unmatched_slider_steps += 1);
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

    fn increment_worker_shutdown_timeouts(&self) {
        self.update(|stats| stats.worker_shutdown_timeouts += 1);
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

fn slider_direction_name(direction: SliderDirection) -> &'static str {
    match direction {
        SliderDirection::Up => "up",
        SliderDirection::Down => "down",
        SliderDirection::Left => "left",
        SliderDirection::Right => "right",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    #[derive(Debug)]
    struct RecordingRunner {
        sender: mpsc::Sender<Vec<String>>,
        status: ActionCommandStatus,
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
    fn matching_slider_step_action_is_queued_and_waited_by_worker_runner() {
        let (sender, receiver) = mpsc::channel();
        let mut dispatcher = ActionDispatcher::with_runner_and_sliders(
            Vec::new(),
            vec![slider_binding(
                Zone::Left,
                SliderDirection::Up,
                vec!["pamixer", "-d", "3"],
                SliderDirection::Down,
                vec!["pamixer", "-i", "3"],
            )],
            4,
            RecordingRunner {
                sender,
                status: ActionCommandStatus::success(),
            },
        )
        .expect("dispatcher should start");

        dispatcher.dispatch_slider_step(slider_step(Zone::Left, SliderDirection::Down));
        let argv = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker should run queued action");
        let stats = dispatcher.shutdown();

        assert_eq!(
            argv,
            vec!["pamixer".to_string(), "-i".to_string(), "3".to_string()]
        );
        assert_eq!(stats.matched_slider_steps, 1);
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

    #[test]
    fn shutdown_interrupts_running_process_action() {
        let mut dispatcher = ActionDispatcher::with_runner(
            vec![binding(
                Zone::Right,
                GestureDirection::Down,
                GestureActionConfig::Command {
                    argv: vec!["sleep".to_string(), "10".to_string()],
                },
            )],
            4,
            ProcessCommandRunner,
        )
        .expect("dispatcher should start");

        dispatcher.dispatch_gesture(gesture(Zone::Right, GestureDirection::Down));
        wait_for_started_command(&dispatcher);

        let started_at = Instant::now();
        let stats = dispatcher.shutdown();

        assert!(
            started_at.elapsed() < Duration::from_secs(2),
            "shutdown should not wait for the full action command duration"
        );
        assert_eq!(stats.started_commands, 1);
        assert_eq!(stats.succeeded_commands, 0);
        assert_eq!(stats.failed_commands, 1);
        assert_eq!(stats.worker_shutdown_timeouts, 0);
    }

    #[test]
    fn action_command_poll_interval_backs_off_to_small_bound() {
        let first = ACTION_COMMAND_INITIAL_POLL_INTERVAL;
        let second = next_action_command_poll_interval(first);
        let third = next_action_command_poll_interval(second);

        assert!(first < Duration::from_millis(50));
        assert_eq!(second, Duration::from_millis(2));
        assert_eq!(third, Duration::from_millis(4));
        assert_eq!(
            next_action_command_poll_interval(Duration::from_millis(100)),
            ACTION_COMMAND_MAX_POLL_INTERVAL
        );
    }

    #[test]
    fn action_command_context_includes_gesture_identity() {
        let command = ActionCommand {
            source: ActionSource::Gesture(gesture(Zone::Right, GestureDirection::Up)),
            argv: vec!["notify-send".to_string(), "edgepad".to_string()],
        };

        assert_eq!(
            action_command_context(&command),
            "zone=right direction=up slot=0 tracking_id=42"
        );
    }

    #[test]
    fn gesture_context_includes_gesture_identity() {
        assert_eq!(
            gesture_context(gesture(Zone::Top, GestureDirection::Left)),
            "zone=top direction=left slot=0 tracking_id=42"
        );
    }

    #[test]
    fn slider_step_context_includes_slider_identity() {
        assert_eq!(
            slider_step_context(slider_step(Zone::Right, SliderDirection::Up)),
            "zone=right direction=up slot=0 tracking_id=42"
        );
    }

    fn wait_for_started_command(dispatcher: &ActionDispatcher) {
        let started_at = Instant::now();
        while dispatcher.stats().started_commands == 0 {
            assert!(
                started_at.elapsed() < Duration::from_secs(1),
                "worker should start queued action command"
            );
            thread::sleep(Duration::from_millis(10));
        }
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

    fn slider_binding(
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

    fn command_action(argv: Vec<&str>) -> CommandActionConfig {
        CommandActionConfig::new(argv).expect("test command should be valid")
    }

    fn slider_step(zone: Zone, direction: SliderDirection) -> SliderStep {
        SliderStep {
            zone,
            direction,
            slot: 0,
            tracking_id: 42,
        }
    }
}

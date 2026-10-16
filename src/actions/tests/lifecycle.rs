use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Instant;

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
fn dropping_dispatcher_requests_shutdown_and_waits_for_the_runner() {
    struct WaitingRunner {
        started: mpsc::Sender<()>,
        stopped: mpsc::Sender<bool>,
        force_exit: Arc<AtomicBool>,
    }
    impl ActionCommandRunner for WaitingRunner {
        fn run(
            &mut self,
            _argv: &[String],
            shutdown: &ActionShutdownToken,
        ) -> Result<ActionCommandStatus, String> {
            self.started.send(()).unwrap();
            while !shutdown.is_requested() && !self.force_exit.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(1));
            }
            self.stopped.send(shutdown.is_requested()).unwrap();
            Ok(ActionCommandStatus::success())
        }
    }
    let (started_tx, started_rx) = mpsc::channel();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let force_exit = Arc::new(AtomicBool::new(false));
    let mut dispatcher = ActionDispatcher::with_runner(
        vec![binding(
            Zone::Left,
            GestureDirection::Tap,
            GestureActionConfig::command(["test-command"]).unwrap(),
        )],
        1,
        WaitingRunner {
            started: started_tx,
            stopped: stopped_tx,
            force_exit: force_exit.clone(),
        },
    )
    .unwrap();
    dispatcher.dispatch_gesture(gesture(Zone::Left, GestureDirection::Tap));
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(dispatcher);
    let observed_stop = stopped_rx.recv_timeout(Duration::from_secs(1));
    force_exit.store(true, Ordering::SeqCst);
    if observed_stop.is_err() {
        let _ = stopped_rx.recv_timeout(Duration::from_secs(1));
    }
    assert_eq!(
        observed_stop,
        Ok(true),
        "dropping the dispatcher must stop its worker"
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

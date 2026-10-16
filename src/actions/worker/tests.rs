use super::*;
use crate::actions::test_support::gesture;
use crate::actions::ActionCommandStatus;
use crate::core::{GestureDirection, Zone};
use std::sync::mpsc;

#[test]
fn a_panicking_runner_is_reported_and_future_commands_are_dropped() {
    struct PanicRunner(mpsc::Sender<()>);
    impl ActionCommandRunner for PanicRunner {
        fn run(
            &mut self,
            _argv: &[String],
            _shutdown: &ActionShutdownToken,
        ) -> Result<ActionCommandStatus, String> {
            self.0.send(()).unwrap();
            panic!("test action runner panic");
        }
    }
    let (started_tx, started_rx) = mpsc::channel();
    let worker = ActionWorker::start(1, PanicRunner(started_tx)).unwrap();
    let source = ActionSource::Gesture(gesture(Zone::Left, GestureDirection::Tap));
    worker.enqueue(source, vec!["first".to_string()]);
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !worker.running.as_ref().unwrap().thread.is_finished() {
        assert!(Instant::now() < deadline, "panicking worker did not exit");
        thread::yield_now();
    }
    worker.enqueue(source, vec!["after-panic".to_string()]);
    let stats = worker.shutdown();
    assert_eq!(stats.started_commands, 1);
    assert_eq!(stats.dropped_commands, 1);
    assert_eq!(stats.worker_panics, 1);
    assert_eq!(stats.worker_shutdown_timeouts, 0);
}

#[test]
fn unresponsive_runner_has_a_bounded_shutdown_without_a_second_wait_on_drop() {
    struct IgnoringRunner {
        started: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        finished: mpsc::Sender<()>,
    }
    impl ActionCommandRunner for IgnoringRunner {
        fn run(
            &mut self,
            _argv: &[String],
            _shutdown: &ActionShutdownToken,
        ) -> Result<ActionCommandStatus, String> {
            self.started.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(ActionCommandStatus::success())
        }
    }
    impl Drop for IgnoringRunner {
        fn drop(&mut self) {
            let _ = self.finished.send(());
        }
    }
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let worker = ActionWorker::start(
        1,
        IgnoringRunner {
            started: started_tx,
            release: release_rx,
            finished: finished_tx,
        },
    )
    .unwrap();
    worker.enqueue(
        ActionSource::Gesture(gesture(Zone::Left, GestureDirection::Tap)),
        vec!["unresponsive".to_string()],
    );
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let start = Instant::now();
    let stats = worker.shutdown();
    let elapsed = start.elapsed();
    release_tx.send(()).unwrap();
    finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(elapsed >= ACTION_WORKER_SHUTDOWN_TIMEOUT);
    assert!(elapsed < ACTION_WORKER_SHUTDOWN_TIMEOUT + Duration::from_millis(750));
    assert_eq!(stats.worker_shutdown_timeouts, 1);
    assert_eq!(stats.worker_panics, 0);
    assert_eq!(stats.started_commands, 1);
}

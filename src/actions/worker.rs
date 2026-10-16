use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::command::{ActionCommand, ActionSource};
use super::stats::SharedActionStats;
use super::{ActionCommandRunner, ActionDispatcherStats, ActionShutdownToken};

const ACTION_WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const ACTION_WORKER_SHUTDOWN_JOIN_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(super) struct ActionWorker {
    running: Option<RunningWorker>,
    pub stats: SharedActionStats,
}

#[derive(Debug)]
struct RunningWorker {
    sender: SyncSender<ActionCommand>,
    thread: JoinHandle<()>,
    shutdown: ActionShutdownToken,
}

impl ActionWorker {
    pub(super) fn start<R: ActionCommandRunner>(
        capacity: usize,
        runner: R,
    ) -> Result<Self, String> {
        let (sender, receiver) = sync_channel(capacity);
        let stats = SharedActionStats::default();
        let worker_stats = stats.clone();
        let shutdown = ActionShutdownToken::default();
        let worker_shutdown = shutdown.clone();
        let thread = thread::Builder::new()
            .name("edgepad-action-worker".to_string())
            .spawn(move || run_action_worker(receiver, runner, worker_stats, worker_shutdown))
            .map_err(|err| format!("failed to start edgepad action worker: {err}"))?;
        Ok(Self {
            running: Some(RunningWorker {
                sender,
                thread,
                shutdown,
            }),
            stats,
        })
    }

    pub(super) fn enqueue(&self, source: ActionSource, argv: Vec<String>) {
        let Some(running) = &self.running else {
            self.stats.increment_dropped_commands();
            return;
        };
        match running.sender.try_send(ActionCommand { source, argv }) {
            Ok(()) => self.stats.increment_queued_commands(),
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.stats.increment_dropped_commands();
            }
        }
    }

    pub(super) fn shutdown(mut self) -> ActionDispatcherStats {
        self.stop();
        self.stats.snapshot()
    }

    fn stop(&mut self) {
        let Some(running) = self.running.take() else {
            return;
        };
        running.shutdown.request();
        // Closing the channel also wakes a worker blocked in recv().
        drop(running.sender);
        if wait_for_action_worker_shutdown(&running.thread, ACTION_WORKER_SHUTDOWN_TIMEOUT) {
            if running.thread.join().is_err() {
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
}

impl Drop for ActionWorker {
    fn drop(&mut self) {
        self.stop();
    }
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
                    command.source, command.argv
                );
            }
            Err(err) => {
                stats.increment_failed_commands();
                eprintln!(
                    "edgepad action command failed: {} argv={:?}: {err}",
                    command.source, command.argv
                );
            }
        }
    }
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

#[cfg(test)]
mod tests;

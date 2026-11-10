use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::command::{ActionCommand, ActionSource};
use super::stats::SharedActionStats;
use super::{ActionCommandRunner, ActionDispatcherStats, ActionShutdownToken};

const ACTION_WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const ACTION_WORKER_SHUTDOWN_JOIN_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(super) struct QueuePermit(Arc<AtomicUsize>);

impl Drop for QueuePermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
struct QueuedCommand {
    command: ActionCommand,
    permit: QueuePermit,
}

#[derive(Debug)]
pub(super) struct ActionWorker {
    running: Option<RunningWorker>,
    pub stats: SharedActionStats,
    occupied: Arc<AtomicUsize>,
    capacity: usize,
}

#[derive(Debug)]
struct RunningWorker {
    sender: SyncSender<QueuedCommand>,
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
            occupied: Arc::new(AtomicUsize::new(0)),
            capacity,
        })
    }

    // A reserved permit keeps room for the terminal action throughout a drag.
    pub(super) fn reserve(&self) -> Option<QueuePermit> {
        let mut occupied = self.occupied.load(Ordering::Acquire);
        while occupied < self.capacity {
            match self.occupied.compare_exchange_weak(
                occupied,
                occupied + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(QueuePermit(self.occupied.clone())),
                Err(current) => occupied = current,
            }
        }
        None
    }

    pub(super) fn enqueue(&self, source: ActionSource, argv: Vec<String>) -> bool {
        // Preserve the existing nonblocking rendezvous behavior for capacity zero.
        if self.capacity == 0 {
            self.occupied.fetch_add(1, Ordering::AcqRel);
            return self.enqueue_reserved(source, argv, QueuePermit(self.occupied.clone()));
        }
        let Some(permit) = self.reserve() else {
            self.stats.increment_dropped_commands();
            return false;
        };
        self.enqueue_reserved(source, argv, permit)
    }

    pub(super) fn enqueue_reserved(
        &self,
        source: ActionSource,
        argv: Vec<String>,
        permit: QueuePermit,
    ) -> bool {
        let queued = QueuedCommand {
            command: ActionCommand { source, argv },
            permit,
        };
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.sender.try_send(queued).is_ok())
        {
            self.stats.increment_queued_commands();
            true
        } else {
            self.stats.increment_dropped_commands();
            false
        }
    }

    pub(super) fn drain(&mut self) {
        let Some(running) = self.running.take() else {
            return;
        };
        drop(running.sender);
        if !wait_for_action_worker_shutdown(&running.thread, ACTION_WORKER_SHUTDOWN_TIMEOUT) {
            running.shutdown.request();
        }
        if wait_for_action_worker_shutdown(&running.thread, ACTION_WORKER_SHUTDOWN_TIMEOUT) {
            if running.thread.join().is_err() {
                self.stats.increment_worker_panics();
            }
        } else {
            self.stats.increment_worker_shutdown_timeouts();
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
    receiver: Receiver<QueuedCommand>,
    mut runner: R,
    stats: SharedActionStats,
    shutdown: ActionShutdownToken,
) where
    R: ActionCommandRunner,
{
    while !shutdown.is_requested() {
        let Ok(queued) = receiver.recv() else {
            break;
        };
        if shutdown.is_requested() {
            break;
        }

        let QueuedCommand { command, permit } = queued;
        drop(permit);
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

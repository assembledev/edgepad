use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

use super::{ActionCommandRunner, ActionCommandStatus, ActionShutdownToken};

const ACTION_COMMAND_INITIAL_POLL_INTERVAL: Duration = Duration::from_millis(1);
const ACTION_COMMAND_MAX_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
                return interrupt_action_command(&mut child, program);
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
    // Isolate each action so shutdown can also stop children of `sh -c`,
    // without signaling the daemon's own process group.
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

fn interrupt_action_command(
    child: &mut Child,
    program: &str,
) -> Result<ActionCommandStatus, String> {
    if let Some(status) = child.try_wait().map_err(|err| {
        format!("failed to poll action command {program:?} before shutdown kill: {err}")
    })? {
        return Ok(ActionCommandStatus {
            success: status.success(),
        });
    }
    let killed = kill_action_child(child, program)?;
    let status = child.wait().map_err(|err| {
        if killed {
            format!("failed to wait for killed action command {program:?}: {err}")
        } else {
            format!("failed to wait for action command {program:?}: {err}")
        }
    })?;
    if killed {
        Err(format!(
            "action command {program:?} interrupted by daemon shutdown"
        ))
    } else {
        Ok(ActionCommandStatus {
            success: status.success(),
        })
    }
}

#[cfg(test)]
mod tests;

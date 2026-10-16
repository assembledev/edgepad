//! Signal handlers set atomic flags; the watcher forwards them to runtime tokens.
//! Config loading and action shutdown run on the normal daemon path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use edgepad::proxy::StopToken;

use super::{ReloadToken, DAEMON_SIGNAL_POLL_INTERVAL};

static DAEMON_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

static DAEMON_RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) fn install_daemon_signal_handlers(
    stop: StopToken,
    reload: ReloadToken,
) -> Result<(), String> {
    DAEMON_STOP_REQUESTED.store(false, Ordering::SeqCst);
    DAEMON_RELOAD_REQUESTED.store(false, Ordering::SeqCst);
    register_daemon_signal_handler(libc::SIGINT)?;
    register_daemon_signal_handler(libc::SIGTERM)?;
    register_daemon_signal_handler(libc::SIGHUP)?;
    thread::Builder::new()
        .name("edgepad-daemon-signal".to_string())
        .spawn(move || {
            while !stop.is_stopped() {
                if DAEMON_STOP_REQUESTED.swap(false, Ordering::SeqCst) {
                    stop.stop();
                    return;
                }
                if DAEMON_RELOAD_REQUESTED.swap(false, Ordering::SeqCst) {
                    reload.request();
                }
                thread::sleep(DAEMON_SIGNAL_POLL_INTERVAL);
            }
        })
        .map_err(|err| format!("failed to start daemon signal watcher: {err}"))?;
    Ok(())
}

fn register_daemon_signal_handler(signal: libc::c_int) -> Result<(), String> {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = handle_daemon_signal as *const () as libc::sighandler_t;
    action.sa_flags = 0;
    let sigempty_result = unsafe { libc::sigemptyset(&mut action.sa_mask) };
    if sigempty_result != 0 {
        return Err(format!(
            "failed to initialize daemon signal mask for signal {signal}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let sigaction_result = unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) };
    if sigaction_result != 0 {
        return Err(format!(
            "failed to install daemon signal handler for signal {signal}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

// Signal context must not parse config, log, or lock. Defer that work by setting
// a flag; the watcher transfers the request to the normal runtime path.
extern "C" fn handle_daemon_signal(signal: libc::c_int) {
    if signal == libc::SIGHUP {
        DAEMON_RELOAD_REQUESTED.store(true, Ordering::SeqCst);
    } else {
        DAEMON_STOP_REQUESTED.store(true, Ordering::SeqCst);
    }
}

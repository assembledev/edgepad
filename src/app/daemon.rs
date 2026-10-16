//! Daemon lifecycle: startup retries, action dispatch, reloads, and final reporting.
//! Reload candidates are validated as a whole before the proxy's idle boundary
//! applies recognition changes. Signal delivery is isolated in the child module.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use edgepad::actions::{ActionDispatcher, ActionDispatcherStats};
use edgepad::config::{DeviceConfig, EdgepadConfig};
use edgepad::proxy::{
    run_proxy_with_gesture_handler_and_ready, GestureHandler, ProxyMode, ProxyRecognitionConfig,
    ProxyRunConfig, ProxyRunLimit, ProxyRunSummary, StopToken,
};

use super::cli::DaemonArgs;
use super::presentation::{edge_widths_label, print_action_summary, print_proxy_summary};
use super::profile::{load_daemon_config, validate_daemon_config, DaemonConfigOverrides};

mod signals;
use signals::install_daemon_signal_handlers;

const DAEMON_IDLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);

const DAEMON_SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(50);

const DAEMON_STARTUP_RETRY_TIMEOUT: Duration = Duration::from_secs(30);

const DAEMON_STARTUP_RETRY_INTERVAL: Duration = Duration::from_millis(500);

const DAEMON_ACTION_QUEUE_CAPACITY: usize = 32;

const DAEMON_STARTUP_RETRY_ENV: &str = "EDGEPAD_DAEMON_STARTUP_RETRY_MS";

#[derive(Debug, Clone, Default)]
struct ReloadToken {
    requested: Arc<AtomicBool>,
}

impl ReloadToken {
    fn new() -> Self {
        Self::default()
    }

    fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    fn take_requested(&self) -> bool {
        self.requested.swap(false, Ordering::SeqCst)
    }
}

// Bridges configuration, actions, and the live loop. The proxy requests a
// reload only at an idle boundary; validate the whole candidate here before
// changing either the action bindings or the recognizer settings.
struct DaemonRuntimeHandler {
    dispatcher: ActionDispatcher,
    config_path: PathBuf,
    overrides: DaemonConfigOverrides,
    active_config: EdgepadConfig,
    reload: ReloadToken,
}

impl DaemonRuntimeHandler {
    fn new(args: &DaemonArgs, reload: ReloadToken) -> Result<Self, String> {
        Ok(Self {
            dispatcher: ActionDispatcher::new_with_sliders(
                args.config.gestures.clone(),
                args.config.sliders.clone(),
                DAEMON_ACTION_QUEUE_CAPACITY,
            )?,
            config_path: args.config_path.clone(),
            overrides: args.overrides.clone(),
            active_config: args.config.clone(),
            reload,
        })
    }

    fn active_config(&self) -> &EdgepadConfig {
        &self.active_config
    }

    fn apply_pending_reload(&mut self) -> Option<ProxyRecognitionConfig> {
        if !self.reload.take_requested() {
            return None;
        }

        let candidate = self.load_reload_candidate();
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(err) => {
                eprintln!("edgepad daemon: reload rejected: {err}; keeping previous configuration");
                return None;
            }
        };

        if candidate.device != self.active_config.device {
            eprintln!(
                "edgepad daemon: reload rejected: device changed from {} to {}; restart required; keeping previous configuration",
                device_config_label(&self.active_config.device),
                device_config_label(&candidate.device)
            );
            return None;
        }
        if candidate.palm_rejection != self.active_config.palm_rejection {
            eprintln!("edgepad daemon: reload rejected: palm_rejection changed; restart required; keeping previous configuration");
            return None;
        }
        if candidate == self.active_config {
            eprintln!("edgepad daemon: reload complete; configuration unchanged");
            return None;
        }

        let recognition = ProxyRecognitionConfig {
            edge_widths: candidate.active_edge_widths(),
            engine_options: candidate.engine_options(),
            slider_specs: candidate.slider_specs(),
        };
        self.dispatcher
            .reconfigure(candidate.gestures.clone(), candidate.sliders.clone());
        self.active_config = candidate;
        eprintln!(
            "edgepad daemon: reload complete; edge_widths={} gesture_bindings={} sliders={}",
            edge_widths_label(recognition.edge_widths),
            self.active_config.gestures.len(),
            self.active_config.sliders.len()
        );
        Some(recognition)
    }

    fn load_reload_candidate(&self) -> Result<EdgepadConfig, String> {
        let mut candidate = load_daemon_config(&self.config_path)?;
        self.overrides.apply(&mut candidate);
        validate_daemon_config(&candidate, &self.config_path)?;
        Ok(candidate)
    }

    fn shutdown(self) -> ActionDispatcherStats {
        self.dispatcher.shutdown()
    }
}

impl GestureHandler for DaemonRuntimeHandler {
    fn handle_gesture(&mut self, gesture: edgepad::core::Gesture) {
        self.dispatcher.dispatch_gesture(gesture);
    }

    fn handle_slider_step(&mut self, step: edgepad::core::SliderStep) {
        self.dispatcher.dispatch_slider_step(step);
    }

    fn take_recognition_config_reload(&mut self) -> Option<ProxyRecognitionConfig> {
        self.apply_pending_reload()
    }
}

fn device_config_label(device: &DeviceConfig) -> String {
    match device {
        DeviceConfig::Auto => "auto".to_string(),
        DeviceConfig::Path(path) => path.display().to_string(),
    }
}

pub(super) fn daemon(args: &DaemonArgs) -> Result<(), String> {
    let startup_retry_timeout = daemon_startup_retry_timeout()?;
    let stop = StopToken::new();
    let reload = ReloadToken::new();
    install_daemon_signal_handlers(stop.clone(), reload.clone())?;
    let mut runtime = DaemonRuntimeHandler::new(args, reload)?;
    eprintln!("edgepad daemon: config={}", args.config_path.display());
    eprintln!("edgepad daemon: press Ctrl+C to stop; send SIGHUP to reload");

    let mut run_result = run_daemon_proxy_with_startup_retry(
        &args.input_root,
        stop,
        &mut runtime,
        startup_retry_timeout,
    );
    if let Ok(Some(summary)) = &mut run_result {
        summary.edge_widths = runtime.active_config().active_edge_widths();
    }
    let action_stats = runtime.shutdown();
    if let Some(summary) = run_result? {
        print_proxy_summary(&summary);
    }
    print_action_summary(&action_stats);
    Ok(())
}

fn daemon_startup_retry_timeout() -> Result<Duration, String> {
    match env::var(DAEMON_STARTUP_RETRY_ENV) {
        Ok(raw) => parse_daemon_startup_retry_timeout_ms(&raw),
        Err(env::VarError::NotPresent) => Ok(DAEMON_STARTUP_RETRY_TIMEOUT),
        Err(env::VarError::NotUnicode(_)) => {
            Err(format!("{DAEMON_STARTUP_RETRY_ENV} must be UTF-8"))
        }
    }
}

fn parse_daemon_startup_retry_timeout_ms(raw: &str) -> Result<Duration, String> {
    let millis = raw
        .parse::<u64>()
        .map_err(|_| format!("{DAEMON_STARTUP_RETRY_ENV} must be a non-negative integer"))?;
    Ok(Duration::from_millis(millis))
}

fn run_daemon_proxy_with_startup_retry(
    input_root: &Path,
    stop: StopToken,
    runtime: &mut DaemonRuntimeHandler,
    startup_retry_timeout: Duration,
) -> Result<Option<ProxyRunSummary>, String> {
    let started_at = Instant::now();
    let mut last_announced_device: Option<PathBuf> = None;
    let mut retry_announced = false;

    loop {
        if stop.is_stopped() {
            eprintln!("edgepad daemon: stopped before startup completed");
            return Ok(None);
        }

        let _ = runtime.apply_pending_reload();
        let config = runtime.active_config().clone();

        let run_result = config.device.resolve(input_root).and_then(|device_path| {
            let edge_widths = config.active_edge_widths();
            if last_announced_device.as_ref() != Some(&device_path) {
                eprintln!(
                    "edgepad daemon: device={} edge_widths={} gesture_bindings={} sliders={}",
                    device_path.display(),
                    edge_widths_label(edge_widths),
                    config.gestures.len(),
                    config.sliders.len()
                );
                last_announced_device = Some(device_path.clone());
            }

            run_proxy_with_gesture_handler_and_ready(
                &ProxyRunConfig {
                    device_path,
                    limit: ProxyRunLimit::UntilStopped {
                        stop: stop.clone(),
                        idle_drain_timeout: DAEMON_IDLE_DRAIN_TIMEOUT,
                    },
                    edge_widths,
                    engine_options: config.engine_options(),
                    slider_specs: config.slider_specs(),
                    mode: ProxyMode::UinputGrab,
                },
                runtime,
                &mut |ready_device| {
                    if edgepad::notify::notify_ready(ready_device)? {
                        eprintln!(
                            "edgepad daemon: systemd ready notification sent for {}",
                            ready_device.display()
                        );
                    }
                    Ok(())
                },
            )
        });

        match run_result {
            Ok(summary) => return Ok(Some(summary)),
            Err(err) if should_retry_daemon_startup_error(&err) => {
                let elapsed = started_at.elapsed();
                if startup_retry_timeout == Duration::ZERO || elapsed >= startup_retry_timeout {
                    return Err(format!(
                        "edgepad daemon startup timed out after {:.1}s waiting for device/uinput access; last error: {err}",
                        startup_retry_timeout.as_secs_f32()
                    ));
                }

                if !retry_announced {
                    eprintln!(
                        "edgepad daemon: startup not ready; retrying for up to {:.1}s: {err}",
                        startup_retry_timeout.as_secs_f32()
                    );
                    retry_announced = true;
                }

                let remaining = startup_retry_timeout.saturating_sub(elapsed);
                let retry_delay = DAEMON_STARTUP_RETRY_INTERVAL.min(remaining);
                if !wait_for_daemon_startup_retry(&stop, retry_delay) {
                    eprintln!("edgepad daemon: stopped before startup completed");
                    return Ok(None);
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn wait_for_daemon_startup_retry(stop: &StopToken, delay: Duration) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < delay {
        if stop.is_stopped() {
            return false;
        }
        let remaining = delay.saturating_sub(started_at.elapsed());
        thread::sleep(DAEMON_SIGNAL_POLL_INTERVAL.min(remaining));
    }
    !stop.is_stopped()
}

fn should_retry_daemon_startup_error(err: &str) -> bool {
    err.starts_with("device=auto found no event devices")
        || err.starts_with("device=auto found no readable event devices")
        || err.starts_with("device=auto found no touchpad candidates")
        || err.starts_with("failed to list ")
        || err.starts_with("failed to open device ")
        || err.starts_with("failed to read touchpad capabilities from ")
        || err.starts_with(
            "failed to create virtual touchpad via /dev/uinput before grabbing physical device",
        )
        || err.starts_with("failed to grab device ")
}

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use crate::core::{
    Capabilities, EdgeWidths, Engine, EngineOptions, ResyncContact, SliderSpec, SlotSnapshot,
};
use crate::raw::{RawEvent, RawOutputComposer, RawOutputSink};

use super::{GestureHandler, ProxyRunLimit, ProxyRuntimeStats};
mod input;
mod output;
mod timing;
use input::{FrameAssembler, FrameInput, PhysicalTouchState};
use output::{
    finish_proxy_output, process_proxy_frame_with_handler, process_proxy_recognition_deadline,
    process_proxy_resync_contacts,
};
use timing::{sync_physical_drain_deadline, ProxyLoopStopper, RecognitionDeadline};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProxyInputEvent {
    pub raw: RawEvent,
    pub timestamp: Option<Duration>,
}

#[derive(Debug, Clone)]
pub(super) struct SessionConfig {
    pub capabilities: Capabilities,
    pub edge_widths: EdgeWidths,
    pub engine_options: EngineOptions,
    pub slider_specs: Vec<SliderSpec>,
    pub initial_slot_snapshots: Vec<SlotSnapshot>,
    pub buttonpad: bool,
}

pub(super) struct ResyncSnapshot {
    pub contacts: Vec<ResyncContact>,
    pub slots: Vec<SlotSnapshot>,
    pub pressed_buttons: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionStep {
    Continue,
    /// Read the kernel state and call `restore_snapshot` before the next input event.
    Resync {
        timestamp: Option<Duration>,
    },
    Finished,
}

/// A session needs idle wakeups as well as device events: a pending tap can
/// expire after the finger is released. See `poll_timeout` and `on_idle_poll`.
pub(super) struct ProxySession<'a> {
    engine: Engine,
    composer: RawOutputComposer,
    stats: ProxyRuntimeStats,
    touch_state: PhysicalTouchState,
    stopper: ProxyLoopStopper<'a>,
    drain_deadline: Option<Instant>,
    frames: FrameAssembler,
    recognition_deadline: RecognitionDeadline,
}

impl<'a> ProxySession<'a> {
    pub(super) fn new(config: SessionConfig, limit: &'a ProxyRunLimit) -> Result<Self, String> {
        let mut engine = Engine::with_options(
            config.capabilities,
            config.edge_widths,
            config.slider_specs,
            config.engine_options,
        );
        engine
            .seed_slot_snapshots(&config.initial_slot_snapshots)
            .map_err(|err| format!("failed to seed multitouch slot positions: {err:?}"))?;
        engine.set_buttonpad(config.buttonpad);
        Ok(Self {
            engine,
            composer: RawOutputComposer::new(config.capabilities),
            stats: ProxyRuntimeStats::for_run(limit),
            touch_state: PhysicalTouchState::new(config.capabilities),
            stopper: ProxyLoopStopper::new(limit),
            drain_deadline: None,
            frames: FrameAssembler::default(),
            recognition_deadline: RecognitionDeadline::default(),
        })
    }

    pub(super) fn poll_timeout(&self, now: Instant) -> Option<Duration> {
        let timeout = self
            .drain_deadline
            .map(|deadline| deadline.saturating_duration_since(now))
            .or_else(|| self.stopper.poll_timeout());
        match (timeout, self.recognition_deadline.poll_timeout(now)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub(super) fn on_event<S, H>(
        &mut self,
        event: ProxyInputEvent,
        now: Instant,
        sink: &mut S,
        handler: &mut H,
    ) -> Result<SessionStep, String>
    where
        S: RawOutputSink,
        S::Error: std::fmt::Debug,
        H: GestureHandler,
    {
        match self.frames.push(event) {
            FrameInput::Pending => Ok(SessionStep::Continue),
            FrameInput::Resync(timestamp) => Ok(SessionStep::Resync { timestamp }),
            FrameInput::Dropped(frame) => {
                self.process_frame(&frame, now, sink, handler)?;
                self.touch_state.mark_desynchronized();
                self.stats.input_frame_boundaries += 1;
                // Stopping and reloads wait for the kernel snapshot at the next SYN_REPORT.
                Ok(SessionStep::Continue)
            }
            FrameInput::Boundary(frame) => {
                if let Some(frame) = frame {
                    self.process_frame(&frame, now, sink, handler)?;
                }
                Ok(if self.frame_boundary(now, sink, handler)? {
                    SessionStep::Finished
                } else {
                    SessionStep::Continue
                })
            }
        }
    }

    pub(super) fn restore_snapshot<S, H>(
        &mut self,
        snapshot: ResyncSnapshot,
        timestamp: Option<Duration>,
        now: Instant,
        sink: &mut S,
        handler: &mut H,
    ) -> Result<bool, String>
    where
        S: RawOutputSink,
        S::Error: std::fmt::Debug,
        H: GestureHandler,
    {
        process_proxy_resync_contacts(
            &snapshot.contacts,
            &snapshot.pressed_buttons,
            &mut self.engine,
            &mut self.composer,
            sink,
            &mut self.stats,
            handler,
        )?;
        self.engine
            .seed_slot_snapshots(&snapshot.slots)
            .map_err(|err| format!("failed to restore multitouch slot positions: {err:?}"))?;
        self.recognition_deadline
            .sync(self.engine.next_deadline(), timestamp, now);
        self.touch_state.restore_contacts(&snapshot.contacts);
        self.frame_boundary(now, sink, handler)
    }

    pub(super) fn on_idle_poll<S, H>(
        &mut self,
        now: Instant,
        sink: &mut S,
        handler: &mut H,
    ) -> Result<bool, String>
    where
        S: RawOutputSink,
        S::Error: std::fmt::Debug,
        H: GestureHandler,
    {
        if let Some(input_deadline) = self.recognition_deadline.take_due(now) {
            process_proxy_recognition_deadline(
                input_deadline,
                &mut self.engine,
                &mut self.composer,
                sink,
                &mut self.stats,
                handler,
            )?;
            self.recognition_deadline
                .sync(self.engine.next_deadline(), Some(input_deadline), now);
        }
        let finished = self.stopper.observe_idle_poll(self.is_busy());
        if self.finish_on_stop(finished, now, sink)? {
            return Ok(true);
        }
        if self.frames.is_idle() {
            self.apply_reload(handler);
        }
        Ok(false)
    }

    pub(super) fn into_stats(self) -> ProxyRuntimeStats {
        self.stats
    }

    fn process_frame<S, H>(
        &mut self,
        frame: &crate::raw::RawFrame,
        now: Instant,
        sink: &mut S,
        handler: &mut H,
    ) -> Result<(), String>
    where
        S: RawOutputSink,
        S::Error: std::fmt::Debug,
        H: GestureHandler,
    {
        self.touch_state.observe_frame(frame);
        process_proxy_frame_with_handler(
            &mut self.engine,
            &mut self.composer,
            sink,
            &mut self.stats,
            frame,
            handler,
        )?;
        self.recognition_deadline
            .sync(self.engine.next_deadline(), frame.timestamp, now);
        Ok(())
    }

    fn frame_boundary<S, H>(
        &mut self,
        now: Instant,
        sink: &mut S,
        handler: &mut H,
    ) -> Result<bool, String>
    where
        S: RawOutputSink,
        S::Error: std::fmt::Debug,
        H: GestureHandler,
    {
        self.stats.input_frame_boundaries += 1;
        let finished = self.stopper.observe_frame_boundary(self.is_busy());
        self.stats.idle_drain_frame_boundaries = self.stopper.extra_frame_boundaries();
        if self.finish_on_stop(finished, now, sink)? {
            return Ok(true);
        }
        self.apply_reload(handler);
        Ok(false)
    }

    fn is_busy(&self) -> bool {
        self.touch_state.is_touch_down() || !self.engine.is_recognition_idle()
    }

    fn apply_reload<H: GestureHandler>(&mut self, handler: &mut H) {
        self.stats.recognition_reloads += apply_pending_recognition_reload(
            &mut self.engine,
            self.touch_state.is_touch_down(),
            handler,
        ) as usize;
    }

    fn finish_on_stop<S>(
        &mut self,
        finished: bool,
        now: Instant,
        sink: &mut S,
    ) -> Result<bool, String>
    where
        S: RawOutputSink,
        S::Error: std::fmt::Debug,
    {
        self.drain_deadline = sync_physical_drain_deadline(
            self.drain_deadline,
            &self.stopper,
            self.touch_state.is_touch_down(),
            now,
        );
        let timed_out = self.drain_deadline.is_some_and(|deadline| deadline <= now);
        if !finished && !timed_out {
            return Ok(false);
        }
        self.stats.idle_drain_timed_out = timed_out;
        finish_proxy_output(&mut self.composer, sink, &mut self.stats)?;
        Ok(true)
    }
}

fn apply_pending_recognition_reload<H>(
    engine: &mut Engine,
    physical_touch_down: bool,
    handler: &mut H,
) -> bool
where
    H: GestureHandler,
{
    // A finger can be up while its single tap still awaits a second tap. Keep
    // both recognition settings and action bindings unchanged until that ends.
    if physical_touch_down || !engine.is_recognition_idle() {
        return false;
    }

    let Some(reloaded) = handler.take_recognition_config_reload() else {
        return false;
    };
    engine.reconfigure_recognition(
        reloaded.edge_widths,
        reloaded.slider_specs,
        reloaded.engine_options,
    );
    true
}

#[cfg(test)]
mod tests;

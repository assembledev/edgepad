use super::super::{ProxyRunLimit, StopAfterFrameLimit, StopToken};
use std::time::{Duration, Instant};

const UINPUT_IDLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Default)]
pub(super) struct RecognitionDeadline {
    input_timestamp: Option<Duration>,
    wall_deadline: Option<Instant>,
}

impl RecognitionDeadline {
    pub(super) fn sync(
        &mut self,
        input_deadline: Option<Duration>,
        input_now: Option<Duration>,
        wall_now: Instant,
    ) {
        let Some(input_deadline) = input_deadline else {
            self.input_timestamp = None;
            self.wall_deadline = None;
            return;
        };
        let Some(input_now) = input_now else {
            return;
        };

        // Kernel event timestamps and Instant use different epochs. Only their
        // elapsed durations are comparable, so anchor each engine deadline to
        // the wall clock at the frame that scheduled it.
        self.input_timestamp = Some(input_deadline);
        self.wall_deadline = Some(wall_now + input_deadline.saturating_sub(input_now));
    }

    pub(super) fn poll_timeout(&self, wall_now: Instant) -> Option<Duration> {
        self.wall_deadline
            .map(|deadline| deadline.saturating_duration_since(wall_now))
    }

    pub(super) fn take_due(&mut self, wall_now: Instant) -> Option<Duration> {
        if self
            .wall_deadline
            .is_some_and(|deadline| deadline <= wall_now)
        {
            self.wall_deadline = None;
            return self.input_timestamp.take();
        }
        None
    }
}

pub(super) fn sync_physical_drain_deadline(
    current: Option<Instant>,
    stopper: &ProxyLoopStopper<'_>,
    physical_touch_down: bool,
    now: Instant,
) -> Option<Instant> {
    // This timeout is a safety valve for a contact that never reaches an idle
    // boundary. Pending tap arbitration is already bounded by the recognizer's
    // own deadline and must not be cut short by the physical-contact guard.
    if !stopper.is_draining() || !physical_touch_down {
        return None;
    }

    current.or_else(|| stopper.drain_timeout().map(|timeout| now + timeout))
}

#[derive(Debug, Clone)]
pub(super) struct ProxyLoopStopper<'a> {
    limit: &'a ProxyRunLimit,
    observed_frame_boundaries: usize,
    extra_frame_boundaries: usize,
    draining: bool,
}

impl<'a> ProxyLoopStopper<'a> {
    pub(super) fn new(limit: &'a ProxyRunLimit) -> Self {
        Self {
            limit,
            observed_frame_boundaries: 0,
            extra_frame_boundaries: 0,
            draining: false,
        }
    }

    pub(super) fn observe_frame_boundary(&mut self, recognition_busy: bool) -> bool {
        self.observed_frame_boundaries += 1;
        match self.limit {
            ProxyRunLimit::Frames {
                frame_boundaries,
                stop_after_limit,
            } => self.observe_frame_limit_boundary(
                *frame_boundaries,
                *stop_after_limit,
                recognition_busy,
            ),
            ProxyRunLimit::UntilStopped { stop, .. } => {
                self.observe_stop_token_boundary(stop, recognition_busy)
            }
        }
    }

    pub(super) fn observe_idle_poll(&mut self, recognition_busy: bool) -> bool {
        if self.draining && !recognition_busy {
            return true;
        }
        match self.limit {
            ProxyRunLimit::UntilStopped { stop, .. } if stop.is_stopped() => {
                self.observe_requested_stop(recognition_busy)
            }
            _ => false,
        }
    }

    pub(super) fn poll_timeout(&self) -> Option<Duration> {
        match self.limit {
            ProxyRunLimit::UntilStopped { .. } => Some(STOP_POLL_INTERVAL),
            ProxyRunLimit::Frames { .. } => None,
        }
    }

    pub(super) fn drain_timeout(&self) -> Option<Duration> {
        match self.limit {
            ProxyRunLimit::Frames {
                stop_after_limit: StopAfterFrameLimit::WhenIdle,
                ..
            } => Some(UINPUT_IDLE_DRAIN_TIMEOUT),
            ProxyRunLimit::UntilStopped {
                idle_drain_timeout, ..
            } => Some(*idle_drain_timeout),
            _ => None,
        }
    }

    pub(super) fn is_draining(&self) -> bool {
        self.draining
    }

    pub(super) fn extra_frame_boundaries(&self) -> usize {
        self.extra_frame_boundaries
    }

    fn observe_frame_limit_boundary(
        &mut self,
        frame_boundaries: usize,
        stop_after_limit: StopAfterFrameLimit,
        recognition_busy: bool,
    ) -> bool {
        if self.observed_frame_boundaries < frame_boundaries {
            return false;
        }

        if self.observed_frame_boundaries == frame_boundaries {
            return match stop_after_limit {
                StopAfterFrameLimit::Immediately => true,
                StopAfterFrameLimit::WhenIdle if !recognition_busy => true,
                StopAfterFrameLimit::WhenIdle => {
                    self.draining = true;
                    false
                }
            };
        }

        self.extra_frame_boundaries += 1;
        match stop_after_limit {
            StopAfterFrameLimit::Immediately => true,
            StopAfterFrameLimit::WhenIdle => {
                self.draining = recognition_busy;
                !recognition_busy
            }
        }
    }

    fn observe_stop_token_boundary(&mut self, stop: &StopToken, recognition_busy: bool) -> bool {
        if self.draining {
            self.extra_frame_boundaries += 1;
            self.draining = recognition_busy;
            return !recognition_busy;
        }

        if stop.is_stopped() {
            return self.observe_requested_stop(recognition_busy);
        }

        false
    }

    fn observe_requested_stop(&mut self, recognition_busy: bool) -> bool {
        if recognition_busy {
            self.draining = true;
            false
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests;

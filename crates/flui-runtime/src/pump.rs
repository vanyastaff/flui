//! What one [`UiRealm::pump`](crate::ui_realm::UiRealm::pump) reads and
//! reports: the frame clock it samples once, and the outcome it hands back to
//! the host's pacing.
//!
//! The host decides *whether* a wake becomes a frame (its per-backend wake
//! gate, ADR-0058); the pump is the frame itself once that gate says render.

/// Where a pump reads its frame's timestamp (Flutter's vsync time).
///
/// Read once per pump and handed to the scheduler's begin frame (its frame
/// timing, and the timestamp every transient and post-frame callback
/// receives) and to the realm's `Vsync` tick, so `Vsync` controllers advance
/// on the frame's clock rather than on whenever the tick read the wall clock.
///
/// A `flui_scheduler::Ticker` does not follow it: a controller built on the
/// scheduler measures elapsed time on the wall clock (a recorded divergence,
/// `flui-scheduler`'s `ARCHITECTURE.md`).
pub trait FrameClockSource {
    /// This frame's timestamp. Called exactly once per pump.
    fn frame_time(&mut self) -> web_time::Instant;

    /// Deadline for idle-priority work in this frame.
    ///
    /// The default never defers idle work, which is what every host did
    /// before a pump took a clock.
    fn idle_deadline(&mut self, frame_time: web_time::Instant) -> flui_scheduler::IdleDeadline {
        flui_scheduler::IdleDeadline::far_future(frame_time)
    }
}

/// A clock the host has already read: the one `now` a wake samples for its
/// pacing feedback is also that frame's timestamp.
#[derive(Debug, Clone, Copy)]
pub struct SampledClock(pub web_time::Instant);

impl FrameClockSource for SampledClock {
    fn frame_time(&mut self) -> web_time::Instant {
        self.0
    }
}

/// What one pump did.
///
/// Fields stay private behind accessors so later facts can be added without
/// breaking a host that reads this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct FrameOutcome {
    presented: bool,
}

impl FrameOutcome {
    pub(crate) const fn new(presented: bool) -> Self {
        Self { presented }
    }

    /// Whether the frame reached `present()`. The host's no-present fallback
    /// pacing keys off this: a presented frame was paced by the present
    /// itself, one that did not carries no pacing signal.
    #[must_use]
    pub const fn presented(self) -> bool {
        self.presented
    }
}

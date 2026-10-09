//! What one [`UiRuntime::pump`](crate::ui_runtime::UiRuntime::pump) reads and
//! reports: the frame clock it samples once, and the outcome it hands back to
//! the host's pacing.
//!
//! The host decides *whether* a wake becomes a frame (its per-backend wake
//! gate, ADR-0058); the pump is the frame itself once that gate says render.

/// Where a pump reads its frame's timestamp (the vsync time).
///
/// Read once per pump and handed to the scheduler's begin frame (its frame
/// timing, and the timestamp every transient and post-frame callback
/// receives) and to the UI runtime's `Vsync` tick, so `Vsync` controllers advance
/// on the frame's clock rather than on whenever the tick read the wall clock.
///
/// Each presentation projects this timestamp through its `MotionClock` before
/// sampling its controllers with a typed `FrameTick`.
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

/// The workspace's one virtual clock, [`flui_foundation::ManualClock`], is
/// also a pump's frame clock: a test advances it by hand, so a pump's frame
/// timestamp — and every `Vsync` controller ticked at it — is a value the
/// test controls rather than whatever the wall clock read. It is the clock
/// `flui-testing`'s headless binding already drives its gesture-arena
/// deadlines from, so a driver that pumps a UI runtime off the same handle keeps
/// the frame timestamp and those deadlines on one timeline (clones share it).
///
/// A UI runtime built on [`flui_scheduler::ClockSource::Manual`] with a clone of
/// the same clock measures frame time from that clock's reading at
/// construction, so its frame timestamps, gesture deadlines and produce gate
/// all sit on the one timeline the test advances.
impl FrameClockSource for flui_foundation::ManualClock {
    fn frame_time(&mut self) -> web_time::Instant {
        flui_foundation::MonotonicClock::now(self)
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

//! Frame pacing a presentation reads: vsync, motion policy and clock, shown
//! state and the frame-rate hold.

use flui_animation::__runtime::VsyncDriver;
use flui_animation::{FrameTick, Vsync};
use flui_scheduler::FrameClock;
use web_time::Duration;

use super::PresentationState;

impl PresentationState {
    /// A handle on this presentation's controller registry, for its tree's
    /// `VsyncScope`. Every clone observes the same registry; only
    /// [`Self::vsync_driver`] ticks or closes it (ADR-0382).
    #[must_use]
    pub(crate) fn vsync(&self) -> Vsync {
        self.vsync_driver.vsync().clone()
    }

    /// The driver of this presentation's controller registry: the frame path
    /// ticks it, and retirement closes it.
    #[must_use]
    pub(crate) fn vsync_driver(&self) -> &VsyncDriver {
        &self.vsync_driver
    }

    /// This frame's animation tick for the UI runtime's raw frame time `raw`
    /// (measured from the UI runtime's start): monotonic and finite, so a stale
    /// or repeated `raw` repeats the previous tick.
    ///
    /// The clock's borrow ends inside this call, before the caller hands the
    /// tick to [`VsyncDriver::tick_all`], which runs controller and listener code.
    ///
    /// The borrow ends before registry delivery invokes user code.
    pub(crate) fn motion_tick(&self, raw: Duration) -> FrameTick {
        self.motion_clock.borrow_mut().frame(raw)
    }

    pub(crate) fn motion_is_paused(&self) -> bool {
        self.motion_clock.borrow().is_paused()
    }

    /// Whether a running animation currently asks this presentation for
    /// frames: some controller runs, the motion clock is not paused, frames
    /// are enabled and the presentation is not hidden.
    pub(crate) fn animation_running(&self, frames_enabled: bool) -> bool {
        frames_enabled
            && !self.clock.is_hidden()
            && self.vsync_driver.vsync().has_running()
            && !self.motion_is_paused()
    }

    /// Whether this presentation can draw: it is neither hidden nor closing.
    pub(crate) fn is_shown(&self) -> bool {
        !self.closing_requested.get() && !self.clock.is_hidden()
    }

    /// When the hold on the high refresh rate runs out. A presentation that is
    /// not shown has none: it is released at its next work item instead, so a
    /// deadline nobody can serve never reaches the loop.
    pub(crate) fn frame_rate_hold(&self) -> Option<web_time::Instant> {
        self.is_shown()
            .then(|| self.frame_rate.get().release_at())
            .flatten()
    }

    /// Whether this presentation wants the display's highest refresh rate right
    /// now: an animation runs or a pointer is down.
    pub(crate) fn wants_high_frame_rate(&self, frames_enabled: bool) -> bool {
        self.animation_running(frames_enabled) || self.gestures.active_pointer_count() != 0
    }

    pub(crate) fn motion_policy(&self) -> flui_animation::MotionPolicy {
        self.motion_clock.borrow().policy()
    }

    pub(crate) fn set_motion_override(&self, preference: flui_animation::MotionOverride) {
        self.motion_clock.borrow_mut().set_override(preference);
    }

    pub(crate) fn set_system_motion(&self, system: flui_platform::MotionPreference) {
        self.motion_clock.borrow_mut().set_system_motion(system);
    }

    pub(crate) fn apply_motion(
        &self,
        request: flui_protocol::motion::MotionRequest,
    ) -> Result<(flui_protocol::motion::MotionState, bool), flui_animation::InvalidPlaybackRate>
    {
        let rate = request
            .rate
            .map(flui_animation::PlaybackRate::try_new)
            .transpose()?;
        let mut clock = self.motion_clock.borrow_mut();
        let old_rate = clock.rate();
        let old_time = clock.now();
        if let Some(rate) = rate {
            clock.set_rate(rate);
        }
        if let Some(step_ms) = request.step_ms {
            clock.step(Duration::from_millis(step_ms));
        }
        let demand = clock.now() != old_time || (clock.rate() != old_rate && !clock.is_paused());
        Ok((
            flui_protocol::motion::MotionState::new(
                clock.rate().get(),
                clock.now().as_duration().as_secs_f64() * 1000.0,
            ),
            demand,
        ))
    }

    /// This presentation's own physical-time produce-gate state machine
    /// See [`FrameClock`]'s own doc for the produce decision
    /// it makes.
    #[must_use]
    pub(crate) fn clock(&self) -> &FrameClock {
        &self.clock
    }
}

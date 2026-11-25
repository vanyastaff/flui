//! Failed-frame accounting of a presentation: the failure streak and retry
//! backoff, device recovery and the frames in flight.

use std::num::NonZeroUsize;

use web_time::Instant;

use super::{FailureRetry, PresentationState};

impl PresentationState {
    /// Record one more consecutive dropped frame and return the new streak
    /// length. See [`Self::frame_failure_streak`]'s field doc.
    pub(crate) fn note_frame_failure(&self) -> u32 {
        let streak = self.frame_failure_streak.get().saturating_add(1);
        self.frame_failure_streak.set(streak);
        streak
    }

    /// Current consecutive frame-drop count without changing it.
    pub(crate) fn frame_failure_streak(&self) -> u32 {
        self.frame_failure_streak.get()
    }

    /// A frame showed the current tree, or a segment painted nothing or a scene no sink took;
    /// the next dropped frame starts a fresh streak. See [`Self::frame_failure_streak`]'s field doc.
    pub(crate) fn reset_frame_failure_streak(&self) {
        self.frame_failure_streak.set(0);
        self.failure_retry.set(FailureRetry::default());
    }

    /// Wait until `at` before trying a dropped frame again. An early try
    /// already spent in this interval stays spent.
    pub(crate) fn arm_failure_retry(&self, at: Instant) {
        let retry = self.failure_retry.get();
        self.failure_retry.set(FailureRetry {
            at: Some(at),
            ..retry
        });
    }

    /// Stop waiting out a dropped frame's retry, keeping the streak: the
    /// presentation's frames are held by something else now.
    pub(crate) fn disarm_failure_retry(&self) {
        self.failure_retry.set(FailureRetry::default());
    }

    /// Where the rebuild of this presentation's lost device stands; `None`
    /// while the device is healthy.
    pub(crate) fn device_recovery(&self) -> Option<crate::frame_failure::DeviceRecovery> {
        self.device_recovery.get()
    }

    /// Record where the rebuild of this presentation's lost device stands
    /// (`None`: healthy again) and return what it was.
    pub(crate) fn replace_device_recovery(
        &self,
        recovery: Option<crate::frame_failure::DeviceRecovery>,
    ) -> Option<crate::frame_failure::DeviceRecovery> {
        self.device_recovery.replace(recovery)
    }

    /// Whether this presentation's device rebuild was given up, which holds
    /// its frames until a new rebuild is asked for.
    pub(crate) fn device_abandoned(&self) -> bool {
        self.device_recovery
            .get()
            .is_some_and(crate::frame_failure::DeviceRecovery::is_abandoned)
    }

    /// The frames this presentation's sink holds unsettled, against the limit the sink its last
    /// frame reached may hold: the in-flight gate its clock is polled with (ADR-0305 §5).
    pub(crate) fn frames_in_flight(&self) -> flui_scheduler::FramesInFlight {
        flui_scheduler::FramesInFlight::new(
            self.pending_frames.borrow().len(),
            self.in_flight_limit.get(),
        )
    }

    /// Record the limit of the sink this presentation's frame is handed to. Nothing clamps it: a
    /// sink that names more than the presentation keeps pending breaks its bound, and the newest
    /// records fold past the ring's bound.
    pub(crate) fn set_in_flight_limit(&self, limit: NonZeroUsize) {
        self.in_flight_limit.set(limit);
    }

    /// Whether this presentation has work a frame would do: demand its clock keeps, a redraw it
    /// was asked for, a build or a dirty node, completion callbacks that wait, or a texture frame
    /// the held version owes ([`Self::owes_texture_frame`]). What a
    /// settlement that frees an in-flight place wakes the loop for, unless a dropped frame's
    /// backoff or an abandoned device holds the presentation (ADR-0305 §5).
    pub(crate) fn has_frame_work(&self) -> bool {
        !self.clock().demand_mask().is_empty()
            || self.redraw_pending.get()
            || self.has_pending_work_snapshot()
            || self.has_eligible_completion()
            || self.owes_texture_frame()
    }

    /// Whether the runtime's next frame serves this presentation's [frame work](Self::has_frame_work):
    /// it has some, it can be seen, its sink holds fewer frames than it may, and its device
    /// rebuild was not given up. Otherwise its work waits for something no frame of another
    /// window does: an unseen presentation's for its window to be seen or for the frame that
    /// window is asked for (ADR-0389, the host's half of ADR-0400 §3), a full sink's for the
    /// settlement that frees a place (ADR-0305 §5), an abandoned device's for a new rebuild.
    pub(crate) fn has_frame_work_for_next_frame(&self) -> bool {
        !self.clock().is_hidden()
            && !self.frames_in_flight().is_saturated()
            && !self.device_abandoned()
            && self.has_frame_work()
    }

    /// While this presentation has frames pending, the latest the owner sleeps until: the next
    /// multiple of [`IN_FLIGHT_FLOOR`](crate::ui_runtime::IN_FLIGHT_FLOOR) since the oldest
    /// pending frame was handed to its sink, so a lost settlement wake costs at most that interval, and a
    /// frame that never settles wakes the owner once per interval, not on every read
    /// (ADR-0305 §5). A host whose raster thread has settled every pending frame of this
    /// presentation, and so already woke it, leaves the floor out
    /// (`UiRuntime::next_wake_owing_floors`).
    pub(crate) fn in_flight_floor(&self) -> Option<Instant> {
        let queued = self.pending_frames.borrow().front()?.submitted_at();
        let floor = crate::ui_runtime::IN_FLIGHT_FLOOR;
        let now = self.clock().now();
        let elapsed = now.saturating_duration_since(queued);
        let intervals = elapsed.as_nanos() / floor.as_nanos() + 1;
        let next = u32::try_from(intervals)
            .ok()
            .and_then(|intervals| floor.checked_mul(intervals))
            .and_then(|since| queued.checked_add(since));
        Some(next.unwrap_or_else(|| now.checked_add(floor).unwrap_or(now)))
    }

    /// The instant a dropped frame's wait ends, if one waits.
    pub(crate) fn failure_retry_at(&self) -> Option<Instant> {
        self.failure_retry.get().at
    }

    /// Whether a dropped frame's wait still runs at `now`. A wait whose end has passed holds
    /// nothing, even before a frame attempt ends it ([`Self::admit_failure_retry`]).
    pub(crate) fn failure_backoff_holds(&self, now: Instant) -> bool {
        self.failure_retry_at().is_some_and(|at| now < at)
    }

    /// End the wait now for a key or button press, unless one already did
    /// since the wait last ran out. Returns whether it ended.
    pub(crate) fn retry_failure_early(&self) -> bool {
        let retry = self.failure_retry.get();
        let ends = retry.at.is_some() && !retry.early_spent;
        if ends {
            self.failure_retry.set(FailureRetry {
                at: None,
                early_spent: true,
            });
        }
        ends
    }

    /// Whether a frame may be attempted at `now`: no wait, or its end has
    /// come, which ends it.
    pub(crate) fn admit_failure_retry(&self, now: Instant) -> bool {
        match self.failure_retry.get().at {
            Some(at) if now < at => false,
            Some(_) => {
                self.failure_retry.set(FailureRetry::default());
                true
            }
            None => true,
        }
    }
}

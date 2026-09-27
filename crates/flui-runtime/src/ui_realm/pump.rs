//! The realm's frame transaction (ADR-0083 §1): the one entry a host drives a
//! frame through, and the background wake that runs no frame.

use super::{DrainReport, UiRealm};
use crate::pump::{FrameClockSource, FrameOutcome};
use crate::sink::FrameSink;

/// Publishes a pump's frame timestamp for the frame's duration and clears it
/// on the way out, the unwinding way included, so a panicking frame cannot
/// leave the next un-pumped `draw_frame` reading a stale frame clock.
struct FrameTimeGuard<'a> {
    realm: &'a UiRealm,
}

impl<'a> FrameTimeGuard<'a> {
    fn publish(realm: &'a UiRealm, frame_time: web_time::Instant) -> Self {
        realm.frame_time.set(Some(frame_time));
        Self { realm }
    }
}

impl Drop for FrameTimeGuard<'_> {
    fn drop(&mut self) {
        self.realm.frame_time.set(None);
    }
}

impl UiRealm {
    /// The frame transaction (ADR-0083 §1), in this order:
    ///
    /// 1. apply commands: drain the owner inbox at the Idle boundary (the
    ///    coalesced redraw request is left for the host's gate to read);
    /// 2. begin frame: transient callbacks, microtasks and the one mid-frame
    ///    async poll, at the timestamp `clock` returns;
    /// 3. draw frame: persistent callbacks, then this realm's pipeline and
    ///    the submit through `sink` ([`Self::render_frame`]);
    /// 4. end frame: the shared post-frame queue and this realm's owner-local
    ///    post-frame lane, in one total order.
    ///
    /// `&mut self` is the point: the compiler rules out starting a frame
    /// while another frame on this realm is running. The pump enters the
    /// realm itself for the whole transaction.
    ///
    /// The clock is read once; every phase sees that instant, `Vsync`
    /// controllers included (`now_secs` reads it for the frame's duration).
    /// Whether a wake becomes a frame at all is the host's decision (its wake
    /// gate, ADR-0058), not this method's.
    pub fn pump(
        &mut self,
        clock: &mut dyn FrameClockSource,
        sink: &mut dyn FrameSink,
    ) -> FrameOutcome {
        self.pump_entered(clock, sink)
    }

    fn pump_entered(
        &self,
        clock: &mut dyn FrameClockSource,
        sink: &mut dyn FrameSink,
    ) -> FrameOutcome {
        let now = clock.frame_time();
        let deadline = clock.idle_deadline(now);
        let _frame_time = FrameTimeGuard::publish(self, now);
        self.enter(|realm| {
            let report = realm.drain_commands();
            if report != DrainReport::default() {
                tracing::trace!(?report, "owner inbox drained at pump start");
            }
            let presented = realm.scheduler.drive_frame_with_lane(
                now,
                deadline,
                || realm.render_frame(sink),
                &realm.local_post_frame,
            );
            FrameOutcome::new(presented)
        })
    }

    /// A wake with frames disabled (the app is hidden, paused or detached):
    /// clear the scheduler's frame latch, then poll the async driver once.
    /// No frame runs — no begin frame, no tickers, no pipeline, no present.
    ///
    /// The order is load-bearing. Only a begin frame clears the latch, and
    /// none runs here; polling first would let a future that schedules a
    /// frame find the latch still set, fire no wake, and starve until
    /// unrelated input arrives (see `UpdateScheduler::finish_async_pump`).
    pub fn pump_background(&mut self) {
        self.enter(|realm| {
            realm.scheduler.finish_async_pump();
            realm.scheduler.drive_async_tasks();
        });
    }

    /// Commit the owner inbox at the Idle boundary and report whether the
    /// drain asked for a redraw.
    ///
    /// A host's frame wake calls this once per wake, inside the realm's
    /// entry and before its wake gate, so a command-driven redraw request is
    /// seen by the very wake it produced — and before any early return the
    /// wake takes. Draining on every wake is what keeps the bounded inbox
    /// from filling; the coalesced redraw request is consumed here.
    pub fn drain_owner_inbox(&self) -> bool {
        let report = self.drain_commands();
        if report != DrainReport::default() {
            tracing::trace!(?report, "owner inbox drained");
        }
        self.take_redraw_request()
    }
}

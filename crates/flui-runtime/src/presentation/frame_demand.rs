//! Which frames a presentation asks for: occluded-frame grants, owed frame
//! requests and the wake-only redraw bit.

use std::sync::atomic::Ordering;

use web_time::Instant;

use super::PresentationState;
use crate::occluded_frame::{OccludedFrame, OwedFrameRequest, Session, Sight};

impl PresentationState {
    /// What a frame of this presentation would reach while it is unseen,
    /// from its window's state now; `None` once the window is gone.
    fn occluded_sight(&self, session: Session) -> Option<Sight> {
        self.with_window(|window| Sight::of(&window.state(), self.occluded_surface, session))
    }

    /// Re-read at `now` what the presentation owes its surface: called when
    /// it is built, when it stops or starts being seen, on every new snapshot
    /// of its window (minimized, restored, hidden, shown, covered: a change of
    /// sight that leaves it unseen changes no visibility) and when its session
    /// locks, unlocks, sleeps or wakes. A seen presentation owes nothing; an
    /// unseen one follows [`OccludedFrame::follow`].
    pub(crate) fn follow_sight(&self, session: Session, now: Instant) {
        let next = if self.clock.is_hidden() {
            self.occluded_sight(session)
                .map_or(OccludedFrame::NotOwed, |sight| {
                    self.occluded_frame.get().follow(sight, now)
                })
        } else {
            OccludedFrame::NotOwed
        };
        self.occluded_frame.set(next);
    }

    /// The surface refused or dropped the frame this presentation drew while
    /// unseen, the frame it owed (the settlement matches the frame, not
    /// whatever settles while the window is unseen): nothing more is owed
    /// until it is seen again, however its content changes.
    pub(crate) fn refuse_occluded_frame(&self) {
        if self.clock.is_hidden() {
            self.occluded_frame.set(OccludedFrame::Refused);
        }
    }

    /// The owed frame ran at `now`: the next quiet period starts.
    pub(crate) fn occluded_frame_drawn(&self, now: Instant) {
        if self.clock.is_hidden()
            && matches!(self.occluded_frame.get(), OccludedFrame::QuietSince(_))
        {
            self.occluded_frame.set(OccludedFrame::QuietSince(now));
        }
    }

    /// Whether frame work waits on this presentation: a demand its clock
    /// keeps, a redraw mark, tree work, or a texture frame the held version owes
    /// ([`Self::owes_texture_frame`]).
    pub(crate) fn has_retained_frame_work(&self) -> bool {
        !self.clock.demand_mask().is_empty()
            || self.redraw_pending.get()
            || self.has_pending_work_snapshot()
            || self.owes_texture_frame()
    }

    /// When this presentation, unseen with frame work waiting, draws the
    /// frame it owes its surface: the end of its quiet period, not before its
    /// dropped-frame retry. It stays due until the frame runs, however late;
    /// `None` when the presentation is seen, owes none or has no work, and
    /// while its sink holds as many frames as it may (the settlement that
    /// frees a place wakes it).
    pub(crate) fn occluded_frame_at(&self) -> Option<Instant> {
        if !self.clock.is_hidden()
            || self.closing_requested.get()
            || self.frames_in_flight().is_saturated()
        {
            return None;
        }
        let due = self.occluded_frame.get().due_at(self.failure_retry_at())?;
        self.has_retained_frame_work().then_some(due)
    }

    /// What this presentation's window is to be asked for the frame it owes
    /// its surface ([`Self::occluded_frame_at`], on the event timeline counted
    /// from `epoch`), when that is not what it was last asked: the instant, or
    /// the withdrawal of the one asked. `None` when the window was asked
    /// exactly that, so a window hears an instant once (ADR-0389); an instant
    /// whose frame was held ([`Self::hold_owed_frame`]) is asked again once its
    /// text preparation releases it. A pure read: weighing the hold changes
    /// nothing.
    pub(crate) fn owed_frame_request(&self, epoch: Instant) -> Option<OwedFrameRequest> {
        let at = self
            .occluded_frame_at()
            .map(|at| crate::occluded_frame::event_time(epoch, at));
        OwedFrameRequest::of(&self.window, at, &self.owed_frame_asked, || {
            crate::ui_runtime::text_preparation::outcome(self)
        })
    }

    /// The frame this presentation owes was served and is held, its text
    /// preparation waiting: the request drew nothing, so the window is asked
    /// again once the preparation releases it, not while it waits. Records the
    /// preparation's outcome generation now and nothing else.
    pub(crate) fn hold_owed_frame(&self) {
        let generation = crate::ui_runtime::text_preparation::outcome(self).generation;
        self.owed_frame_asked
            .set(self.owed_frame_asked.get().hold(generation));
    }

    /// This presentation's window delivered a frame (`note_frame_request`):
    /// when the frame it owes its surface has come due, that frame serves it
    /// (ADR-0389). Only a frame of its own window serves it, so the frames of
    /// other windows, and work due elsewhere, never run it.
    pub(crate) fn note_owed_frame_served(&self) {
        if self
            .occluded_frame_at()
            .is_some_and(|due| self.clock.now() >= due)
        {
            self.owed_frame_served.set(true);
        }
    }

    /// Whether a frame of this presentation's window serves the frame it owes
    /// its surface now: delivered since its owed instant came, and not yet
    /// taken by an attempt ([`Self::grant_occluded_frame`]).
    pub(crate) fn serves_owed_frame(&self) -> bool {
        self.owed_frame_served.get() && self.occluded_frame_at().is_some()
    }

    /// Forget a served owed frame no attempt took (a pump that rendered
    /// nothing for this presentation): only a later frame of its window
    /// serves it again.
    pub(crate) fn forget_served_owed_frame(&self) {
        self.owed_frame_served.set(false);
    }

    /// At `now`, grant the owed frame a frame of this presentation's window
    /// served, if it would still be shown (the window may have been locked
    /// since, which only its sight says): the clock produces it at the first
    /// poll that can. The serve is taken whatever comes of it, so a frame held
    /// past it (text preparation) is not served again by the next pump. The
    /// quiet period starts again only when it runs
    /// ([`Self::occluded_frame_drawn`]). Answers whether it was granted.
    pub(crate) fn grant_occluded_frame(&self, session: Session, now: Instant) -> bool {
        if !self.owed_frame_served.replace(false)
            || self.occluded_frame_at().is_none_or(|due| now < due)
        {
            return false;
        }
        if self.occluded_sight(session).is_some_and(Sight::owes) {
            self.clock.grant_hidden_frame();
            true
        } else {
            self.occluded_frame.set(OccludedFrame::NotOwed);
            false
        }
    }

    /// Whether this is the first time the presentation's surface withheld
    /// frames past the budget: `true` once per presentation, for the
    /// report that says so.
    pub(crate) fn first_withdrawal_past_budget(&self) -> bool {
        !self.withheld_reported.replace(true)
    }

    // ========================================================================
    // Pump segment (ADR-0043 §3): per-presentation dirty predicate and wake bit
    // ========================================================================

    /// Mark this presentation's own wake-only redraw bit. See
    /// [`Self::redraw_pending`]'s field doc.
    pub(crate) fn mark_redraw_pending(&self) {
        self.redraw_pending.set(true);
        self.redraw_requests
            .set(self.redraw_requests.get().wrapping_add(1));
    }

    /// Let the next demand poke this presentation's window again: the pump
    /// sampling it is the frame the previous poke asked for.
    pub(crate) fn release_redraw_poke(&self) {
        self.redraw_poked.store(false, Ordering::Release);
    }

    /// Remember how many redraw marks this presentation has had, so the frame
    /// tail can tell a mark raised during the frame from an older one.
    pub(crate) fn watch_redraw_requests(&self) {
        self.redraw_requests_watched.set(self.redraw_requests.get());
    }

    /// Whether this presentation's redraw bit is set by a mark made since
    /// [`Self::watch_redraw_requests`]: a request raised after its segment
    /// took the bit, which the next frame must serve.
    pub(crate) fn redraw_requested_since_watch(&self) -> bool {
        self.redraw_pending.get()
            && self.redraw_requests.get() != self.redraw_requests_watched.get()
    }

    /// Read-and-clear this presentation's wake-only redraw bit — called at
    /// the START of this presentation's pump segment, before dirty is
    /// sampled, so a mark that arrives WHILE the segment runs sets the bit
    /// again rather than being silently absorbed by this read.
    pub(crate) fn take_redraw_pending(&self) -> bool {
        self.redraw_pending.replace(false)
    }
}

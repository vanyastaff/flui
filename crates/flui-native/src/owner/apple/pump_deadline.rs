//! What the AppKit wake pump (`macos::wake_pump`) does at each look at the
//! application's wake deadline. Pure, so every host runs its tests.
//!
//! A look takes the handler's answer through [`WakeDeadline`], the rule every
//! native host shares: a due instant is delivered once, as a frame request to
//! every window, and the same instant is armed again only after a frame ran.
//! A deadline whose frames the handler declines, or that no window draws,
//! therefore costs one delivery and leaves the pump stopped until a frame runs
//! or the answer changes; one a frame left due again, such as the frame a
//! covered window owes its surface while a hold keeps it from running
//! (ADR-0357), is delivered again, however late the look that finds it.

use std::time::Duration;

use web_time::Instant;

use crate::owner::wake_deadline::WakeDeadline;

/// How long after a look that found nothing armed, or after a delivery, the
/// pump looks again.
///
/// One pacing period is the natural scale: the frame a redraw request
/// triggers runs within the pass AppKit schedules for it, and the fallback
/// gate's own deadline is shorter than a period on every display measured so
/// far (9.5 ms against a 100 Hz panel). A fixed delay rather than the
/// display's refresh period because the pump is per-*platform* while the
/// period is per-*window*, and the deadline it reads back is absolute — so
/// being a few milliseconds off costs a re-schedule, never a missed or an
/// early wake.
pub(crate) const FOLLOW_UP: Duration = Duration::from_millis(8);

/// Which look at the deadline a tick is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    /// The look a redraw request scheduled: the frame it asked for may not
    /// have run yet, so finding nothing armed still looks once more.
    First,
    /// A look that found something before, or the second look: finding
    /// nothing armed stops the chain.
    Again,
}

/// What the pump does after a look.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NextLook {
    /// Nothing is armed and no frame is coming: the chain stops.
    Stop,
    /// Look again at the instant: the armed deadline, still ahead, or
    /// [`FOLLOW_UP`] after a first look that found nothing armed.
    At(Instant),
    /// The armed deadline is due, `late` past its instant: ask every window
    /// for a frame, then look again [`FOLLOW_UP`] later, since that frame may
    /// leave a deadline of its own.
    Wake { late: Duration },
}

/// The wake deadline as the AppKit pump arms and delivers it: the owner
/// thread's, reached from the pump's ticks and from the end of each owner
/// turn.
#[derive(Default)]
pub(crate) struct PumpDeadline {
    deadline: WakeDeadline,
}

impl PumpDeadline {
    /// A tick's look at `now`: the handler answered `answer` with `frames`
    /// frames run so far. A due deadline is delivered here, recorded at
    /// `frames`, the count before the frames the delivery asks for.
    pub(crate) fn look(
        &mut self,
        look: Look,
        answer: Option<Instant>,
        now: Instant,
        frames: u64,
    ) -> NextLook {
        match self.deadline.arm(answer, frames) {
            Some(at) if self.deadline.take_due(now, frames) => NextLook::Wake {
                late: now.saturating_duration_since(at),
            },
            Some(at) => NextLook::At(at),
            None => match look {
                Look::First => NextLook::At(now + FOLLOW_UP),
                Look::Again => NextLook::Stop,
            },
        }
    }

    /// An owner turn ended and the handler answered `answer` with `frames`
    /// frames run so far: the instant a tick is owed for, if any. A turn can
    /// leave a deadline no redraw request starts the pump for, such as the
    /// frame a covered window owes its surface, whose window asks for no
    /// frame while it cannot be seen (ADR-0357).
    pub(crate) fn after_owner_turn(
        &mut self,
        answer: Option<Instant>,
        frames: u64,
    ) -> Option<Instant> {
        self.deadline.arm(answer, frames)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use web_time::Instant;

    use super::{FOLLOW_UP, Look, NextLook, PumpDeadline};

    const fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// The pump's looks over a run of ticks: a due deadline is delivered once
    /// per frame generation, a handler that declines its frames (the count
    /// stays) leaves the pump stopped instead of waking every follow-up, a
    /// frame that ran lets the same instant be delivered again, and an owed
    /// frame whose look comes late is still delivered. Red-check: deliver
    /// whenever the answer is not later than `now`, the rule the pump had
    /// before `WakeDeadline`.
    #[test]
    fn a_due_deadline_is_delivered_once_per_frame_generation() {
        let start = Instant::now();
        let at = start + ms(5);
        let later = start + ms(100);
        let mut deadline = PumpDeadline::default();

        // (case, look, answer, now, frames run, next look)
        for (case, look, answer, now, frames, next) in [
            (
                "a first look finds nothing yet",
                Look::First,
                None,
                start,
                0,
                NextLook::At(start + FOLLOW_UP),
            ),
            (
                "the second look finds nothing: the chain stops",
                Look::Again,
                None,
                start,
                0,
                NextLook::Stop,
            ),
            (
                "a deadline ahead is waited for",
                Look::First,
                Some(at),
                start,
                0,
                NextLook::At(at),
            ),
            (
                "due at its instant: one delivery",
                Look::Again,
                Some(at),
                at,
                0,
                NextLook::Wake { late: ms(0) },
            ),
            (
                "the frame was declined: the follow-up stops",
                Look::Again,
                Some(at),
                at + FOLLOW_UP,
                0,
                NextLook::Stop,
            ),
            (
                "a redraw request's look, its frame not run yet",
                Look::First,
                Some(at),
                at + ms(20),
                0,
                NextLook::At(at + ms(20) + FOLLOW_UP),
            ),
            (
                "that frame was declined too: still stopped",
                Look::Again,
                Some(at),
                at + ms(28),
                0,
                NextLook::Stop,
            ),
            (
                "a frame ran and left the same instant due: delivered again",
                Look::Again,
                Some(at),
                at + ms(30),
                1,
                NextLook::Wake { late: ms(30) },
            ),
            (
                "and once for that generation",
                Look::Again,
                Some(at),
                at + ms(38),
                1,
                NextLook::Stop,
            ),
            (
                "another instant is a new deadline",
                Look::Again,
                Some(later),
                at + ms(38),
                1,
                NextLook::At(later),
            ),
        ] {
            assert_eq!(deadline.look(look, answer, now, frames), next, "{case}");
        }
    }

    /// The frame a covered window owes its surface is armed at the end of
    /// the owner turn that left it, with no redraw request behind it, and is
    /// delivered at a look that comes long after its instant. A turn that
    /// leaves the instant already delivered, with no frame run since, owes
    /// no tick. Red-check: answer every deadline from `after_owner_turn`
    /// without the delivery record.
    #[test]
    fn an_owed_frame_is_delivered_when_its_look_is_late() {
        let start = Instant::now();
        let owed = start + ms(250);
        let mut deadline = PumpDeadline::default();

        assert_eq!(
            deadline.after_owner_turn(Some(owed), 0),
            Some(owed),
            "the turn that left the owed frame owes a tick at its instant"
        );
        assert_eq!(
            deadline.look(Look::Again, Some(owed), owed + ms(900), 0),
            NextLook::Wake { late: ms(900) },
            "a late look still delivers the owed frame"
        );
        assert_eq!(
            deadline.after_owner_turn(Some(owed), 0),
            None,
            "a turn after a delivery no frame served owes no tick"
        );
        assert_eq!(
            deadline.after_owner_turn(Some(owed), 1),
            Some(owed),
            "once a frame ran, the frame still owed is armed again"
        );
        assert_eq!(
            deadline.look(Look::Again, Some(owed), owed + ms(950), 1),
            NextLook::Wake { late: ms(950) },
            "and delivered"
        );
    }
}

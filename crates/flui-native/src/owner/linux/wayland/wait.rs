//! How long one turn of the Wayland loop may block in `poll` (ADR-0249's
//! rule on this host).
//!
//! Every producer of work wakes the loop: the compositor's socket, the
//! owner signal and a redraw asked for on another thread write what `poll`
//! waits on. So the loop blocks until woken unless a window has a frame to
//! deliver now, or the application's handler named a wake deadline.
//!
//! A deadline that came due is served by a frame: the loop asks every
//! window for one, once, and arms the same instant again only after a
//! frame ran ([`WakeDeadline`](crate::owner::wake_deadline::WakeDeadline),
//! the rule the X11 and Win32 loops follow). Asking again with no frame
//! since would turn a window that declines its frames, or one the
//! compositor does not draw, into a busy loop. Only the deadline armed
//! bounds the wait.
//!
//! A frame asked of a window whose frame callback is out waits for that
//! callback or its grace, and one asked of a window the compositor has not
//! configured or has suspended waits for the compositor. So the loop wakes
//! for a deadline no earlier than the first window lets a frame through
//! ([`deadline_wake`]): a deadline that moves on each answer, as the
//! runtime's in-flight floor does while a raster thread holds a frame,
//! would otherwise wake the loop at its own pace for frames no window
//! delivers, where the window is never polled.

#![forbid(unsafe_code)]

use std::time::Duration;

use web_time::Instant;

/// When a frame asked of a window now goes through, ordered from the
/// soonest: the first of several windows is their minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FrameHold {
    /// At once: nothing holds the window's next frame back.
    Free,
    /// Once the frame callback that is out is given up at this instant,
    /// or sooner, when the compositor answers it.
    Until(Instant),
    /// Only once the compositor says so: it has not configured the
    /// window, or suspended it. What it sends wakes the loop.
    Compositor,
}

/// When the loop wakes for the wake deadline `deadline`, which it serves
/// by asking every window for a frame, where `first` is the soonest
/// [`FrameHold`] of its windows (`None`: it holds none): at the deadline,
/// but not before a window lets a frame through, and not at all while
/// only the compositor can release one. A deadline the loop wakes for
/// later is still armed and delivered then.
pub(crate) fn deadline_wake(
    deadline: Option<Instant>,
    first: Option<FrameHold>,
) -> Option<Instant> {
    let deadline = deadline?;
    match first {
        None | Some(FrameHold::Free) => Some(deadline),
        Some(FrameHold::Until(released)) => Some(deadline.max(released)),
        Some(FrameHold::Compositor) => None,
    }
}

/// What one turn of the loop knows when it chooses how long to block.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopTurn {
    /// A window has a frame to deliver now.
    pub(crate) frame_due: bool,
    /// The earliest instant a window gives up on a frame callback that
    /// never came, with a frame asked for behind it.
    pub(crate) frame_wait: Option<Instant>,
    /// When the input next delivers something no event brings: a held
    /// key's repeat.
    pub(crate) input_due: Option<Instant>,
    /// The earliest instant a window stops waiting for the compositor to
    /// carry out a mode change it asked for, and publishes what it reads.
    pub(crate) mode_due: Option<Instant>,
    /// The wake deadline armed: one the application's handler asked for
    /// and the last delivery does not hold back. One already due is
    /// delivered on the next turn.
    pub(crate) wake_deadline: Option<Instant>,
}

/// How the last flush of the connection went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flush {
    /// Every request reached the socket.
    Complete,
    /// The socket was full: requests wait in the connection's buffer.
    Partial,
}

/// What the loop does once it has dispatched and flushed, before it
/// would block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeforePoll {
    /// Dispatching recorded events: the windows carry them out before the
    /// loop sleeps, so the turn ends here and the next applies them.
    Apply,
    /// Nothing waits to be applied: block, on the socket becoming readable
    /// and, after a partial flush, writable, so the rest of the requests
    /// go out.
    Poll {
        /// Wait for the socket to take more requests.
        writable: bool,
    },
}

/// What follows `dispatched` events and `flush` before the loop blocks.
pub(crate) fn before_poll(dispatched: usize, flush: Flush) -> BeforePoll {
    if dispatched > 0 {
        BeforePoll::Apply
    } else {
        BeforePoll::Poll {
            writable: flush == Flush::Partial,
        }
    }
}

/// How long `poll` may block; `None` blocks until something wakes the
/// loop.
pub(crate) fn poll_timeout(turn: LoopTurn, now: Instant) -> Option<Duration> {
    if turn.frame_due {
        return Some(Duration::ZERO);
    }
    [
        turn.wake_deadline,
        turn.frame_wait,
        turn.input_due,
        turn.mode_due,
    ]
    .into_iter()
    .flatten()
    .min()
    .map(|instant| instant.saturating_duration_since(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An idle Wayland loop blocks until it is woken: no timer of its own,
    /// a deadline ahead bounds the wait, an armed deadline that came due
    /// ends it at once so the next turn delivers it, and a frame waiting
    /// on a callback that may be lost bounds it too.
    #[test]
    fn an_idle_wayland_loop_blocks_until_it_is_woken() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let behind = now.checked_sub(ms(40));
        let turn = |frame_due, frame_wait, wake_deadline| LoopTurn {
            frame_due,
            frame_wait,
            input_due: None,
            mode_due: None,
            wake_deadline,
        };
        let repeating = |input_due, wake_deadline| LoopTurn {
            frame_due: false,
            frame_wait: None,
            input_due,
            mode_due: None,
            wake_deadline,
        };
        let waiting = |mode_due, wake_deadline| LoopTurn {
            frame_due: false,
            frame_wait: None,
            input_due: None,
            mode_due,
            wake_deadline,
        };
        for (case, turn, expected) in [
            ("idle", turn(false, None, None), None),
            (
                "a frame to deliver",
                turn(true, None, None),
                Some(Duration::ZERO),
            ),
            (
                "a frame to deliver before a deadline ahead",
                turn(true, None, Some(now + ms(250))),
                Some(Duration::ZERO),
            ),
            (
                "a deadline ahead",
                turn(false, None, Some(now + ms(250))),
                Some(ms(250)),
            ),
            (
                "a deadline that came due",
                turn(false, None, behind),
                Some(Duration::ZERO),
            ),
            (
                "a deadline exactly due",
                turn(false, None, Some(now)),
                Some(Duration::ZERO),
            ),
            (
                "a frame waits on a callback that may be lost",
                turn(false, Some(now + ms(700)), None),
                Some(ms(700)),
            ),
            (
                "the earlier of a lost callback and a deadline",
                turn(false, Some(now + ms(700)), Some(now + ms(100))),
                Some(ms(100)),
            ),
            (
                "a held key repeats",
                repeating(Some(now + ms(33)), None),
                Some(ms(33)),
            ),
            (
                "a repeat that came due",
                repeating(behind, None),
                Some(Duration::ZERO),
            ),
            (
                "the earlier of a repeat and a deadline",
                repeating(Some(now + ms(33)), Some(now + ms(500))),
                Some(ms(33)),
            ),
            (
                "a window waits for a mode change",
                waiting(Some(now + ms(900)), None),
                Some(ms(900)),
            ),
            (
                "the earlier of a mode change's wait and a deadline",
                waiting(Some(now + ms(900)), Some(now + ms(500))),
                Some(ms(500)),
            ),
        ] {
            assert_eq!(poll_timeout(turn, now), expected, "{case}");
        }
    }

    /// A wake deadline wakes the loop no sooner than a window lets a
    /// frame through: at once for a window nothing holds or with no
    /// window, at the end of a callback's grace for one whose callback
    /// is out, and never by time for one only the compositor releases.
    /// Several windows are released by the first of them.
    #[test]
    fn a_wake_deadline_waits_for_a_window_to_take_its_frame() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let deadline = Some(now + ms(50));
        let grace = now + ms(1_000);
        let first = |holds: &[FrameHold]| holds.iter().copied().min();
        for (case, deadline, holds, expected) in [
            ("no deadline", None, vec![FrameHold::Free], None),
            ("no window", deadline, vec![], deadline),
            ("a free window", deadline, vec![FrameHold::Free], deadline),
            (
                "a callback out",
                deadline,
                vec![FrameHold::Until(grace)],
                Some(grace),
            ),
            (
                "a callback given up before the deadline",
                deadline,
                vec![FrameHold::Until(now)],
                deadline,
            ),
            (
                "a suspended or unconfigured window",
                deadline,
                vec![FrameHold::Compositor],
                None,
            ),
            (
                "a held window beside a free one",
                deadline,
                vec![FrameHold::Until(grace), FrameHold::Free],
                deadline,
            ),
            (
                "a held window beside a suspended one",
                deadline,
                vec![FrameHold::Compositor, FrameHold::Until(grace)],
                Some(grace),
            ),
            (
                "two callbacks out",
                deadline,
                vec![FrameHold::Until(grace + ms(500)), FrameHold::Until(grace)],
                Some(grace),
            ),
        ] {
            assert_eq!(deadline_wake(deadline, first(&holds)), expected, "{case}");
        }
    }

    /// Events a dispatch recorded are carried out before the loop sleeps,
    /// and a flush the socket cut short is finished when the socket takes
    /// more: neither waits for an unrelated wake.
    #[test]
    fn the_loop_sleeps_only_with_nothing_to_apply_or_send() {
        for (case, dispatched, flush, expected) in [
            (
                "nothing recorded, all sent",
                0,
                Flush::Complete,
                BeforePoll::Poll { writable: false },
            ),
            (
                "nothing recorded, the socket full",
                0,
                Flush::Partial,
                BeforePoll::Poll { writable: true },
            ),
            ("events recorded", 3, Flush::Complete, BeforePoll::Apply),
            (
                "events recorded, the socket full",
                1,
                Flush::Partial,
                BeforePoll::Apply,
            ),
        ] {
            assert_eq!(before_poll(dispatched, flush), expected, "{case}");
        }
    }
}

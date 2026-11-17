//! When a modal loop asks the owner for frames.
//!
//! While a modal loop stands in for the owner's message loop (OLE's
//! `DoDragDrop`, or `DefWindowProc`'s while a window is moved or sized or its
//! menu is open), the owner delivers the wake deadlines and paints that loop
//! does not (`owner::windows::modal_loop::ModalFrames`), each time a timer of
//! the loop fires. [`PaceTimer`] is when that timer fires: a pure rule, so
//! every host checks it.
//!
//! The main loop asks the application's handler for its deadlines each time
//! its queue runs dry, so a deadline the platform's code moved is seen before
//! the loop waits. A modal loop's timer follows the same rule: it fires at the
//! earliest deadline the last delivery armed, and again within
//! [`ASK_AGAIN_MS`] once the platform's code ran on the owner thread (an owner
//! turn, a window's message, a drop target's event), since that code may have
//! moved a deadline. With no deadline and nothing run since, no timer is set:
//! a menu left open costs nothing.

#![forbid(unsafe_code)]

use std::time::Duration;

/// How soon a modal loop asks again once the platform's code ran on the owner
/// thread: Win32's shortest timer (`USER_TIMER_MINIMUM`), which a shorter
/// request is raised to anyway.
pub(crate) const ASK_AGAIN_MS: u32 = 10;

/// The longest a timer is set for (`USER_TIMER_MAXIMUM`).
pub(crate) const LONGEST_WAIT_MS: u32 = 0x7FFF_FFFF;

/// The pacing timer of one modal loop: whether it is set, and when it fires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PaceTimer {
    /// No timer is set: no modal loop runs, or nothing is due and the
    /// platform's code has not run since the last delivery.
    #[default]
    Stopped,
    /// The timer fires at `due`.
    Armed {
        /// When it fires.
        due: web_time::Instant,
    },
    /// The timer fired and its delivery runs. `asked_again` once the
    /// platform's code ran meanwhile: what the delivery painted may have moved
    /// a deadline it already read.
    Delivering {
        /// The platform's code ran during the delivery.
        asked_again: bool,
    },
}

/// What the loop does to its timer after a step of [`PaceTimer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimerChange {
    /// Set it to fire in this many milliseconds, replacing one already set.
    Set(u32),
    /// Kill it.
    Kill,
    /// Leave it as it is.
    Keep,
}

impl PaceTimer {
    /// The platform's code ran on the owner thread, at `now`: the timer fires
    /// within [`ASK_AGAIN_MS`]. One set to fire sooner is kept, so code that
    /// runs more often than that (the pointer moving a window) never holds the
    /// timer back; during a delivery the delivery asks again as it ends. A
    /// modal loop starting asks this way too, for its first delivery.
    pub(crate) fn ask_again(&mut self, now: web_time::Instant) -> TimerChange {
        let soon = now + Duration::from_millis(u64::from(ASK_AGAIN_MS));
        match *self {
            Self::Delivering { .. } => {
                *self = Self::Delivering { asked_again: true };
                TimerChange::Keep
            }
            Self::Armed { due } if due <= soon => TimerChange::Keep,
            Self::Stopped | Self::Armed { .. } => {
                *self = Self::Armed { due: soon };
                TimerChange::Set(ASK_AGAIN_MS)
            }
        }
    }

    /// The timer fired: its delivery starts.
    pub(crate) fn deliver(&mut self) {
        *self = Self::Delivering { asked_again: false };
    }

    /// The delivery ended at `now`, with `deadline` the earliest instant it
    /// armed (`None` when nothing is due). The timer fires at that deadline,
    /// rounded up to a whole millisecond and at least one, or within
    /// [`ASK_AGAIN_MS`] if the platform's code ran during the delivery,
    /// whichever comes first; with neither, it is killed.
    pub(crate) fn delivered(
        &mut self,
        deadline: Option<web_time::Instant>,
        now: web_time::Instant,
    ) -> TimerChange {
        let asked_again = matches!(*self, Self::Delivering { asked_again: true })
            .then(|| now + Duration::from_millis(u64::from(ASK_AGAIN_MS)));
        let Some(due) = deadline.into_iter().chain(asked_again).min() else {
            *self = Self::Stopped;
            return TimerChange::Kill;
        };
        *self = Self::Armed { due };
        let millis = due
            .saturating_duration_since(now)
            .as_nanos()
            .div_ceil(1_000_000);
        TimerChange::Set(
            u32::try_from(millis.clamp(1, u128::from(LONGEST_WAIT_MS))).unwrap_or(LONGEST_WAIT_MS),
        )
    }

    /// The modal loop ended: no timer.
    pub(crate) fn stop(&mut self) -> TimerChange {
        *self = Self::Stopped;
        TimerChange::Kill
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{ASK_AGAIN_MS, PaceTimer, TimerChange};

    /// A delivery that leaves no deadline, with nothing run during it, kills
    /// the timer: a modal loop left open asks nothing until the platform's
    /// code runs again. Red-check: answer no deadline with the old pace (a
    /// timer set every frame), and the first row fails.
    #[test]
    fn a_delivery_sets_the_timer_for_what_is_due_and_nothing_else() {
        let now = web_time::Instant::now();
        let ms = Duration::from_millis;
        let past = now.checked_sub(ms(5)).expect("a past instant");
        // (case, deadline, platform code ran during the delivery, change)
        let rows = [
            ("nothing due", None, false, TimerChange::Kill),
            ("due now", Some(now), false, TimerChange::Set(1)),
            ("already past", Some(past), false, TimerChange::Set(1)),
            (
                "a sub-millisecond rounds up",
                Some(now + Duration::from_micros(4_200)),
                false,
                TimerChange::Set(5),
            ),
            (
                "far off is not polled for",
                Some(now + ms(3_000)),
                false,
                TimerChange::Set(3_000),
            ),
            (
                "nothing due, code ran",
                None,
                true,
                TimerChange::Set(ASK_AGAIN_MS),
            ),
            (
                "far off, code ran",
                Some(now + ms(3_000)),
                true,
                TimerChange::Set(ASK_AGAIN_MS),
            ),
            (
                "due sooner than asking again",
                Some(now + ms(2)),
                true,
                TimerChange::Set(2),
            ),
        ];
        for (case, deadline, ran, change) in rows {
            let mut timer = PaceTimer::Stopped;
            timer.deliver();
            if ran {
                assert_eq!(timer.ask_again(now), TimerChange::Keep, "{case}");
            }
            assert_eq!(timer.delivered(deadline, now), change, "{case}");
            let stopped = change == TimerChange::Kill;
            assert_eq!(timer == PaceTimer::Stopped, stopped, "{case}");
        }
    }

    /// The platform's code running sets a stopped timer to ask again, keeps
    /// one set to fire sooner, and brings one set for later forward.
    #[test]
    fn platform_code_asks_again_without_holding_the_timer_back() {
        let now = web_time::Instant::now();
        let ms = Duration::from_millis;
        let soon = now + ms(u64::from(ASK_AGAIN_MS));
        // (case, timer before, change, timer after)
        let rows = [
            (
                "stopped",
                PaceTimer::Stopped,
                TimerChange::Set(ASK_AGAIN_MS),
                PaceTimer::Armed { due: soon },
            ),
            (
                "set sooner",
                PaceTimer::Armed { due: now + ms(3) },
                TimerChange::Keep,
                PaceTimer::Armed { due: now + ms(3) },
            ),
            (
                "set for later",
                PaceTimer::Armed { due: now + ms(500) },
                TimerChange::Set(ASK_AGAIN_MS),
                PaceTimer::Armed { due: soon },
            ),
            (
                "delivering",
                PaceTimer::Delivering { asked_again: false },
                TimerChange::Keep,
                PaceTimer::Delivering { asked_again: true },
            ),
        ];
        for (case, before, change, after) in rows {
            let mut timer = before;
            assert_eq!(timer.ask_again(now), change, "{case}");
            assert_eq!(timer, after, "{case}");
        }

        // Code that runs every few milliseconds lets the timer fire.
        let mut timer = PaceTimer::Stopped;
        let _ = timer.ask_again(now);
        for step in 1..ASK_AGAIN_MS {
            assert_eq!(
                timer.ask_again(now + ms(u64::from(step))),
                TimerChange::Keep,
                "{step} ms later"
            );
        }
        assert_eq!(timer.stop(), TimerChange::Kill);
        assert_eq!(timer, PaceTimer::Stopped);
    }
}

//! When a Wayland window delivers the frame it was asked for: once the
//! compositor has shown the last one (its frame callback's `done`), or
//! once that callback is given up.
//!
//! The raster thread arms the callback just before it presents, and the
//! present's commit carries it to the compositor. A present that is
//! skipped (an outdated surface, a zero size) commits nothing, so its
//! callback never comes: after its grace the window stops waiting for it.
//! A window the compositor does not draw, without saying so, sends no `done`
//! either, so its callbacks go unanswered one after another: each one the
//! window gives up doubles the grace of the next, from [`CALLBACK_GRACE`]
//! up to [`MAX_CALLBACK_GRACE`], so an unpaced window is asked for a frame
//! less and less often, down to once per ceiling, and never polled. The
//! first `done` the compositor sends ends the backoff: the window is paced
//! again. Each callback carries a ticket, so a `done` of an older one ends
//! the backoff but never releases the callback that is out now: the
//! compositor showed an earlier commit, not the one the window waits on.
//!
//! A window the compositor says it does not show (`suspended`: covered by
//! other windows, on a hidden workspace, its outputs off) is occluded: it
//! is given no frame, and no grace runs out for it. The frame asked for
//! is kept, and delivered once the compositor shows the window again.

#![forbid(unsafe_code)]

use std::time::Duration;

use web_time::Instant;

/// How long a window waits for a frame callback before it gives it up,
/// while the compositor answered the last one.
pub(crate) const CALLBACK_GRACE: Duration = Duration::from_secs(1);

/// The longest a window waits for a frame callback: the grace after enough
/// callbacks in a row went unanswered.
pub(crate) const MAX_CALLBACK_GRACE: Duration = Duration::from_secs(8);

/// The grace of a callback armed after `unanswered` callbacks in a row were
/// given up: [`CALLBACK_GRACE`] doubled once per callback given up, at most
/// [`MAX_CALLBACK_GRACE`].
pub(crate) fn grace(unanswered: u32) -> Duration {
    CALLBACK_GRACE
        .checked_mul(1_u32.checked_shl(unanswered).unwrap_or(u32::MAX))
        .map_or(MAX_CALLBACK_GRACE, |grace| grace.min(MAX_CALLBACK_GRACE))
}

/// A frame callback that is out: when it was armed, its ticket, and the
/// grace it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Out {
    armed: Instant,
    ticket: u64,
    grace: Duration,
}

/// A window's frame callbacks: the one out, if any, the last ticket issued
/// and how many callbacks in a row the window gave up unanswered.
#[derive(Debug, Default)]
pub(crate) struct Callbacks {
    out: Option<Out>,
    issued: u64,
    unanswered: u32,
}

impl Callbacks {
    /// The instant the window gives up the callback that is out.
    pub(crate) fn gives_up_at(&self) -> Option<Instant> {
        self.out.map(|out| out.armed + out.grace)
    }

    /// Arm a callback at `now` unless one still holds the window back: the
    /// ticket the callback carries to its `done`, or `None` when the window
    /// still waits on the one out.
    pub(crate) fn arm(&mut self, now: Instant) -> Option<u64> {
        if self.gives_up_at().is_some_and(|until| now < until) {
            return None;
        }
        // A ticket per present: a window would present for longer than the
        // universe lasts before the count saturated, and two equal tickets
        // then only release a callback a little early.
        self.issued = self.issued.saturating_add(1);
        self.out = Some(Out {
            armed: now,
            ticket: self.issued,
            grace: grace(self.unanswered),
        });
        Some(self.issued)
    }

    /// The compositor answered the callback `ticket` (`done`): it draws the
    /// window, so the backoff ends. The callback out is released when it is
    /// that one or an older one: a commit's callback comes after every
    /// earlier commit's.
    pub(crate) fn answered(&mut self, ticket: u64) {
        self.unanswered = 0;
        if self.out.is_some_and(|out| out.ticket <= ticket) {
            self.out = None;
        }
    }

    /// The window delivers a frame at `now`: a callback out past its grace is
    /// given up, and the next one waits twice as long, up to the ceiling.
    pub(crate) fn deliver(&mut self, now: Instant) {
        if self.gives_up_at().is_some_and(|until| now >= until) {
            self.out = None;
            self.unanswered = self.unanswered.saturating_add(1);
        }
    }
}

/// What decides whether a window delivers a frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pacing {
    /// A frame was asked for.
    pub(crate) asked: bool,
    /// The compositor configured the window.
    pub(crate) configured: bool,
    /// The compositor does not show the window.
    pub(crate) suspended: bool,
    /// When the frame callback that is out is given up
    /// ([`Callbacks::gives_up_at`]).
    pub(crate) callback_until: Option<Instant>,
}

impl Pacing {
    /// Whether the window delivers its frame now.
    pub(crate) fn due(self, now: Instant) -> bool {
        self.asked
            && self.configured
            && !self.suspended
            && self.callback_until.is_none_or(|until| now >= until)
    }

    /// When the window gives up the callback that holds its frame back;
    /// `None` with nothing armed, nothing asked for, or while the window
    /// is not shown, whose frame waits for the compositor instead.
    pub(crate) fn gives_up_at(self) -> Option<Instant> {
        self.callback_until
            .filter(|_| self.asked && !self.suspended)
    }
}

/// The first output in `entered` (the order a surface entered them) the
/// compositor still announces: one removed while the surface was on it
/// sends the surface no `leave`.
pub(crate) fn first_live<T: Copy>(entered: &[T], live: impl Fn(T) -> bool) -> Option<T> {
    entered.iter().copied().find(|output| live(*output))
}

/// What the compositor said about whether it draws a window.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sight {
    /// The window acknowledged a configure.
    pub(crate) configured: bool,
    /// The compositor answered a frame callback of the window's: it
    /// mapped the window at least once.
    pub(crate) shown: bool,
    /// The surface is on an output now.
    pub(crate) on_output: bool,
    /// The compositor suspended the window.
    pub(crate) suspended: bool,
}

impl Sight {
    /// Whether the window is occluded: not configured yet, suspended, or
    /// shown once and on no output now. A configured window the
    /// compositor has not shown yet is on no output either, since a
    /// surface enters one only once it is mapped with its first frame; it
    /// is not occluded, or it would never be given that frame.
    pub(crate) fn occluded(self) -> bool {
        !self.configured || self.suspended || (self.shown && !self.on_output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A configured window is occluded once the compositor suspends it,
    /// or once it showed the window and the window is on no output; a
    /// window it has not shown yet waits for its first frame unoccluded,
    /// on no output.
    #[test]
    fn a_window_is_occluded_once_shown_and_off_every_output() {
        for (case, configured, shown, on_output, suspended, expected) in [
            ("not configured", false, false, false, false, true),
            (
                "configured, before its first frame",
                true,
                false,
                false,
                false,
                false,
            ),
            ("shown, on an output", true, true, true, false, false),
            ("shown, on no output", true, true, false, false, true),
            (
                "suspended before its first frame",
                true,
                false,
                false,
                true,
                true,
            ),
            ("suspended on an output", true, true, true, true, true),
        ] {
            let sight = Sight {
                configured,
                shown,
                on_output,
                suspended,
            };
            assert_eq!(sight.occluded(), expected, "{case}");
        }
    }

    /// A frame waits on the callback its present armed until the
    /// compositor answers or the grace runs out; a present that committed
    /// nothing never holds the window for longer.
    #[test]
    fn a_lost_frame_callback_holds_the_window_for_its_grace_only() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let mut callbacks = Callbacks::default();
        assert_eq!(callbacks.gives_up_at(), None, "nothing armed");
        assert_eq!(callbacks.arm(now), Some(1), "armed with the first ticket");
        assert_eq!(callbacks.arm(now + ms(900)), None, "still waiting on it");
        assert_eq!(callbacks.gives_up_at(), Some(now + CALLBACK_GRACE));
        let pacing = |asked, callback_until| Pacing {
            asked,
            configured: true,
            suspended: false,
            callback_until,
        };
        let until = callbacks.gives_up_at();
        assert_eq!(pacing(true, until).gives_up_at(), until);
        assert_eq!(
            pacing(false, until).gives_up_at(),
            None,
            "no frame asked for"
        );
        assert_eq!(pacing(true, None).gives_up_at(), None);
        assert!(pacing(true, None).due(now));
        assert!(!pacing(true, until).due(now), "the callback holds it");
        assert!(
            pacing(true, until).due(now + CALLBACK_GRACE),
            "given up at the grace"
        );
        assert!(!pacing(false, None).due(now), "nothing asked for");
        let unconfigured = Pacing {
            configured: false,
            ..pacing(true, None)
        };
        assert!(!unconfigured.due(now));
    }

    /// An occluded window (the compositor suspended it) is given no frame
    /// and gives up no callback, however long it waits; the frame asked
    /// for is delivered once the compositor shows the window again.
    #[test]
    fn an_occluded_window_holds_its_frame_until_it_is_shown() {
        let now = Instant::now();
        let long_ago = now.checked_sub(CALLBACK_GRACE * 5);
        let occluded = Pacing {
            asked: true,
            configured: true,
            suspended: true,
            callback_until: long_ago.map(|armed| armed + CALLBACK_GRACE),
        };
        assert!(!occluded.due(now), "past the grace, still occluded");
        assert_eq!(occluded.gives_up_at(), None, "no wake for a grace");
        let unarmed = Pacing {
            callback_until: None,
            ..occluded
        };
        assert!(!unarmed.due(now), "nothing armed, still occluded");
        let shown = Pacing {
            suspended: false,
            ..occluded
        };
        assert!(shown.due(now), "shown again: the kept frame goes out");
    }

    /// A window whose callbacks go unanswered is asked for frames less and
    /// less often: each callback it gives up doubles the next one's grace up
    /// to the ceiling, and the compositor's first `done` ends the backoff. A
    /// `done` of an older callback ends it too, but never releases the one
    /// out now. Red-check: keep `grace(0)` for every callback and the
    /// window is asked for a frame once a second forever; release the
    /// callback out on any `done` and the stale one lets the next frame go
    /// before the compositor showed the last.
    #[test]
    fn unanswered_callbacks_back_off_to_the_ceiling_until_one_is_answered() {
        let start = Instant::now();
        let mut callbacks = Callbacks::default();
        let mut now = start;
        // (callback, the grace it gets)
        let schedule = [
            (1, CALLBACK_GRACE),
            (2, CALLBACK_GRACE * 2),
            (3, CALLBACK_GRACE * 4),
            (4, MAX_CALLBACK_GRACE),
            (5, MAX_CALLBACK_GRACE),
        ];
        for (ticket, wait) in schedule {
            assert_eq!(callbacks.arm(now), Some(ticket), "callback {ticket}");
            assert_eq!(
                callbacks.gives_up_at(),
                Some(now + wait),
                "callback {ticket}"
            );
            callbacks.deliver(now + wait.saturating_sub(Duration::from_millis(1)));
            assert!(callbacks.gives_up_at().is_some(), "not given up early");
            now += wait;
            callbacks.deliver(now);
            assert_eq!(callbacks.gives_up_at(), None, "given up at its grace");
        }
        let elapsed = now.saturating_duration_since(start);
        assert_eq!(
            elapsed,
            CALLBACK_GRACE * 7 + MAX_CALLBACK_GRACE * 2,
            "five frames in twenty-three seconds, not one a second"
        );

        // Two callbacks out in turn: the older one's `done` comes late.
        let older = callbacks.arm(now).expect("nothing out");
        now += MAX_CALLBACK_GRACE;
        callbacks.deliver(now);
        let current = callbacks.arm(now).expect("the older one was given up");
        callbacks.answered(older);
        assert_eq!(
            callbacks.gives_up_at(),
            Some(now + MAX_CALLBACK_GRACE),
            "an older `done` leaves the callback out"
        );
        callbacks.answered(current);
        assert_eq!(callbacks.gives_up_at(), None, "its own `done` releases it");
        callbacks.arm(now);
        assert_eq!(
            callbacks.gives_up_at(),
            Some(now + CALLBACK_GRACE),
            "an answered window is paced again"
        );
        assert_eq!(grace(u32::MAX), MAX_CALLBACK_GRACE, "no overflow");
    }

    /// A surface is on the first output it entered that still exists.
    #[test]
    fn a_removed_output_names_no_display() {
        let live = |output: u32| output != 7;
        assert_eq!(first_live(&[7, 9], live), Some(9));
        assert_eq!(first_live(&[7], live), None);
        assert_eq!(first_live::<u32>(&[], live), None);
    }
}

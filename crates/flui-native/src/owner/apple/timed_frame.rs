//! An AppKit window's timed frame request
//! (`PlatformWindow::request_frame_at`, ADR-0389): the instant the window was
//! asked for a frame at, which a tick on the owner queue serves for that window
//! alone, once. Pure, so every host runs its tests.
//!
//! The frame a covered window owes its surface reaches it this way, not through
//! the application's wake deadline: the wake pump asks every window for a frame
//! and arms the same instant again after a frame of any window, so a covered
//! window AppKit runs no display pass for would have its instant delivered
//! again at the frame rate of the windows beside it. A served instant is gone,
//! and only the next request arms another tick.
//!
//! One tick is live at a time: each tick armed carries the generation it was
//! armed at ([`ArmedTick`]), and only the one armed last serves. A request for
//! the instant the live tick serves arms none; any other request arms a tick
//! of a new generation, so a tick armed earlier does nothing when it runs,
//! whatever instant it names, as `WaitEndRead` keeps a window's mode-wait read.
//! The owner queue cannot take a tick back, so a superseded one still runs,
//! and finds itself stale.

use std::time::Duration;

use flui_platform::EventTime;

/// A tick armed for a window's timed frame request: the instant it serves and
/// the generation it was armed at, which only the live tick still has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArmedTick {
    at: EventTime,
    generation: u64,
}

/// The tick an AppKit window's timed frame request has live, if any, and the
/// generation the next one is armed at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimedFrame {
    live: Option<ArmedTick>,
    armed: u64,
}

/// What a tick does when it runs ([`TimedFrame::tick`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimedTick {
    /// The tick is the live one and its instant has come: ask the window for
    /// a frame, once.
    Serve,
    /// The tick is the live one and its instant has not come (the queue's
    /// timer ran early): run it again after this long.
    Wait(Duration),
    /// A later request superseded the tick, or its instant was served or
    /// withdrawn: nothing to do.
    Stale,
}

impl TimedFrame {
    /// The window was asked for a frame at `at`, or the instant asked before
    /// withdrawn (`None`), at `now`. Answers the tick to arm and how long
    /// after `now` it is to run: `None` when the live tick already serves
    /// `at`, or nothing is asked. An instant served before is armed again:
    /// asking for it again means its frame is still owed.
    ///
    /// # Panics
    ///
    /// Past 2^64 ticks armed for one window.
    pub(crate) fn request(
        &mut self,
        at: Option<EventTime>,
        now: EventTime,
    ) -> Option<(ArmedTick, Duration)> {
        if self.live.map(|tick| tick.at) == at {
            return None;
        }
        let Some(at) = at else {
            self.live = None;
            return None;
        };
        self.armed = self
            .armed
            .checked_add(1)
            .expect("BUG: a window arms fewer than 2^64 timed frame ticks");
        let tick = ArmedTick {
            at,
            generation: self.armed,
        };
        self.live = Some(tick);
        Some((tick, at.saturating_duration_since(now)))
    }

    /// `tick` runs at `now`. Only the live tick serves, and once: its instant
    /// is then gone, so a window is asked once for it however many ticks name
    /// it.
    pub(crate) fn tick(&mut self, tick: ArmedTick, now: EventTime) -> TimedTick {
        if self.live != Some(tick) {
            return TimedTick::Stale;
        }
        if now < tick.at {
            return TimedTick::Wait(tick.at.saturating_duration_since(now));
        }
        self.live = None;
        TimedTick::Serve
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform::EventTime;

    use super::{ArmedTick, TimedFrame, TimedTick};

    fn ms(millis: u64) -> EventTime {
        EventTime::from_nanos(millis * 1_000_000)
    }

    fn armed(frame: &mut TimedFrame, at: u64, now: u64) -> ArmedTick {
        frame
            .request(Some(ms(at)), ms(now))
            .map(|(tick, _)| tick)
            .expect("a new instant arms a tick")
    }

    /// A window's request is served by the tick armed for it, once, at its
    /// instant: a tick that runs early waits out the rest, and ticks for an
    /// instant that was served, withdrawn or replaced do nothing. A request
    /// for the instant the live tick serves arms no second tick. Red-check:
    /// serve every tick whose instant has come (the wake deadline's rule
    /// without its frame count), and the instant served is served again; or
    /// arm a tick for every request, and two ticks are queued for one instant.
    #[test]
    fn a_timed_request_is_served_once_at_its_instant() {
        let mut frame = TimedFrame::default();
        let first = frame.request(Some(ms(1000)), ms(0));
        assert_eq!(
            first.map(|(_, delay)| delay),
            Some(Duration::from_millis(1000)),
            "armed"
        );
        let (tick, _) = first.expect("armed");
        assert_eq!(
            frame.request(Some(ms(1000)), ms(10)),
            None,
            "the live tick serves the same instant: none more is armed"
        );
        // (case, now, what the tick does)
        for (case, now, found) in [
            (
                "a tick that runs early waits",
                ms(990),
                TimedTick::Wait(Duration::from_millis(10)),
            ),
            ("served at its instant", ms(1000), TimedTick::Serve),
            ("and once", ms(1016), TimedTick::Stale),
        ] {
            assert_eq!(frame.tick(tick, now), found, "{case}");
        }

        let replaced = armed(&mut frame, 2000, 1500);
        let replacing = frame.request(Some(ms(3000)), ms(1500));
        assert_eq!(
            replacing.map(|(_, delay)| delay),
            Some(Duration::from_millis(1500)),
            "replaced"
        );
        assert_eq!(
            frame.tick(replaced, ms(2500)),
            TimedTick::Stale,
            "the replaced instant's tick"
        );
        assert_eq!(
            frame.request(None, ms(2500)),
            None,
            "withdrawn: nothing armed"
        );
        let withdrawn = replacing.expect("armed").0;
        assert_eq!(
            frame.tick(withdrawn, ms(3000)),
            TimedTick::Stale,
            "the withdrawn instant's tick"
        );
    }

    /// An instant asked, replaced, then asked again (A, B, A): the tick armed
    /// for the first A names the instant asked now, yet only the tick armed
    /// last serves it, so one tick is live, and the window is asked once.
    /// Red-check: serve any tick whose instant is the one asked, and the first
    /// A's tick and the last A's both serve it.
    #[test]
    fn only_the_tick_armed_last_serves_an_instant_asked_again() {
        let mut frame = TimedFrame::default();
        let first_a = armed(&mut frame, 1000, 0);
        let _b = armed(&mut frame, 2000, 10);
        let last_a = armed(&mut frame, 1000, 20);
        assert_eq!(
            frame.tick(first_a, ms(1000)),
            TimedTick::Stale,
            "the first tick for the instant is not the live one"
        );
        assert_eq!(
            frame.tick(last_a, ms(1000)),
            TimedTick::Serve,
            "the tick armed last serves it"
        );
    }

    /// A served instant whose frame was held is asked for again (the runtime
    /// asks once its preparation's outcome moved on): it is armed again, past,
    /// and served at once, once. Red-check: treat an instant served before as
    /// the one the live tick serves, and the frame a hold kept from drawing is
    /// never asked for again.
    #[test]
    fn a_served_instant_asked_again_is_served_again() {
        let mut frame = TimedFrame::default();
        let tick = armed(&mut frame, 1000, 0);
        assert_eq!(frame.tick(tick, ms(1000)), TimedTick::Serve, "served");
        let again = frame.request(Some(ms(1000)), ms(1200));
        assert_eq!(
            again.map(|(_, delay)| delay),
            Some(Duration::ZERO),
            "asked again after its frame was held: armed at once"
        );
        let again = again.expect("armed").0;
        assert_eq!(
            frame.tick(again, ms(1200)),
            TimedTick::Serve,
            "served again"
        );
        assert_eq!(frame.tick(again, ms(1216)), TimedTick::Stale, "and once");
    }

    /// A covered window owing a frame at its instant, beside a shown one that
    /// draws a frame every 16 ms and whose every frame makes the loop look
    /// again (the wake pump's look after a window's run): however often the
    /// covered window's tick is run again after its instant, it is asked for
    /// its frame once, and the shown window, which asked for none, never.
    /// Red-check: leave the instant live once served, which asks the covered
    /// window again at the shown window's frame rate.
    #[test]
    fn a_shown_windows_frames_do_not_serve_a_covered_windows_request_again() {
        let mut covered = TimedFrame::default();
        let mut shown = TimedFrame::default();
        let owed = armed(&mut covered, 1000, 0);
        let (mut covered_served, mut shown_served) = (0_u32, 0_u32);
        for frame in 0..200_u64 {
            let now = ms(frame * 16);
            if shown.tick(owed, now) == TimedTick::Serve {
                shown_served += 1;
            }
            if now >= ms(1000) && covered.tick(owed, now) == TimedTick::Serve {
                covered_served += 1;
            }
        }
        assert_eq!(covered_served, 1, "the covered window is asked once");
        assert_eq!(
            shown_served, 0,
            "the shown window is asked by no timed request"
        );
    }
}

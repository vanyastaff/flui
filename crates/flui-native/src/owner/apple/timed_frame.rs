//! An AppKit window's timed frame request
//! (`PlatformWindow::request_frame_at`, ADR-0389): the instant the window was
//! asked for a frame at, which a tick on the owner queue serves for that window
//! alone, once. Pure, so every host runs its tests.
//!
//! The frame a covered window owes its surface reaches it this way, not through
//! the application's wake deadline: the wake pump asks every window for a frame
//! and arms the same instant again after a frame of any window, so a covered
//! window AppKit runs no display pass for would have its instant delivered
//! again at the frame rate of the windows beside it. Here a tick names the
//! instant it was scheduled for and serves it only while that is still the one
//! asked; a served instant is gone, and only the next request arms another.
//!
//! One tick is live at a time, the one armed for the instant asked now: a
//! request for the instant the live tick already serves arms none, and a tick
//! armed for an instant replaced or withdrawn since does nothing when it runs,
//! as `WaitEndRead` keeps a window's mode-wait read.

use std::time::Duration;

use flui_platform::EventTime;

/// The instant an AppKit window was asked for a frame at and has not been
/// served: the one its live tick is armed for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimedFrame {
    pending: Option<EventTime>,
}

/// What a tick scheduled for an instant does when it runs ([`TimedFrame::tick`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimedTick {
    /// The instant is still the one asked and has come: ask the window for a
    /// frame, once.
    Serve,
    /// The instant is still the one asked and has not come (the queue's timer
    /// ran early): run again after this long.
    Wait(Duration),
    /// The instant was served, withdrawn or replaced: nothing to do.
    Stale,
}

impl TimedFrame {
    /// The window was asked for a frame at `at`, or the instant asked before
    /// withdrawn (`None`), at `now`. Answers how long after `now` a tick for
    /// `at` is to run, when one is to be armed: `None` when the live tick
    /// already serves `at`, or nothing is asked. An instant served before is
    /// armed again: asking for it again means its frame is still owed.
    pub(crate) fn request(&mut self, at: Option<EventTime>, now: EventTime) -> Option<Duration> {
        if self.pending == at {
            return None;
        }
        self.pending = at;
        at.map(|at| at.saturating_duration_since(now))
    }

    /// A tick scheduled for `at` runs at `now`. It serves `at` only while that
    /// is the instant asked, and the instant is then gone, so a window is asked
    /// once for it however many ticks name it.
    pub(crate) fn tick(&mut self, at: EventTime, now: EventTime) -> TimedTick {
        if self.pending != Some(at) {
            return TimedTick::Stale;
        }
        if now < at {
            return TimedTick::Wait(at.saturating_duration_since(now));
        }
        self.pending = None;
        TimedTick::Serve
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform::EventTime;

    use super::{TimedFrame, TimedTick};

    fn ms(millis: u64) -> EventTime {
        EventTime::from_nanos(millis * 1_000_000)
    }

    /// A window's request is served by the tick scheduled for it, once, at its
    /// instant: a tick that runs early waits out the rest, and ticks for an
    /// instant that was served, withdrawn or replaced do nothing. A request
    /// for the instant the live tick serves arms no second tick. Red-check:
    /// serve every tick whose instant has come (the wake deadline's rule
    /// without its frame count), and the instant served is served again; or
    /// arm a tick for every request, and two ticks are queued for one instant.
    #[test]
    fn a_timed_request_is_served_once_at_its_instant() {
        let mut frame = TimedFrame::default();
        let delay = Some(Duration::from_millis(1000));
        assert_eq!(frame.request(Some(ms(1000)), ms(0)), delay, "armed");
        assert_eq!(
            frame.request(Some(ms(1000)), ms(10)),
            None,
            "the live tick serves the same instant: none more is armed"
        );
        // (case, the tick's instant, now, what it does)
        for (case, at, now, found) in [
            (
                "a tick that runs early waits",
                ms(1000),
                ms(990),
                TimedTick::Wait(Duration::from_millis(10)),
            ),
            (
                "served at its instant",
                ms(1000),
                ms(1000),
                TimedTick::Serve,
            ),
            ("and once", ms(1000), ms(1016), TimedTick::Stale),
        ] {
            assert_eq!(frame.tick(at, now), found, "{case}");
        }

        assert_eq!(
            frame.request(Some(ms(2000)), ms(1500)),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            frame.request(Some(ms(3000)), ms(1500)),
            Some(Duration::from_millis(1500)),
            "replaced"
        );
        assert_eq!(
            frame.tick(ms(2000), ms(2500)),
            TimedTick::Stale,
            "the replaced instant's tick"
        );
        assert_eq!(
            frame.request(None, ms(2500)),
            None,
            "withdrawn: nothing armed"
        );
        assert_eq!(
            frame.tick(ms(3000), ms(3000)),
            TimedTick::Stale,
            "the withdrawn instant's tick"
        );
    }

    /// A served instant whose frame was held is asked for again (the runtime
    /// asks once its hold may have ended): it is armed again, past, and served
    /// at once, once. Red-check: treat an instant served before as the one the
    /// live tick serves, and the frame a hold kept from drawing is never asked
    /// for again.
    #[test]
    fn a_served_instant_asked_again_is_served_again() {
        let mut frame = TimedFrame::default();
        let owed = ms(1000);
        assert!(frame.request(Some(owed), ms(0)).is_some());
        assert_eq!(frame.tick(owed, ms(1000)), TimedTick::Serve, "served");
        assert_eq!(
            frame.request(Some(owed), ms(1200)),
            Some(Duration::ZERO),
            "asked again after its frame was held: armed at once"
        );
        assert_eq!(frame.tick(owed, ms(1200)), TimedTick::Serve, "served again");
        assert_eq!(frame.tick(owed, ms(1216)), TimedTick::Stale, "and once");
    }

    /// A covered window owing a frame at its instant, beside a shown one that
    /// draws a frame every 16 ms and whose every frame makes the loop look
    /// again (the wake pump's look after a window's run): however often the
    /// covered window's tick is run again after its instant, it is asked for
    /// its frame once, and the shown window, which asked for none, never.
    /// Red-check: leave the instant pending once served, which asks the covered
    /// window again at the shown window's frame rate.
    #[test]
    fn a_shown_windows_frames_do_not_serve_a_covered_windows_request_again() {
        let mut covered = TimedFrame::default();
        let mut shown = TimedFrame::default();
        let owed = ms(1000);
        assert!(
            covered.request(Some(owed), ms(0)).is_some(),
            "the covered window's request is armed"
        );
        let (mut covered_served, mut shown_served) = (0_u32, 0_u32);
        for frame in 0..200_u64 {
            let now = ms(frame * 16);
            if shown.tick(owed, now) == TimedTick::Serve {
                shown_served += 1;
            }
            if now >= owed && covered.tick(owed, now) == TimedTick::Serve {
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

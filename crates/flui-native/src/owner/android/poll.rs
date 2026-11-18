//! How long one turn of the Android event loop may block in `poll_events`
//! (ADR-0249). Pure, so every host runs its tests.

#![forbid(unsafe_code)]

use std::time::Duration;

use web_time::Instant;

/// What one turn of the Android event loop knows when it chooses how long
/// `poll_events` may block.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopTurn {
    /// The turn has a frame to deliver: a redraw request or a due deadline,
    /// while running.
    pub(crate) should_render: bool,
    /// The turn has a frame to deliver at the choreographer's next frame,
    /// whose callback is posted and wakes the looper (ADR-0269).
    pub(crate) awaiting_vsync: bool,
    /// The Activity is on screen, paused or not (ADR-0366), so a frame can be
    /// delivered at all.
    pub(crate) running: bool,
    /// The earliest wall-clock deadline the wake hook reported.
    pub(crate) wake_deadline: Option<Instant>,
    /// When preferences, scroll factors and insets are read next
    /// ([`LoopReads::next`]); `None` while their changes are reported instead.
    pub(crate) next_preference_sample: Option<Instant>,
    /// When the oldest request for a result the Java main thread has not
    /// taken is due to end, running or not.
    pub(crate) hand_off_deadline: Option<Instant>,
    /// When the earliest intent still waiting for its documents is due to
    /// refuse the ones not opened, running or not.
    pub(crate) open_deadline: Option<Instant>,
}

/// The longest wait `ALooper_pollOnce` takes: its timeout is `i32` milliseconds,
/// and `android-activity` converts with `as i32`, so a longer one wraps negative
/// and waits forever.
const LOOPER_MAX_WAIT: Duration = Duration::from_millis(i32::MAX as u64);

/// How long `poll_events` may block; `None` blocks until the looper is woken.
///
/// Every producer of work wakes the looper (a window redraw request, the owner
/// signal, the Java view's event queue, the inset reply, the display watch), so
/// the loop wakes on no timer of its own:
///
/// - a turn with a frame to deliver does not block;
/// - a turn waiting for the choreographer's next frame blocks until it is
///   woken, or until a deadline or preference sample still ahead: a deadline
///   already behind is the frame it waits for;
/// - a running turn blocks until the next deadline or preference sample, and
///   with neither until it is woken;
/// - a stopped turn blocks until a deadline that is still ahead, and with none
///   until it is woken. A deadline already behind was offered to the owner
///   once and is left waiting for a frame, which nothing delivers before the
///   Activity starts again; preferences are read again on `Start`.
pub(crate) fn poll_timeout(turn: LoopTurn, now: Instant) -> Option<Duration> {
    if turn.should_render {
        return Some(Duration::ZERO);
    }
    let until = |deadline: Instant| deadline.saturating_duration_since(now).min(LOOPER_MAX_WAIT);
    let deadline = if turn.running && !turn.awaiting_vsync {
        turn.wake_deadline
    } else {
        turn.wake_deadline.filter(|deadline| *deadline > now)
    };
    let sample = turn.next_preference_sample.filter(|_| turn.running);
    [deadline, sample, turn.hand_off_deadline, turn.open_deadline]
        .into_iter()
        .flatten()
        .min()
        .map(until)
}

/// When the loop reads the preferences, scroll factors and insets on its own
/// rather than on a notice: on its first turn and after `Start` (a stopped
/// Activity reads nothing), and when the retry a failed read owes falls due.
///
/// Every other change reaches the loop as a notice: `FluiView` reports the
/// settings and the insets, once more as it enters the window after its
/// observers are registered, and the configuration arrives as
/// `ConfigChanged`. Nothing here repeats, so no cadence stands in for a notice
/// that has not come.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopReads {
    /// A read is owed at the loop's next turn.
    owed: bool,
}

/// A notice the loop hears that bears on the preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreferenceNotice {
    /// `ConfigChanged`.
    ConfigurationChanged,
    /// `FluiView` reported a setting or the contrast level changed.
    SettingsReported,
    /// The Activity started: nothing was read while it was stopped.
    Started,
}

impl LoopReads {
    /// The reads of a loop that has not read yet: its first turn reads.
    pub(crate) const fn first() -> Self {
        Self { owed: true }
    }

    /// What the loop does on `heard`: it tells the source of the notice
    /// (`notice`, which lets a source whose retries ran out read again), and
    /// it reads now (`read`) for a change it is told of, or at its next turn
    /// for a start, which runs before the Activity's frames do.
    pub(crate) fn hear(
        &mut self,
        heard: PreferenceNotice,
        notice: impl FnOnce(),
        read: impl FnOnce(),
    ) {
        notice();
        match heard {
            PreferenceNotice::ConfigurationChanged | PreferenceNotice::SettingsReported => read(),
            PreferenceNotice::Started => self.owed = true,
        }
    }

    /// When the loop reads next: now while a read is owed, else at `retry`,
    /// the source's retry deadline; `None` waits for a notice.
    pub(crate) fn next(self, retry: Option<Instant>, now: Instant) -> Option<Instant> {
        self.owed.then_some(now).into_iter().chain(retry).min()
    }

    /// Whether the loop reads at `now`; taking a read clears the owed one.
    pub(crate) fn take_due(&mut self, retry: Option<Instant>, now: Instant) -> bool {
        let due = self.next(retry, now).is_some_and(|at| now >= at);
        if due {
            self.owed = false;
        }
        due
    }
}

/// Which past deadline the owner was last offered.
///
/// The offer wakes the owner signal, and that wake also wakes the looper. Until
/// a frame clears the deadline it stays behind, so offering it on every turn
/// would wake the looper on every turn: a busy loop while the turn waits for
/// the choreographer's next frame or for the Activity to start again.
#[derive(Debug, Default)]
pub(crate) struct DueDeadline {
    offered: Option<Instant>,
}

impl DueDeadline {
    /// Whether `deadline` is behind `now` and has not been offered yet.
    /// A deadline the owner moves, or clears and arms again, is offered again.
    pub(crate) fn offer(&mut self, deadline: Option<Instant>, now: Instant) -> bool {
        let due = deadline.filter(|deadline| now >= *deadline);
        let fresh = due.is_some() && due != self.offered;
        self.offered = due;
        fresh
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A stopped Activity cannot deliver a frame, so the loop blocks: no timer
    /// of its own, no past deadline turning the poll into a busy loop, and only
    /// a deadline still ahead (a timer the owner serves without a frame) bounds
    /// the wait. A running loop whose changes are reported blocks the same way,
    /// and so does one waiting for the choreographer's next frame, whose
    /// callback wakes it.
    #[test]
    fn a_stopped_loop_blocks_until_it_is_woken() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let turn = |should_render, running, wake_deadline| LoopTurn {
            awaiting_vsync: false,
            should_render,
            running,
            wake_deadline,
            next_preference_sample: Some(now + ms(500)),
            hand_off_deadline: None,
            open_deadline: None,
        };
        let awaiting = |wake_deadline| LoopTurn {
            awaiting_vsync: true,
            ..turn(false, true, wake_deadline)
        };
        let reported = |wake_deadline| LoopTurn {
            awaiting_vsync: false,
            should_render: false,
            running: true,
            wake_deadline,
            next_preference_sample: None,
            hand_off_deadline: None,
            open_deadline: None,
        };
        for (case, turn, expected) in [
            (
                "a frame to deliver",
                turn(true, true, None),
                Some(Duration::ZERO),
            ),
            ("running and idle", turn(false, true, None), Some(ms(500))),
            (
                "waiting for the next frame, its deadline behind",
                awaiting(now.checked_sub(ms(40))),
                Some(ms(500)),
            ),
            (
                "waiting for the next frame, a timer ahead",
                awaiting(Some(now + ms(3))),
                Some(ms(3)),
            ),
            (
                "running and idle, with changes reported",
                reported(None),
                None,
            ),
            (
                "running, with changes reported, a timer ahead",
                reported(Some(now + ms(250))),
                Some(ms(250)),
            ),
            (
                "running, a deadline before the next sample",
                turn(false, true, Some(now + ms(3))),
                Some(ms(3)),
            ),
            (
                "running, a deadline after the next sample",
                turn(false, true, Some(now + ms(5_000))),
                Some(ms(500)),
            ),
            ("stopped and idle", turn(false, false, None), None),
            (
                "stopped, a request for a result due to end",
                LoopTurn {
                    hand_off_deadline: Some(now + ms(700)),
                    ..turn(false, false, None)
                },
                Some(ms(700)),
            ),
            (
                "stopped, an intent's documents due to be refused",
                LoopTurn {
                    open_deadline: Some(now + ms(900)),
                    ..turn(false, false, None)
                },
                Some(ms(900)),
            ),
            (
                "stopped, a deadline that needs a frame is already behind",
                turn(false, false, now.checked_sub(ms(40))),
                None,
            ),
            (
                "stopped, a deadline exactly due",
                turn(false, false, Some(now)),
                None,
            ),
            (
                "stopped, a timer still ahead",
                turn(false, false, Some(now + ms(250))),
                Some(ms(250)),
            ),
            (
                "stopped, a timer further ahead than the looper can wait",
                turn(false, false, Some(now + Duration::from_hours(30 * 24))),
                Some(LOOPER_MAX_WAIT),
            ),
        ] {
            assert_eq!(poll_timeout(turn, now), expected, "{case}");
        }
    }

    /// The loop reads on its own on its first turn, after `Start` and at a
    /// due retry, each once, and never again until one of them comes back: an
    /// hour idle after a read names no instant, so the poll blocks.
    #[test]
    fn the_loop_reads_on_its_own_only_at_start_and_at_a_due_retry() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut reads = LoopReads::first();
        assert!(reads.take_due(None, start), "the first turn reads");
        for second in 0..=3600 {
            assert_eq!(
                reads.next(None, at(second)),
                None,
                "a read owed at {second} s"
            );
            assert!(!reads.take_due(None, at(second)));
        }
        let retry = Some(at(3601));
        assert_eq!(reads.next(retry, at(3600)), retry);
        assert!(!reads.take_due(retry, at(3600)), "a retry ahead waits");
        assert!(reads.take_due(retry, at(3601)), "a retry due reads");
        assert!(!reads.take_due(None, at(3601)), "and once");
        reads.hear(PreferenceNotice::Started, || {}, || {});
        assert_eq!(reads.next(retry, at(3602)), Some(at(3601)));
        assert!(reads.take_due(None, at(3602)), "`Start` reads");
        assert!(!reads.take_due(None, at(3603)));
    }

    /// Every notice tells the source; a change the loop is told of is read
    /// at once, a start at the loop's next turn.
    #[test]
    fn each_preference_notice_tells_the_source_and_reads() {
        let now = Instant::now();
        for (heard, reads_now, owed_after) in [
            (PreferenceNotice::ConfigurationChanged, true, false),
            (PreferenceNotice::SettingsReported, true, false),
            (PreferenceNotice::Started, false, true),
        ] {
            let mut reads = LoopReads::first();
            assert!(reads.take_due(None, now));
            let (mut noticed, mut read) = (false, false);
            reads.hear(heard, || noticed = true, || read = true);
            assert!(noticed, "{heard:?}: the source is not told");
            assert_eq!(read, reads_now, "{heard:?}: read now");
            assert_eq!(
                reads.take_due(None, now),
                owed_after,
                "{heard:?}: read next turn"
            );
        }
    }

    /// A deadline left behind is offered once, however many turns pass before
    /// a frame clears it; a later deadline, or one cleared and armed again, is
    /// offered again.
    #[test]
    fn a_deadline_left_behind_is_offered_once() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let behind = now.checked_sub(ms(40));
        let mut due = DueDeadline::default();
        for (case, deadline, at, expected) in [
            ("nothing armed", None, now, false),
            ("a deadline ahead", Some(now + ms(5)), now, false),
            ("the deadline passes", Some(now + ms(5)), now + ms(5), true),
            (
                "the next turn, still behind",
                Some(now + ms(5)),
                now + ms(9),
                false,
            ),
            ("the owner moves it, and it is behind", behind, now, true),
            ("a turn waiting for the frame", behind, now, false),
            ("a frame clears it", None, now, false),
            ("armed again at the same instant", behind, now, true),
        ] {
            assert_eq!(due.offer(deadline, at), expected, "{case}");
        }
    }
}

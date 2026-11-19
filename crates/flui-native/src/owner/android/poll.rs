//! How long one turn of the Android event loop may block in `poll_events`
//! (ADR-0249), and when it delivers the application's wake deadline. Pure, so
//! every host runs its tests.

#![forbid(unsafe_code)]

use std::time::Duration;

use web_time::Instant;

use crate::owner::wake_deadline::WakeTurn;

/// What one turn of the Android event loop knows when it chooses how long
/// `poll_events` may block.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopTurn {
    /// The turn has a frame to deliver now: a redraw request, a due wake
    /// deadline's included, while running and not waiting for the
    /// choreographer's next frame, whose callback wakes the looper
    /// (ADR-0269).
    pub(crate) should_render: bool,
    /// The Activity is on screen, paused or not (ADR-0366), so a frame can be
    /// delivered at all.
    pub(crate) running: bool,
    /// What the turn's look at the wake deadline found
    /// (`WakeDeadline::turn`). A due one was delivered as a redraw request,
    /// which `should_render` carries while a frame can run, and bounds no
    /// wait: stopped, or waiting for the choreographer, the turn blocks.
    pub(crate) wake: WakeTurn,
    /// The turn deadline, still ahead when the turn took it.
    pub(crate) turn_deadline: Option<Instant>,
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
/// - a turn with a frame to deliver now does not block;
/// - a running turn, one waiting for the choreographer's next frame included,
///   blocks until the next deadline or preference sample, and with neither
///   until it is woken;
/// - a stopped turn blocks until the next deadline, and with none until it is
///   woken; preferences are read again on `Start`.
///
/// The wake deadline is the loop's look at it (`WakeDeadline::turn`): a due
/// instant was delivered as a redraw request, which waits for a turn that
/// can deliver it and bounds no wait, and it is not armed again until a frame
/// ran, so a deadline whose frames the handler declines, or that a stopped
/// Activity cannot draw, costs one delivery and no turn after it. A deadline
/// that came due between the turn's look and this wait ends the wait at
/// once, and the next turn delivers it.
pub(crate) fn poll_timeout(turn: LoopTurn, now: Instant) -> Option<Duration> {
    if turn.should_render {
        return Some(Duration::ZERO);
    }
    let until = |deadline: Instant| deadline.saturating_duration_since(now).min(LOOPER_MAX_WAIT);
    let sample = turn.next_preference_sample.filter(|_| turn.running);
    [
        turn.wake.wait(),
        turn.turn_deadline,
        sample,
        turn.hand_off_deadline,
        turn.open_deadline,
    ]
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
    /// (`notice`, which lets the next read ask the system), unless the view
    /// told it as it reported, and it reads now (`read`) for a change it is
    /// told of, or at its next turn for a start, which runs before the
    /// Activity's frames do.
    pub(crate) fn hear(
        &mut self,
        heard: PreferenceNotice,
        notice: impl FnOnce(),
        read: impl FnOnce(),
    ) {
        match heard {
            PreferenceNotice::ConfigurationChanged => {
                notice();
                read();
            }
            // The view told the source as it reported, before its wake, so
            // the turn that wake brought may have read already; a read here
            // then finds the answer current and asks the system nothing.
            PreferenceNotice::SettingsReported => read(),
            PreferenceNotice::Started => {
                notice();
                self.owed = true;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner::wake_deadline::WakeDeadline;

    /// A stopped Activity cannot deliver a frame, so the loop blocks: no timer
    /// of its own, and only a deadline still ahead (a timer the owner serves
    /// without a frame) bounds the wait. A running loop whose changes are
    /// reported blocks the same way, and so does one waiting for the
    /// choreographer's next frame, whose callback wakes it.
    #[test]
    fn a_stopped_loop_blocks_until_it_is_woken() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let turn = |should_render, running, wake| LoopTurn {
            should_render,
            running,
            wake,
            turn_deadline: None,
            next_preference_sample: Some(now + ms(500)),
            hand_off_deadline: None,
            open_deadline: None,
        };
        let reported = |wake| LoopTurn {
            should_render: false,
            running: true,
            wake,
            turn_deadline: None,
            next_preference_sample: None,
            hand_off_deadline: None,
            open_deadline: None,
        };
        let idle = WakeTurn::Idle;
        let wait = |after| WakeTurn::Wait(now + after);
        for (case, turn, expected) in [
            (
                "a frame to deliver",
                turn(true, true, idle),
                Some(Duration::ZERO),
            ),
            ("running and idle", turn(false, true, idle), Some(ms(500))),
            (
                "waiting for the next frame, its deadline delivered",
                turn(false, true, WakeTurn::Due(now)),
                Some(ms(500)),
            ),
            (
                "waiting for the next frame, a timer ahead",
                turn(false, true, wait(ms(3))),
                Some(ms(3)),
            ),
            (
                "running and idle, with changes reported",
                reported(idle),
                None,
            ),
            (
                "running, with changes reported, a timer ahead",
                reported(wait(ms(250))),
                Some(ms(250)),
            ),
            (
                "running, a deadline before the next sample",
                turn(false, true, wait(ms(3))),
                Some(ms(3)),
            ),
            (
                "running, a deadline after the next sample",
                turn(false, true, wait(ms(5_000))),
                Some(ms(500)),
            ),
            (
                "running, a turn deadline ahead",
                LoopTurn {
                    turn_deadline: Some(now + ms(40)),
                    ..turn(false, true, idle)
                },
                Some(ms(40)),
            ),
            ("stopped and idle", turn(false, false, idle), None),
            (
                "stopped, a request for a result due to end",
                LoopTurn {
                    hand_off_deadline: Some(now + ms(700)),
                    ..turn(false, false, idle)
                },
                Some(ms(700)),
            ),
            (
                "stopped, an intent's documents due to be refused",
                LoopTurn {
                    open_deadline: Some(now + ms(900)),
                    ..turn(false, false, idle)
                },
                Some(ms(900)),
            ),
            (
                "stopped, a deadline delivered as a frame nothing can draw",
                turn(false, false, WakeTurn::Due(now)),
                None,
            ),
            (
                "stopped, a deadline that came due since the turn took it",
                turn(false, false, wait(ms(0))),
                Some(Duration::ZERO),
            ),
            (
                "stopped, a timer still ahead",
                turn(false, false, wait(ms(250))),
                Some(ms(250)),
            ),
            (
                "stopped, a timer further ahead than the looper can wait",
                turn(false, false, wait(Duration::from_hours(30 * 24))),
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

    /// A configuration change and a start tell the source (a view report
    /// told it as the view reported); a change the loop is told of is read at
    /// once, a start at the loop's next turn.
    #[test]
    fn each_preference_notice_tells_the_source_and_reads() {
        let now = Instant::now();
        for (heard, tells, reads_now, owed_after) in [
            (PreferenceNotice::ConfigurationChanged, true, true, false),
            (PreferenceNotice::SettingsReported, false, true, false),
            (PreferenceNotice::Started, true, false, true),
        ] {
            let mut reads = LoopReads::first();
            assert!(reads.take_due(None, now));
            let (mut noticed, mut read) = (false, false);
            reads.hear(heard, || noticed = true, || read = true);
            assert_eq!(noticed, tells, "{heard:?}: the source is told");
            assert_eq!(read, reads_now, "{heard:?}: read now");
            assert_eq!(
                reads.take_due(None, now),
                owed_after,
                "{heard:?}: read next turn"
            );
        }
    }

    /// Turns of the loop over a past wake deadline, as the loop composes
    /// them: the look, the redraw request a delivery leaves (taken only by a
    /// turn that can draw), and the poll. Each frame the handler runs is
    /// declined, so the count stays. The poll timeout of each turn.
    fn turns_over_a_declined_deadline(running: bool) -> Vec<Option<Duration>> {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let behind = start + ms(5);
        let mut deadline = WakeDeadline::default();
        let mut redraw_requested = false;
        (0..4_u32)
            .map(|turn| {
                let now = behind + ms(16) * turn;
                let wake = deadline.turn(Some(behind), now, 0);
                if wake.is_due() {
                    redraw_requested = true;
                }
                let timeout = poll_timeout(
                    LoopTurn {
                        should_render: running && redraw_requested,
                        running,
                        wake,
                        turn_deadline: None,
                        next_preference_sample: None,
                        hand_off_deadline: None,
                        open_deadline: None,
                    },
                    now,
                );
                // A running turn takes the request and runs the frame, which
                // the handler declines; a stopped one keeps it.
                if running {
                    redraw_requested = false;
                }
                timeout
            })
            .collect()
    }

    /// A running loop whose handler declines the frame a past deadline asks
    /// for blocks after that one frame: the next turn arms nothing, has no
    /// redraw left, and waits for a wake. Red-check: deliver whenever the
    /// answer is not later than `now`, which kept the loop at a zero timeout.
    #[test]
    fn a_declined_frame_does_not_keep_the_loop_awake() {
        assert_eq!(
            turns_over_a_declined_deadline(true),
            [Some(Duration::ZERO), None, None, None],
            "one turn delivers the deadline, and the loop then blocks"
        );
    }

    /// A stopped Activity cannot draw the frame a past deadline asks for:
    /// the delivery waits as a redraw request for `Start`, and no turn polls
    /// at a zero timeout for it meanwhile. Red-check: let a delivered
    /// deadline bound the wait (the instant of `WakeTurn::Due`).
    #[test]
    fn a_deadline_delivered_while_stopped_blocks_the_loop() {
        assert_eq!(
            turns_over_a_declined_deadline(false),
            [None, None, None, None],
            "a stopped loop blocks from the delivery on"
        );
    }
}

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
    /// while resumed.
    pub(crate) should_render: bool,
    /// The turn has a frame to deliver at the choreographer's next frame,
    /// whose callback is posted and wakes the looper (ADR-0269).
    pub(crate) awaiting_vsync: bool,
    /// The Activity is resumed, so a frame can be delivered at all.
    pub(crate) resumed: bool,
    /// The earliest wall-clock deadline the wake hook reported.
    pub(crate) wake_deadline: Option<Instant>,
    /// When preferences, scroll factors and insets are read next; `None` while
    /// their changes are reported instead.
    pub(crate) next_preference_sample: Option<Instant>,
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
/// - a resumed turn blocks until the next deadline or preference sample, and
///   with neither until it is woken;
/// - a paused turn blocks until a deadline that is still ahead, and with none
///   until it is woken. A deadline already behind was offered to the owner this
///   turn and is left waiting for a frame, which nothing delivers before the
///   Activity resumes; preferences are read again on `Resume`.
pub(crate) fn poll_timeout(turn: LoopTurn, now: Instant) -> Option<Duration> {
    if turn.should_render {
        return Some(Duration::ZERO);
    }
    let until = |deadline: Instant| deadline.saturating_duration_since(now).min(LOOPER_MAX_WAIT);
    let deadline = if turn.resumed && !turn.awaiting_vsync {
        turn.wake_deadline
    } else {
        turn.wake_deadline.filter(|deadline| *deadline > now)
    };
    let sample = turn.next_preference_sample.filter(|_| turn.resumed);
    [deadline, sample].into_iter().flatten().min().map(until)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paused Activity cannot deliver a frame, so the loop blocks: no timer
    /// of its own, no past deadline turning the poll into a busy loop, and only
    /// a deadline still ahead (a timer the owner serves without a frame) bounds
    /// the wait. A resumed loop whose changes are reported blocks the same way,
    /// and so does one waiting for the choreographer's next frame, whose
    /// callback wakes it.
    #[test]
    fn a_paused_loop_blocks_until_it_is_woken() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let turn = |should_render, resumed, wake_deadline| LoopTurn {
            awaiting_vsync: false,
            should_render,
            resumed,
            wake_deadline,
            next_preference_sample: Some(now + ms(500)),
        };
        let awaiting = |wake_deadline| LoopTurn {
            awaiting_vsync: true,
            ..turn(false, true, wake_deadline)
        };
        let reported = |wake_deadline| LoopTurn {
            awaiting_vsync: false,
            should_render: false,
            resumed: true,
            wake_deadline,
            next_preference_sample: None,
        };
        for (case, turn, expected) in [
            (
                "a frame to deliver",
                turn(true, true, None),
                Some(Duration::ZERO),
            ),
            ("resumed and idle", turn(false, true, None), Some(ms(500))),
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
                "resumed and idle, with changes reported",
                reported(None),
                None,
            ),
            (
                "resumed, with changes reported, a timer ahead",
                reported(Some(now + ms(250))),
                Some(ms(250)),
            ),
            (
                "resumed, a deadline before the next sample",
                turn(false, true, Some(now + ms(3))),
                Some(ms(3)),
            ),
            (
                "resumed, a deadline after the next sample",
                turn(false, true, Some(now + ms(5_000))),
                Some(ms(500)),
            ),
            ("paused and idle", turn(false, false, None), None),
            (
                "paused, a deadline that needs a frame is already behind",
                turn(false, false, now.checked_sub(ms(40))),
                None,
            ),
            (
                "paused, a deadline exactly due",
                turn(false, false, Some(now)),
                None,
            ),
            (
                "paused, a timer still ahead",
                turn(false, false, Some(now + ms(250))),
                Some(ms(250)),
            ),
            (
                "paused, a timer further ahead than the looper can wait",
                turn(false, false, Some(now + Duration::from_hours(30 * 24))),
                Some(LOOPER_MAX_WAIT),
            ),
        ] {
            assert_eq!(poll_timeout(turn, now), expected, "{case}");
        }
    }
}

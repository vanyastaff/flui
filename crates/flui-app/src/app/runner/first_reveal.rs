//! When a freshly installed window is revealed.
//!
//! A backend that defers the physical reveal of a window opened visible
//! (`WindowOptions::visible`) waits for the embedder to say when — see
//! [`flui_platform::traits::PlatformWindow::reveal_after_first_frame`]. This
//! module is the embedder's half of that contract: the decision of *which*
//! frame outcome earns the reveal, kept pure so it is host-tested while the
//! desktop frame closure that consults it stays glue.

use std::time::Duration;

use web_time::Instant;

/// The reveal decision for one window: the first frame whose present
/// succeeded reveals it, and a frame that ran and presented nothing arms a
/// bounded fallback so a surface that never presents still yields a window
/// the user can see and close.
///
/// `Send + Sync` behind one `parking_lot::Mutex`: captured by the desktop
/// frame closure, whose registration requires `Send`, and consulted from the
/// wake-deadline hook on the same thread.
pub(super) struct FirstReveal {
    state: parking_lot::Mutex<State>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No frame outcome has been reported yet.
    Waiting,
    /// A frame ran and presented nothing at the recorded instant; reveal at
    /// `reveal_by` unless a present lands first.
    FallbackArmed { reveal_by: Instant },
    /// The reveal was handed to the window; every later report is ignored.
    Revealed,
}

impl FirstReveal {
    /// How long a window that ran a frame and presented nothing waits before
    /// it is revealed regardless. Measured from the first such outcome, not
    /// from install: a cold first frame that is still *inside* its render
    /// (shader compilation, the GPU stack's first submit) arms nothing, so
    /// the bound only fires for a surface that keeps producing nothing.
    pub(super) const FALLBACK: Duration = Duration::from_secs(1);

    pub(super) fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(State::Waiting),
        }
    }

    /// Record one frame outcome at `now`. Returns `true` exactly once, on
    /// the report that earns the reveal: the first presented frame, or the
    /// first non-presenting report at or past the armed fallback deadline.
    #[must_use]
    pub(super) fn after_frame(&self, presented: bool, now: Instant) -> bool {
        let mut state = self.state.lock();
        match *state {
            State::Revealed => false,
            State::Waiting | State::FallbackArmed { .. } if presented => {
                *state = State::Revealed;
                true
            }
            State::Waiting => {
                *state = State::FallbackArmed {
                    reveal_by: now + Self::FALLBACK,
                };
                false
            }
            State::FallbackArmed { reveal_by } => {
                if now >= reveal_by {
                    *state = State::Revealed;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// The armed fallback deadline, for the wake-deadline hook and the frame
    /// closure's `dirty` predicate — both, for the reason every wake-deadline
    /// source in this crate carries: a deadline the platform actuates but
    /// the predicate ignores reaches `WakeAction::Skip` before this policy is
    /// consulted. `None` while waiting for the first report, and after the
    /// reveal.
    pub(super) fn next_deadline(&self) -> Option<Instant> {
        match *self.state.lock() {
            State::FallbackArmed { reveal_by } => Some(reveal_by),
            State::Waiting | State::Revealed => None,
        }
    }
}

#[cfg(test)]
mod first_reveal_tests {
    use std::time::Duration;

    use web_time::Instant;

    use super::FirstReveal;

    #[test]
    fn the_first_presented_frame_reveals_exactly_once() {
        let reveal = FirstReveal::new();
        let now = Instant::now();
        assert!(
            reveal.next_deadline().is_none(),
            "nothing armed before a report"
        );
        assert!(reveal.after_frame(true, now), "the first present reveals");
        assert!(
            !reveal.after_frame(true, now),
            "a second present reveals nothing more"
        );
        assert!(
            reveal.next_deadline().is_none(),
            "nothing to wake for once revealed"
        );
    }

    #[test]
    fn a_non_presenting_frame_arms_the_fallback_and_a_present_before_it_wins() {
        let reveal = FirstReveal::new();
        let now = Instant::now();
        assert!(
            !reveal.after_frame(false, now),
            "presented nothing: not yet"
        );
        assert_eq!(
            reveal.next_deadline(),
            Some(now + FirstReveal::FALLBACK),
            "the fallback is armed one bound out from the FIRST empty outcome"
        );
        let later = now + Duration::from_millis(10);
        assert!(!reveal.after_frame(false, later), "still inside the bound");
        assert_eq!(
            reveal.next_deadline(),
            Some(now + FirstReveal::FALLBACK),
            "a second empty outcome does not push the deadline out"
        );
        assert!(
            reveal.after_frame(true, later),
            "a present inside the bound reveals"
        );
        assert!(reveal.next_deadline().is_none());
    }

    #[test]
    fn the_fallback_reveals_a_surface_that_never_presents() {
        let reveal = FirstReveal::new();
        let now = Instant::now();
        assert!(!reveal.after_frame(false, now));
        let deadline = reveal.next_deadline().expect("armed");
        let just_before = deadline
            .checked_sub(Duration::from_millis(1))
            .expect("a deadline armed from `now` lies well after the clock's epoch");
        assert!(
            !reveal.after_frame(false, just_before),
            "one tick before the bound: not yet"
        );
        assert!(
            reveal.after_frame(false, deadline),
            "at the bound, an empty outcome reveals anyway"
        );
        assert!(!reveal.after_frame(false, deadline), "exactly once");
        assert!(reveal.next_deadline().is_none());
    }
}

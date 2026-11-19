//! Who holds the pointer lock among a UIKit process's windows: the pure
//! rules the iOS host's one lock owner (`ios::pointer_lock`) follows, apart
//! from UIKit and GameController, so every host's tests run them.
//!
//! A window asks for the lock (its root controller then prefers it), and the
//! system answers per scene. The system locks one scene at a time; the
//! mouse's motion goes to the window whose scene it locked, and to no other.
//! A window lets go when the application asks, when it loses the focus, and
//! when it leaves its scene; once the system lets its lock go, it stops asking,
//! so nothing takes the pointer again unasked (ADR-0354 §2).

#![forbid(unsafe_code)]

use smallvec::SmallVec;

/// The lock as the owner keeps it, over windows named by `W`.
#[derive(Debug, Clone)]
pub(crate) struct LockLedger<W: Copy + Eq> {
    /// The windows whose controller prefers the lock now.
    asked: SmallVec<[W; 2]>,
    /// The window whose scene the system last said it locked.
    held_by: Option<W>,
}

impl<W: Copy + Eq> Default for LockLedger<W> {
    fn default() -> Self {
        Self {
            asked: SmallVec::new(),
            held_by: None,
        }
    }
}

/// What one step changed, for the owner to carry out on UIKit and
/// GameController after the ledger is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LockStep<W> {
    /// The window whose controller's preference changed: UIKit is asked to
    /// read it again.
    pub(crate) preference: Option<W>,
    /// The windows whose published grab changed, at most two (the window
    /// that took the lock and the one that lost it).
    pub(crate) published: [Option<W>; 2],
}

impl<W> LockStep<W> {
    const NOTHING: Self = Self {
        preference: None,
        published: [None, None],
    };
}

impl<W: Copy + Eq> LockLedger<W> {
    /// Whether `window`'s controller prefers the lock.
    pub(crate) fn prefers(&self, window: W) -> bool {
        self.asked.contains(&window)
    }

    /// Whether the system holds `window`'s scene's pointer locked, as it last
    /// said: what the window's snapshot publishes.
    pub(crate) fn holds(&self, window: W) -> bool {
        self.held_by == Some(window)
    }

    /// The window the locked mouse's motion goes to; `None` while the system
    /// holds no lock, when no motion goes anywhere.
    pub(crate) fn holder(&self) -> Option<W> {
        self.held_by
    }

    /// `window` asks for the lock: its controller prefers it from now on.
    pub(crate) fn ask(&mut self, window: W) -> LockStep<W> {
        if self.prefers(window) {
            return LockStep::NOTHING;
        }
        self.asked.push(window);
        LockStep {
            preference: Some(window),
            published: [None, None],
        }
    }

    /// `window` lets go: the application asked, it lost the focus or it left
    /// its scene. It stops preferring the lock, and a lock it held is no
    /// longer its own, whatever the system says later of a scene it left.
    pub(crate) fn withdraw(&mut self, window: W) -> LockStep<W> {
        let preference = self.forget_preference(window).then_some(window);
        let released = (self.held_by == Some(window)).then(|| {
            self.held_by = None;
            window
        });
        LockStep {
            preference,
            published: [released, None],
        }
    }

    /// The system said whether it holds `window`'s scene's pointer locked.
    /// A lock it grants moves the motion to `window`, from the window that
    /// held it before. A lock it lets go of is `window`'s to lose alone, and
    /// `window` stops preferring it; another window's lock stands. A grant
    /// to a window that no longer asks (it let go meanwhile) is not taken.
    pub(crate) fn reported(&mut self, window: W, locked: bool) -> LockStep<W> {
        if locked {
            if self.held_by == Some(window) || !self.prefers(window) {
                return LockStep::NOTHING;
            }
            let previous = self.held_by.replace(window);
            return LockStep {
                preference: None,
                published: [Some(window), previous],
            };
        }
        if self.held_by != Some(window) {
            return LockStep::NOTHING;
        }
        self.held_by = None;
        LockStep {
            preference: self.forget_preference(window).then_some(window),
            published: [Some(window), None],
        }
    }

    /// Take `window` out of the windows that prefer the lock; whether it was
    /// one.
    fn forget_preference(&mut self, window: W) -> bool {
        let before = self.asked.len();
        self.asked.retain(|asked| *asked != window);
        self.asked.len() != before
    }
}

/// The raw motion a locked mouse's `GCMouse` deltas carry: right and down
/// positive (GameController measures up as positive); `None` for a motion of
/// nothing, which delivers nothing.
pub(crate) fn locked_delta(dx: f32, dy: f32) -> Option<(f64, f64)> {
    if dx == 0.0 && dy == 0.0 {
        return None;
    }
    Some((f64::from(dx), -f64::from(dy)))
}

/// Whether a window of `own` size (width, height) fills a screen of `screen`
/// size, as a scene UIKit locks the pointer of does: not one beside another
/// app, over one, or in a Stage Manager frame. Both are points in the
/// interface's orientation; half a point allows for rounding.
pub(crate) fn fills_its_screen(own: (f64, f64), screen: (f64, f64)) -> bool {
    (own.0 - screen.0).abs() < 0.5 && (own.1 - screen.1).abs() < 0.5
}

#[cfg(test)]
mod tests {
    use super::{LockLedger, LockStep, fills_its_screen, locked_delta};

    const A: u8 = 1;
    const B: u8 = 2;

    fn published(step: LockStep<u8>) -> Vec<u8> {
        step.published.into_iter().flatten().collect()
    }

    /// A lock is the system's answer to a request: asking prefers it and
    /// holds nothing, the grant holds it for the window that asked, and the
    /// motion goes there alone.
    fn a_lock_is_held_from_the_systems_grant() {
        let mut ledger = LockLedger::default();
        assert_eq!(ledger.ask(A).preference, Some(A));
        assert_eq!(
            ledger.ask(A),
            LockStep::NOTHING,
            "asking twice changes nothing"
        );
        assert!(!ledger.holds(A) && ledger.holder().is_none());

        assert_eq!(published(ledger.reported(A, true)), [A]);
        assert!(ledger.holds(A));
        assert_eq!(ledger.holder(), Some(A));
    }

    /// The system letting a lock go ends the request too: the controller
    /// stops preferring it, so the scene coming back to full screen or the
    /// app becoming active again takes no pointer.
    fn a_lock_the_system_lets_go_is_not_asked_for_again() {
        let mut ledger = LockLedger::default();
        ledger.ask(A);
        ledger.reported(A, true);
        let step = ledger.reported(A, false);
        assert_eq!((step.preference, published(step)), (Some(A), vec![A]));
        assert!(!ledger.prefers(A) && !ledger.holds(A));
        assert_eq!(
            ledger.reported(A, true),
            LockStep::NOTHING,
            "an unasked grant is not taken"
        );
    }

    /// Leaving the scene, losing the focus or the application letting go
    /// drops the lock at once: the snapshot and the motion stop, whatever the
    /// system says later of a scene the window left.
    fn a_window_that_lets_go_holds_nothing() {
        let mut ledger = LockLedger::default();
        ledger.ask(A);
        ledger.reported(A, true);
        let step = ledger.withdraw(A);
        assert_eq!((step.preference, published(step)), (Some(A), vec![A]));
        assert!(ledger.holder().is_none());
        assert_eq!(ledger.reported(A, true), LockStep::NOTHING);
        assert_eq!(
            ledger.withdraw(A),
            LockStep::NOTHING,
            "letting go twice changes nothing"
        );
    }

    /// Two scenes: the system locks one at a time, a grant to the second
    /// moves the motion and unpublishes the first, and the first's later
    /// unlock leaves the second's lock standing.
    fn one_scenes_unlock_leaves_the_other_scenes_lock() {
        let mut ledger = LockLedger::default();
        ledger.ask(A);
        ledger.ask(B);
        ledger.reported(A, true);
        assert_eq!(published(ledger.reported(B, true)), [B, A]);
        assert_eq!(ledger.holder(), Some(B));

        assert_eq!(ledger.reported(A, false), LockStep::NOTHING);
        assert!(ledger.holds(B) && ledger.prefers(A));
        assert_eq!(ledger.withdraw(A).published, [None, None]);
        assert!(ledger.holds(B), "A letting go never clears B's lock");
    }

    /// GameController's deltas become FLUI's motion: down positive, nothing
    /// for a motion of nothing.
    fn the_locked_mouses_deltas_point_down_and_skip_nothing() {
        assert_eq!(locked_delta(3.0, 2.0), Some((3.0, -2.0)));
        assert_eq!(locked_delta(0.0, -1.5), Some((0.0, 1.5)));
        assert_eq!(locked_delta(0.0, 0.0), None);
    }

    /// Only a scene that fills its screen is a scene UIKit locks: Split View,
    /// Slide Over and a Stage Manager frame are refused at the command.
    fn only_a_full_screen_scene_is_locked() {
        let screen = (1180.0, 820.0);
        assert!(fills_its_screen((1180.0, 820.0), screen));
        assert!(fills_its_screen((1180.2, 819.8), screen), "rounding");
        assert!(!fills_its_screen((678.0, 820.0), screen), "Split View");
        assert!(
            !fills_its_screen((1180.0, 700.0), screen),
            "a Stage Manager frame"
        );
    }

    #[test]
    fn ios_pointer_lock_ledger() {
        crate::table_test::run_table(
            "ios_pointer_lock_ledger",
            &[
                (
                    "a_lock_is_held_from_the_systems_grant",
                    a_lock_is_held_from_the_systems_grant,
                ),
                (
                    "a_lock_the_system_lets_go_is_not_asked_for_again",
                    a_lock_the_system_lets_go_is_not_asked_for_again,
                ),
                (
                    "a_window_that_lets_go_holds_nothing",
                    a_window_that_lets_go_holds_nothing,
                ),
                (
                    "one_scenes_unlock_leaves_the_other_scenes_lock",
                    one_scenes_unlock_leaves_the_other_scenes_lock,
                ),
                (
                    "the_locked_mouses_deltas_point_down_and_skip_nothing",
                    the_locked_mouses_deltas_point_down_and_skip_nothing,
                ),
                (
                    "only_a_full_screen_scene_is_locked",
                    only_a_full_screen_scene_is_locked,
                ),
            ],
        );
    }
}

//! Who holds the pointer lock among a UIKit process's windows: the pure
//! rules the iOS host's one lock owner (`ios::pointer_lock`) follows, apart
//! from UIKit and GameController, so every host's tests run them.
//!
//! A window asks for the lock (its root controller then prefers it), and the
//! system answers per scene. The two are apart: an ask is the application's
//! and ends only when the application lets go, the window loses the focus or
//! leaves its scene (ADR-0354 §2); a hold is the system's, which its reports
//! change and nothing else. The system locks one scene at a time; the mouse's
//! motion goes to the window whose scene it locked, and to no other. A lock
//! the system lets go of while the window still asks (the scene left full
//! screen) is asked for again, so UIKit takes it when it can, without the
//! window ever having lost the focus.

#![forbid(unsafe_code)]

use smallvec::SmallVec;

/// One window that asks for the lock, and whether the system holds its
/// scene's pointer locked: a hold exists only on an ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ask<W> {
    window: W,
    held: bool,
}

/// The lock as the owner keeps it, over windows named by `W`: the windows
/// that ask, at most one of them held.
#[derive(Debug, Clone)]
pub(crate) struct LockLedger<W: Copy + Eq> {
    asks: SmallVec<[Ask<W>; 2]>,
}

impl<W: Copy + Eq> Default for LockLedger<W> {
    fn default() -> Self {
        Self {
            asks: SmallVec::new(),
        }
    }
}

/// What one step changed, for the owner to carry out on UIKit and
/// GameController after the ledger is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LockStep<W> {
    /// The window whose controller UIKit is to ask again for its
    /// preference: it changed, or the system let a lock go that the window
    /// still asks for.
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
        self.asks.iter().any(|ask| ask.window == window)
    }

    /// Whether the system holds `window`'s scene's pointer locked, as it last
    /// said: what the window's snapshot publishes.
    pub(crate) fn holds(&self, window: W) -> bool {
        self.asks.iter().any(|ask| ask.window == window && ask.held)
    }

    /// The window the locked mouse's motion goes to; `None` while the system
    /// holds no lock, when no motion goes anywhere.
    pub(crate) fn holder(&self) -> Option<W> {
        self.asks.iter().find(|ask| ask.held).map(|ask| ask.window)
    }

    /// `window` asks for the lock: its controller prefers it from now on.
    /// `locked_now` is whether the system holds the window's scene's pointer
    /// locked as it asks: a window that let go and asks again before UIKit
    /// read the preference in between finds the lock never let go, and UIKit
    /// tells nothing of it, so the window holds it at once.
    pub(crate) fn ask(&mut self, window: W, locked_now: bool) -> LockStep<W> {
        let preference = if self.prefers(window) {
            None
        } else {
            self.asks.push(Ask {
                window,
                held: false,
            });
            Some(window)
        };
        let granted = if locked_now {
            self.reported(window, true)
        } else {
            LockStep::NOTHING
        };
        LockStep {
            preference,
            published: granted.published,
        }
    }

    /// `window` lets go: the application asked, it lost the focus or it left
    /// its scene. It asks no more, and a lock it held is no longer its own,
    /// whatever the system says later of a scene it left.
    pub(crate) fn withdraw(&mut self, window: W) -> LockStep<W> {
        let Some(index) = self.asks.iter().position(|ask| ask.window == window) else {
            return LockStep::NOTHING;
        };
        let ask = self.asks.remove(index);
        LockStep {
            preference: Some(window),
            published: [ask.held.then_some(window), None],
        }
    }

    /// The system said whether it holds `window`'s scene's pointer locked; it
    /// changes the hold and never the ask. A lock it grants a window that
    /// asks moves the motion there, from the window that held it before; a
    /// grant to a window that does not ask (it let go meanwhile) is not
    /// taken. A lock it lets go of is `window`'s to lose alone, and the
    /// window, still asking, is asked about again.
    pub(crate) fn reported(&mut self, window: W, locked: bool) -> LockStep<W> {
        let Some(index) = self.asks.iter().position(|ask| ask.window == window) else {
            return LockStep::NOTHING;
        };
        if self.asks[index].held == locked {
            return LockStep::NOTHING;
        }
        if !locked {
            self.asks[index].held = false;
            return LockStep {
                preference: Some(window),
                published: [Some(window), None],
            };
        }
        let previous = self.holder();
        for ask in &mut self.asks {
            ask.held = ask.window == window;
        }
        LockStep {
            preference: None,
            published: [Some(window), previous],
        }
    }
}

/// The raw motion a locked mouse's `GCMouse` deltas carry, right and down
/// positive (GameController measures up as positive), or `None`: for a
/// motion of nothing, and for a motion reported off the main thread
/// (`on_main_thread` false). A mouse's `handlerQueue` is the application's
/// to set, to a background queue too; FLUI's windows and their lock owner
/// live on the main thread, so such a motion is dropped rather than
/// delivered from another thread, and the queue is not set for the
/// application.
pub(crate) fn routable_delta(on_main_thread: bool, dx: f32, dy: f32) -> Option<(f64, f64)> {
    if !on_main_thread || (dx == 0.0 && dy == 0.0) {
        return None;
    }
    Some((f64::from(dx), -f64::from(dy)))
}

/// The mice FLUI put its motion handler on, each with the handler it found
/// there (`H`), which the application's code may have set. A copy of the
/// application's handler is never destroyed here: its last release runs the
/// application's code, which the owner runs after it published, contained.
#[derive(Debug)]
pub(crate) struct HandlerSwaps<M, H> {
    swaps: Vec<(M, Option<H>)>,
}

impl<M, H> Default for HandlerSwaps<M, H> {
    fn default() -> Self {
        Self { swaps: Vec::new() }
    }
}

/// What a release of the handlers does: the handler to put back on each
/// mouse that still carries FLUI's, and every copy, put back or not, for the
/// owner to destroy once it is done.
#[derive(Debug)]
pub(crate) struct Restoration<M, H> {
    /// Each mouse that still carries FLUI's handler and the one FLUI found.
    pub(crate) put_back: Vec<(M, Option<H>)>,
    /// The mice whose handler was replaced since, with the copy FLUI kept.
    pub(crate) left: Vec<(M, Option<H>)>,
}

impl<M: PartialEq, H> HandlerSwaps<M, H> {
    /// FLUI put its handler on `mouse`, which had `previous`. A mouse FLUI
    /// already holds keeps the handler it found first, and `previous` is
    /// handed back.
    #[must_use = "a refused copy is destroyed by the owner, after it is done"]
    pub(crate) fn installed(&mut self, mouse: M, previous: Option<H>) -> Option<Option<H>> {
        if self.swaps.iter().any(|(known, _)| *known == mouse) {
            return Some(previous);
        }
        self.swaps.push((mouse, previous));
        None
    }

    /// `mouse` went: nothing is put back on it, and its copy is handed back.
    #[must_use = "the copy is destroyed by the owner, after it is done"]
    pub(crate) fn forget(&mut self, mouse: &M) -> Option<(M, Option<H>)> {
        let index = self.swaps.iter().position(|(known, _)| known == mouse)?;
        Some(self.swaps.remove(index))
    }

    /// Split the mice between those that still carry FLUI's handler
    /// (`still_ours`), which get back the one FLUI found, and those whose
    /// handler was replaced since, which keep it.
    pub(crate) fn restore(self, still_ours: impl Fn(&M) -> bool) -> Restoration<M, H> {
        let (put_back, left) = self
            .swaps
            .into_iter()
            .partition(|(mouse, _)| still_ours(mouse));
        Restoration { put_back, left }
    }
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
    use std::rc::Rc;

    use super::{HandlerSwaps, LockLedger, LockStep, fills_its_screen, routable_delta};

    const A: u8 = 1;
    const B: u8 = 2;

    fn published(step: LockStep<u8>) -> Vec<u8> {
        step.published.into_iter().flatten().collect()
    }

    /// A lock is the system's answer to an ask: asking prefers it and holds
    /// nothing, the grant holds it for the window that asked, and the motion
    /// goes there alone.
    fn a_lock_is_held_from_the_systems_grant() {
        let mut ledger = LockLedger::default();
        assert_eq!(ledger.ask(A, false).preference, Some(A));
        assert_eq!(
            ledger.ask(A, false),
            LockStep::NOTHING,
            "asking twice changes nothing"
        );
        assert!(!ledger.holds(A) && ledger.holder().is_none());
        assert_eq!(published(ledger.reported(A, true)), [A]);
        assert_eq!(ledger.holder(), Some(A));
    }

    /// The system letting a lock go changes the hold, not the ask: the
    /// window still asks and UIKit is asked about it again.
    fn a_lock_the_system_lets_go_is_asked_for_again() {
        let mut ledger = LockLedger::default();
        ledger.ask(A, false);
        ledger.reported(A, true);
        let step = ledger.reported(A, false);
        assert_eq!((step.preference, published(step)), (Some(A), vec![A]));
        assert!(ledger.prefers(A) && !ledger.holds(A));
        assert_eq!(published(ledger.reported(A, true)), [A], "taken again");
    }

    /// An ask that overlaps an unlock in flight is kept: the grant read as
    /// the window asks again, then the system's late unlock, leave the window
    /// asking, and the next grant holds it.
    fn an_ask_over_an_unlock_in_flight_is_kept() {
        let mut ledger = LockLedger::default();
        ledger.ask(A, false);
        ledger.reported(A, true);
        ledger.withdraw(A);
        assert_eq!(published(ledger.ask(A, true)), [A]);
        let unlocked = ledger.reported(A, false);
        assert_eq!(unlocked.preference, Some(A), "UIKit asked again");
        assert!(ledger.prefers(A), "the ask survives the system's unlock");
        assert_eq!(published(ledger.reported(A, true)), [A]);
    }

    /// Letting go ends the ask and the hold at once: the snapshot and the
    /// motion stop, and a grant for a window that no longer asks is not
    /// taken.
    fn a_window_that_lets_go_holds_nothing() {
        let mut ledger = LockLedger::default();
        ledger.ask(A, false);
        ledger.reported(A, true);
        let step = ledger.withdraw(A);
        assert_eq!((step.preference, published(step)), (Some(A), vec![A]));
        assert!(ledger.holder().is_none() && !ledger.prefers(A));
        assert_eq!(ledger.reported(A, true), LockStep::NOTHING);
        assert_eq!(ledger.withdraw(A), LockStep::NOTHING, "letting go twice");
    }

    /// Letting go and asking again before UIKit reads the preference in
    /// between leaves the system's lock in place with no notification: the
    /// scene's lock as the window asks holds it again at once.
    fn asking_again_finds_a_lock_the_system_never_let_go() {
        let mut ledger = LockLedger::default();
        ledger.ask(A, false);
        ledger.reported(A, true);
        ledger.withdraw(A);
        let step = ledger.ask(A, true);
        assert_eq!((step.preference, published(step)), (Some(A), vec![A]));
        assert_eq!(ledger.holder(), Some(A));
    }

    /// Two scenes: the system locks one at a time, a grant to the second
    /// moves the motion and unpublishes the first, and the first's later
    /// unlock or release leaves the second's lock standing.
    fn one_scenes_unlock_leaves_the_other_scenes_lock() {
        let mut ledger = LockLedger::default();
        ledger.ask(A, false);
        ledger.ask(B, false);
        ledger.reported(A, true);
        assert_eq!(published(ledger.reported(B, true)), [B, A]);
        assert_eq!(ledger.holder(), Some(B));
        assert_eq!(ledger.reported(A, false), LockStep::NOTHING);
        assert_eq!(ledger.withdraw(A).published, [None, None]);
        assert!(ledger.holds(B), "A letting go never clears B's lock");
    }

    /// GameController's deltas become FLUI's motion: down positive, nothing
    /// for a motion of nothing, and nothing off the main thread.
    fn the_locked_mouses_deltas_point_down_and_stay_on_the_main_thread() {
        assert_eq!(routable_delta(true, 3.0, 2.0), Some((3.0, -2.0)));
        assert_eq!(routable_delta(true, 0.0, -1.5), Some((0.0, 1.5)));
        assert_eq!(routable_delta(true, 0.0, 0.0), None);
        assert_eq!(routable_delta(false, 3.0, 2.0), None, "a background queue");
    }

    /// FLUI puts back the handler it found on each mouse that still carries
    /// its own, leaves a handler set since alone, and destroys no copy of the
    /// application's handler itself: every one comes back to the owner.
    fn the_handler_flui_replaced_is_put_back_and_no_copy_is_destroyed() {
        let app = Rc::new(());
        let mut swaps = HandlerSwaps::default();
        assert!(swaps.installed("trackpad", Some(Rc::clone(&app))).is_none());
        assert!(swaps.installed("mouse", Some(Rc::clone(&app))).is_none());
        let refused = swaps.installed("trackpad", Some(Rc::clone(&app)));
        let gone = swaps.forget(&"mouse");
        assert!(swaps.installed("pen", Some(Rc::clone(&app))).is_none());
        let restoration = swaps.restore(|mouse| *mouse == "trackpad");
        assert_eq!(Rc::strong_count(&app), 5, "every copy is handed back");
        let put_back: Vec<_> = restoration
            .put_back
            .iter()
            .map(|(mouse, _)| *mouse)
            .collect();
        let left: Vec<_> = restoration.left.iter().map(|(mouse, _)| *mouse).collect();
        assert_eq!((put_back, left), (vec!["trackpad"], vec!["pen"]));
        drop((refused, gone, restoration));
        assert_eq!(Rc::strong_count(&app), 1);
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
                    "a_lock_the_system_lets_go_is_asked_for_again",
                    a_lock_the_system_lets_go_is_asked_for_again,
                ),
                (
                    "an_ask_over_an_unlock_in_flight_is_kept",
                    an_ask_over_an_unlock_in_flight_is_kept,
                ),
                (
                    "a_window_that_lets_go_holds_nothing",
                    a_window_that_lets_go_holds_nothing,
                ),
                (
                    "asking_again_finds_a_lock_the_system_never_let_go",
                    asking_again_finds_a_lock_the_system_never_let_go,
                ),
                (
                    "one_scenes_unlock_leaves_the_other_scenes_lock",
                    one_scenes_unlock_leaves_the_other_scenes_lock,
                ),
                (
                    "the_locked_mouses_deltas_point_down_and_stay_on_the_main_thread",
                    the_locked_mouses_deltas_point_down_and_stay_on_the_main_thread,
                ),
                (
                    "the_handler_flui_replaced_is_put_back_and_no_copy_is_destroyed",
                    the_handler_flui_replaced_is_put_back_and_no_copy_is_destroyed,
                ),
                (
                    "only_a_full_screen_scene_is_locked",
                    only_a_full_screen_scene_is_locked,
                ),
            ],
        );
    }
}

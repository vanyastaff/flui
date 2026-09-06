//! Base traits and types for gesture recognizers
//!
//! Defines the core `GestureRecognizer` trait and common types used by all
//! recognizers.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;
use tracing::instrument;

use crate::{
    arena::{GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition},
    ids::PointerId,
    routing::PointerDispatch,
};

/// Base trait for all gesture recognizers
///
/// A gesture recognizer detects specific gestures (tap, drag, scale, etc.) from
/// pointer events and calls user-provided callbacks.
///
/// # Lifecycle
///
/// 1. `add_pointer()` - Recognizer starts tracking a new pointer
/// 2. `handle_event()` - Process pointer events (move, up, cancel)
/// 3. Win/lose in gesture arena
/// 4. Call user callbacks on successful recognition
/// 5. `dispose()` - Clean up when done
pub trait GestureRecognizer: GestureArenaMember {
    /// Add a new pointer to track
    ///
    /// Called when a pointer goes down. The recognizer should add itself to the
    /// gesture arena if it wants to compete for this pointer.
    ///
    /// The receiver is the owning [`Arc`] so the exact recognizer identity is
    /// registered in the arena. Manufacturing an `Arc` from a cloned struct
    /// creates a different allocation, makes weak entry handles go stale as
    /// soon as the arena resolves, and cannot support post-resolution timers.
    /// `position` is in the recognizer's own space — the one its slop,
    /// velocity and axis maths are measured in. `global_position` is the same
    /// contact in the root's space, carried alongside because a recognizer
    /// cannot recover it: the event it is handed has already been localised.
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<Pixels>,
        global_position: Offset<Pixels>,
    );

    /// Handle a pointer event.
    ///
    /// Process move, up, and cancel events for tracked pointers.
    ///
    /// Takes the whole [`PointerDispatch`] rather than one event. Dispatch
    /// localises the event for the receiving target and keeps the untransformed
    /// one beside it; handing a recognizer only the local half is what made
    /// every detail struct's `global_position` a local position under a new
    /// name (issue #908). Everything a recognizer measures against its own box
    /// comes from `dispatch.local`; `dispatch.global` exists to be reported,
    /// not measured with.
    fn handle_event(&self, dispatch: PointerDispatch<'_>);

    /// Dispose of this recognizer
    ///
    /// Clean up resources and clear callbacks. Called when the recognizer is
    /// no longer needed.
    fn dispose(&self);

    /// Get the primary pointer being tracked (if any)
    fn primary_pointer(&self) -> Option<PointerId>;
}

/// Base composition data for gesture recognizers.
///
/// Provides common functionality that all recognizers need:
/// - Arena membership
/// - Primary pointer tracking
/// - Initial position tracking
/// - Disposal
///
/// Renamed from `GestureRecognizerState` to free that name for the
/// canonical Flutter `GestureRecognizerState` FSM enum (see below).
#[derive(Clone)]
pub struct RecognizerBase {
    /// Gesture arena for conflict resolution
    arena: GestureArena,

    /// Primary pointer ID being tracked, as a raw `u64` (`0` == none).
    ///
    /// Read on every event (the per-pointer filter), so it is a lock-free
    /// `AtomicU64` rather than `Mutex<Option<PointerId>>`. `PointerId` is
    /// `NonZeroU64`-backed, so `0` is an unambiguous "none" sentinel.
    primary_pointer: Arc<AtomicU64>,

    /// Where the primary pointer went down, in BOTH spaces.
    ///
    /// One field rather than two, because the two must agree about which
    /// contact they describe and a caller that could set them apart would
    /// eventually do so. That is not hypothetical: the cancel paths fall back
    /// to the stored LOCAL position because a `Cancel` event carries none, and
    /// the global half was left reading the event's own `Offset::ZERO` until
    /// they became one value.
    initial_contact: Arc<Mutex<Option<InitialContact>>>,

    /// Whether recognizer has been disposed. Checked on every event via
    /// `assert_not_disposed`, so a lock-free `AtomicBool`.
    disposed: Arc<AtomicBool>,

    /// Stale-safe handle to the exact member and arena generation registered
    /// by [`start_tracking`](Self::start_tracking). The handle stores weak
    /// identities, so retaining it here cannot form a recognizer cycle.
    tracked_entry: Arc<Mutex<Option<GestureArenaEntry>>>,
}

/// Where a gesture's primary pointer went down, in both coordinate spaces.
///
/// Recorded as one value by [`RecognizerBase::start_tracking`] and never
/// written apart, so the two halves always describe the same contact.
#[derive(Debug, Clone, Copy)]
struct InitialContact {
    /// The receiving node's space — what a `Move`/`Up` event carries after
    /// hit-test dispatch has rewritten it.
    local: Offset<Pixels>,
    /// The root's space — what dispatch rewrote away.
    global: Offset<Pixels>,
}

impl RecognizerBase {
    /// Create new recognizer base data with arena
    pub fn new(arena: GestureArena) -> Self {
        Self {
            arena,
            primary_pointer: Arc::new(AtomicU64::new(0)),
            initial_contact: Arc::new(Mutex::new(None)),
            disposed: Arc::new(AtomicBool::new(false)),
            tracked_entry: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the gesture arena
    #[inline]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// The current instant on the arena's clock — the time a deadline-driven
    /// recognizer compares its captured down-time against. Reads the OS clock in
    /// production; a headless frame driver's virtual clock in tests, so a
    /// deadline elapses deterministically without a wall-clock sleep.
    #[inline]
    pub fn now(&self) -> web_time::Instant {
        self.arena.now()
    }

    /// Get the primary pointer ID (if tracking one)
    #[inline]
    pub fn primary_pointer(&self) -> Option<PointerId> {
        // `PointerId::new` is `0 -> None`, so it round-trips the sentinel.
        PointerId::new(self.primary_pointer.load(Ordering::Relaxed))
    }

    /// Set the primary pointer
    pub fn set_primary_pointer(&self, pointer: Option<PointerId>) {
        let raw = pointer.map_or(0, |id| id.get_inner().get());
        self.primary_pointer.store(raw, Ordering::Relaxed);
    }

    /// Where the primary pointer went down, in the receiving node's space.
    #[inline]
    pub fn initial_position(&self) -> Option<Offset<Pixels>> {
        self.initial_contact.lock().map(|c| c.local)
    }

    /// Where the primary pointer went down, in the root's space.
    ///
    /// The half a `Cancel` cannot supply: dispatch rewrites an event into the
    /// receiving node's coordinates, and a cancel carries no position at all,
    /// so a recogniser reporting a cancelled gesture has nowhere else to read
    /// an untransformed position from.
    #[inline]
    pub fn initial_global_position(&self) -> Option<Offset<Pixels>> {
        self.initial_contact.lock().map(|c| c.global)
    }

    /// Forget the recorded contact. Set only by
    /// [`start_tracking`](Self::start_tracking), so the pair cannot drift.
    pub fn clear_initial_contact(&self) {
        *self.initial_contact.lock() = None;
    }

    /// Check if recognizer has been disposed
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Relaxed)
    }

    /// Mark as disposed
    pub fn mark_disposed(&self) {
        self.disposed.store(true, Ordering::Relaxed);
    }

    /// Debug-assert that the recognizer has not been disposed.
    ///
    /// Adopts the `ChangeNotifier::dispose` lifecycle pattern at
    /// [`crates/flui-foundation/src/notifier.rs`](../../crates/flui-foundation/src/notifier.rs)
    /// — use-after-dispose triggers a `debug_assert!` panic in debug
    /// builds + `tracing::warn!` + no-op semantics in release.
    ///
    /// Returns `true` if the recognizer is still live (call sites should
    /// proceed); `false` if disposed (call sites should early-return).
    #[instrument(
        name = "recognizer.assert_not_disposed",
        level = "trace",
        skip(self),
        fields(op = %op, primary = ?self.primary_pointer())
    )]
    #[inline]
    pub fn assert_not_disposed(&self, op: &'static str) -> bool {
        if self.is_disposed() {
            debug_assert!(
                false,
                "Recognizer::{op} called after dispose (use-after-dispose)"
            );
            tracing::warn!(op, "Recognizer used after dispose");
            return false;
        }
        true
    }

    /// Start tracking a pointer
    ///
    /// Sets this as the primary pointer and stores initial position.
    /// Adds recognizer to gesture arena.
    #[instrument(
        name = "recognizer.start_tracking",
        level = "debug",
        skip(self, recognizer),
        fields(
            pointer = ?pointer,
            position = ?position,
            event = %crate::observability::GestureEvent::StartedTracking,
        )
    )]
    pub fn start_tracking<T: GestureArenaMember + Clone + 'static>(
        &self,
        pointer: PointerId,
        position: Offset<Pixels>,
        global_position: Offset<Pixels>,
        recognizer: &Arc<T>,
    ) {
        if self.is_disposed() {
            return;
        }

        self.set_primary_pointer(Some(pointer));
        *self.initial_contact.lock() = Some(InitialContact {
            local: position,
            global: global_position,
        });

        // Register with the arena and retain the exact slot/member identity.
        let member: Arc<dyn GestureArenaMember> = recognizer.clone();
        let entry = self.arena.add(pointer, member);
        *self.tracked_entry.lock() = Some(entry);
    }

    /// Claim the arena win for the currently-tracked pointer.
    ///
    /// Resolves the arena in favour of this recognizer using the stable member
    /// identity captured in [`start_tracking`](Self::start_tracking), so
    /// competing members receive `reject_gesture`. No-op when not tracking a
    /// pointer or when the arena entry is already resolved or gone.
    pub fn accept_tracked(&self) {
        if let Some(entry) = self.tracked_entry.lock().clone() {
            entry.resolve(GestureDisposition::Accepted);
        }
    }

    /// The exact `Arc<dyn GestureArenaMember>` this recognizer registered with
    /// the arena in [`start_tracking`](Self::start_tracking), upgraded from the
    /// stored entry handle.
    ///
    /// Returns `None` once the recognizer is no longer tracking (the member was
    /// dropped). Used by the double-tap lifecycle to resolve the first contact's
    /// entry in favour of the double-tap.
    #[inline]
    pub fn tracked_member(&self) -> Option<Arc<dyn GestureArenaMember>> {
        self.tracked_entry
            .lock()
            .as_ref()
            .and_then(GestureArenaEntry::member)
    }

    /// The exact stale-safe entry registered for the tracked pointer.
    pub fn tracked_entry(&self) -> Option<GestureArenaEntry> {
        self.tracked_entry.lock().clone()
    }

    /// Stop tracking (called on success or rejection)
    #[instrument(
        name = "recognizer.stop_tracking",
        level = "debug",
        skip(self),
        fields(
            pointer = ?self.primary_pointer(),
            event = %crate::observability::GestureEvent::StoppedTracking,
        )
    )]
    pub fn stop_tracking(&self) {
        // Sweep only when this recognizer owns the arena lifecycle. In a
        // binding-driven arena the binding sweeps on `PointerUp` after routing
        // the event to the whole hit-test path; a recognizer self-sweeping here
        // would force-resolve a shared entry to the front member before a
        // double-tap (or a peer detector) could complete. Local tracking is
        // cleared in both modes.
        if self.primary_pointer().is_some()
            && self.arena.sweep_model() == crate::arena::SweepModel::SelfDriven
            && let Some(entry) = self.tracked_entry.lock().clone()
        {
            entry.sweep();
        }
        self.set_primary_pointer(None);
        self.clear_initial_contact();
        self.tracked_entry.lock().take();
    }

    /// Reject this gesture (lose the arena or explicit rejection)
    #[instrument(
        name = "recognizer.reject",
        level = "debug",
        skip(self),
        fields(
            pointer = ?self.primary_pointer(),
            event = %crate::observability::GestureEvent::ArenaRejected,
        )
    )]
    pub fn reject(&self) {
        if self.primary_pointer().is_none() {
            return;
        }
        // Withdraw ONLY this recognizer from the arena, using the stable member
        // identity captured in `start_tracking`. Resolving the whole entry with
        // no winner (the previous behavior) rejected every *competing* member
        // too, so a recognizer that bowed out of a shared arena (e.g. a tap
        // exceeding its slop) silently killed the drag it was competing with.
        let entry = self.tracked_entry.lock().take();
        // Clear ONLY this recognizer's local tracking — do NOT `stop_tracking`
        // (it would `sweep`, force-resolving any still-open competition in
        // first-member-wins fashion mid-gesture). Tear the shared entry down
        // only once it has actually settled (a winner emerged, or no members
        // remain); a competition that still has rivals must keep running until
        // one accepts or the pointer lifts.
        self.set_primary_pointer(None);
        self.clear_initial_contact();
        if let Some(entry) = entry {
            entry.resolve(GestureDisposition::Rejected);
        }
    }
}

impl std::fmt::Debug for RecognizerBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecognizerBase")
            .field("primary_pointer", &self.primary_pointer())
            .field("initial_position", &self.initial_position())
            .field("disposed", &self.is_disposed())
            .finish()
    }
}

/// Canonical gesture recognizer FSM state, matching Flutter
/// [`recognizer.dart:585`](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/recognizer.dart)
/// `GestureRecognizerState` enum.
///
/// A recognizer cycles `Ready` → `Possible` → (`Ready` | `Defunct`) and stays
/// in `Defunct` until all tracked pointers are removed, at which point it
/// returns to `Ready` for the next sequence.
///
/// Concrete recognizers typically retain a richer private FSM (e.g. Tap's
/// down-then-up timing, DoubleTap's between-tap window) but expose progress
/// through this canonical 3-state enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GestureRecognizerState {
    /// The recognizer is ready to start recognizing a gesture.
    #[default]
    Ready,

    /// The sequence of pointer events seen thus far is consistent with the
    /// gesture this recognizer is attempting to recognize but the gesture has
    /// not been accepted definitively.
    Possible,

    /// Further pointer events cannot cause this recognizer to recognize the
    /// gesture until the recognizer returns to [`Self::Ready`] (typically when all
    /// tracked pointers are removed).
    Defunct,
}

/// Constants for gesture recognition
pub mod constants {
    /// Maximum distance in pixels a pointer can move before it's no longer
    /// considered a tap
    pub const TAP_SLOP: f64 = 18.0;

    /// Maximum distance between two taps for double-tap
    pub const DOUBLE_TAP_SLOP: f64 = 100.0;

    /// Maximum time between taps for double-tap (milliseconds)
    pub const DOUBLE_TAP_TIMEOUT_MS: u64 = 300;

    /// Minimum duration for long press (milliseconds)
    pub const LONG_PRESS_DURATION_MS: u64 = 500;

    /// Minimum distance to start a drag
    pub const DRAG_SLOP: f64 = 18.0;

    /// Minimum distance to start a pan
    pub const PAN_SLOP: f64 = 18.0;

    /// Minimum distance between two pointers to start scale
    pub const SCALE_SLOP: f64 = 18.0;

    /// Minimum velocity to trigger fling (pixels per second)
    pub const MIN_FLING_VELOCITY: f64 = 50.0;

    /// Minimum distance for fling
    pub const MIN_FLING_DISTANCE: f64 = 50.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The minimum an arena entry needs: `start_tracking` registers a member,
    /// and nothing in these tests resolves the competition.
    #[derive(Clone)]
    struct TestMember;

    impl crate::sealed::arena_member::Sealed for TestMember {}

    impl GestureArenaMember for TestMember {
        fn accept_gesture(&self, _pointer: PointerId) {}
        fn reject_gesture(&self, _pointer: PointerId) {}
    }

    #[test]
    fn test_recognizer_base_creation() {
        let arena = GestureArena::new();
        let base = RecognizerBase::new(arena);

        assert_eq!(base.primary_pointer(), None);
        assert_eq!(base.initial_position(), None);
        assert!(!base.is_disposed());
    }

    #[test]
    fn test_recognizer_base_tracking() {
        let arena = GestureArena::new();
        let base = RecognizerBase::new(arena);

        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let position = Offset::new(Pixels(100.0), Pixels(200.0));
        // Deliberately different, so a test that confused the two spaces would
        // read the wrong number rather than the right one by coincidence.
        let global_position = Offset::new(Pixels(250.0), Pixels(350.0));
        let member = Arc::new(TestMember);

        base.start_tracking(pointer, position, global_position, &member);

        assert_eq!(base.primary_pointer(), Some(pointer));
        assert_eq!(base.initial_position(), Some(position));
        assert_eq!(base.initial_global_position(), Some(global_position));

        base.stop_tracking();
        assert_eq!(base.primary_pointer(), None);
        assert_eq!(base.initial_position(), None);
        assert_eq!(
            base.initial_global_position(),
            None,
            "the two halves are one stored contact, so clearing must take both"
        );
    }

    #[test]
    fn test_gesture_recognizer_state_enum() {
        let state = GestureRecognizerState::Ready;
        assert_eq!(state, GestureRecognizerState::Ready);

        let state = GestureRecognizerState::Possible;
        assert_ne!(state, GestureRecognizerState::Ready);

        let state = GestureRecognizerState::Defunct;
        assert_ne!(state, GestureRecognizerState::Possible);
    }

    #[test]
    fn test_constants() {
        const { assert!(constants::TAP_SLOP > 0.0) };
        const { assert!(constants::DRAG_SLOP > 0.0) };
        const { assert!(constants::MIN_FLING_VELOCITY > 0.0) };
    }
}

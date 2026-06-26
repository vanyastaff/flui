//! Base traits and types for gesture recognizers
//!
//! Defines the core `GestureRecognizer` trait and common types used by all
//! recognizers.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;
use tracing::instrument;

use crate::{
    arena::{GestureArena, GestureArenaMember},
    events::PointerEvent,
    ids::PointerId,
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
pub trait GestureRecognizer: GestureArenaMember + Send + Sync {
    /// Add a new pointer to track
    ///
    /// Called when a pointer goes down. The recognizer should add itself to the
    /// gesture arena if it wants to compete for this pointer.
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>);

    /// Handle a pointer event
    ///
    /// Process move, up, and cancel events for tracked pointers.
    fn handle_event(&self, event: &PointerEvent);

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

    /// Initial position of primary pointer
    initial_position: Arc<Mutex<Option<Offset<Pixels>>>>,

    /// Whether recognizer has been disposed. Checked on every event via
    /// `assert_not_disposed`, so a lock-free `AtomicBool`.
    disposed: Arc<AtomicBool>,

    /// Weak handle to the exact `Arc<dyn GestureArenaMember>` this recognizer
    /// registered with the arena in [`start_tracking`](Self::start_tracking).
    ///
    /// The arena identifies winners by `Arc::ptr_eq`, so claiming a win
    /// requires resolving with the *same* allocation that was added — not a
    /// fresh `Arc::new(self.clone())`. A `Weak` (not `Arc`) avoids a
    /// self-referential cycle that would leak the recognizer.
    tracked_member: Arc<Mutex<Option<Weak<dyn GestureArenaMember>>>>,
}

impl RecognizerBase {
    /// Create new recognizer base data with arena
    pub fn new(arena: GestureArena) -> Self {
        Self {
            arena,
            primary_pointer: Arc::new(AtomicU64::new(0)),
            initial_position: Arc::new(Mutex::new(None)),
            disposed: Arc::new(AtomicBool::new(false)),
            tracked_member: Arc::new(Mutex::new(None)),
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
    pub fn now(&self) -> std::time::Instant {
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

    /// Get the initial position of the primary pointer
    #[inline]
    pub fn initial_position(&self) -> Option<Offset<Pixels>> {
        *self.initial_position.lock()
    }

    /// Set the initial position
    pub fn set_initial_position(&self, position: Option<Offset<Pixels>>) {
        *self.initial_position.lock() = position;
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
        recognizer: &Arc<T>,
    ) {
        if self.is_disposed() {
            return;
        }

        self.set_primary_pointer(Some(pointer));
        self.set_initial_position(Some(position));

        // Register with the arena and remember this exact allocation (as a
        // `Weak`) so a later `accept_tracked()` can resolve with the same
        // `Arc` identity the arena matches on via `Arc::ptr_eq`.
        let member: Arc<dyn GestureArenaMember> = recognizer.clone();
        *self.tracked_member.lock() = Some(Arc::downgrade(&member));
        self.arena.add(pointer, member);
    }

    /// Claim the arena win for the currently-tracked pointer.
    ///
    /// Resolves the arena in favour of this recognizer using the stable member
    /// identity captured in [`start_tracking`](Self::start_tracking), so
    /// competing members receive `reject_gesture`. No-op when not tracking a
    /// pointer or when the arena entry is already resolved or gone.
    pub fn accept_tracked(&self) {
        let Some(pointer) = self.primary_pointer() else {
            return;
        };
        let Some(member) = self.tracked_member.lock().as_ref().and_then(Weak::upgrade) else {
            return;
        };
        self.arena.resolve(pointer, Some(member));
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
        if let Some(pointer) = self.primary_pointer() {
            self.arena.sweep(pointer);
        }
        self.set_primary_pointer(None);
        self.set_initial_position(None);
    }

    /// Accept this gesture (win the arena)
    #[instrument(
        name = "recognizer.accept",
        level = "debug",
        skip(self, recognizer),
        fields(
            pointer = ?self.primary_pointer(),
            event = %crate::observability::GestureEvent::ArenaAccepted,
        )
    )]
    pub fn accept<T: GestureArenaMember + Clone + 'static>(&self, recognizer: &Arc<T>) {
        if let Some(pointer) = self.primary_pointer() {
            self.arena.resolve(pointer, Some(recognizer.clone()));
        }
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
        let Some(pointer) = self.primary_pointer() else {
            return;
        };
        // Withdraw ONLY this recognizer from the arena, using the stable member
        // identity captured in `start_tracking`. Resolving the whole entry with
        // no winner (the previous behavior) rejected every *competing* member
        // too, so a recognizer that bowed out of a shared arena (e.g. a tap
        // exceeding its slop) silently killed the drag it was competing with.
        let member = self.tracked_member.lock().as_ref().and_then(Weak::upgrade);
        if let Some(member) = member {
            self.arena.reject_member(pointer, &member);
        }
        // Clear ONLY this recognizer's local tracking — do NOT `stop_tracking`
        // (it would `sweep`, force-resolving any still-open competition in
        // first-member-wins fashion mid-gesture). Tear the shared entry down
        // only once it has actually settled (a winner emerged, or no members
        // remain); a competition that still has rivals must keep running until
        // one accepts or the pointer lifts.
        self.set_primary_pointer(None);
        self.set_initial_position(None);
        self.arena.remove_if_settled(pointer);
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

        base.set_primary_pointer(Some(pointer));
        base.set_initial_position(Some(position));

        assert_eq!(base.primary_pointer(), Some(pointer));
        assert_eq!(base.initial_position(), Some(position));

        base.stop_tracking();
        assert_eq!(base.primary_pointer(), None);
        assert_eq!(base.initial_position(), None);
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

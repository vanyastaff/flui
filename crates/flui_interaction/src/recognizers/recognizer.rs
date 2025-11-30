//! Base traits and types for gesture recognizers
//!
//! Defines the core `GestureRecognizer` trait and common types used by all recognizers.

use crate::arena::{GestureArena, GestureArenaMember};
use crate::ids::PointerId;
use flui_types::{events::PointerEvent, Offset};
use parking_lot::Mutex;
use std::sync::Arc;

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
    fn add_pointer(&self, pointer: PointerId, position: Offset);

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

/// Base state for gesture recognizers
///
/// Provides common functionality that all recognizers need:
/// - Arena membership
/// - Primary pointer tracking
/// - Initial position tracking
/// - Disposal
#[derive(Clone)]
pub struct GestureRecognizerState {
    /// Gesture arena for conflict resolution
    arena: GestureArena,

    /// Primary pointer ID being tracked
    primary_pointer: Arc<Mutex<Option<PointerId>>>,

    /// Initial position of primary pointer
    initial_position: Arc<Mutex<Option<Offset>>>,

    /// Whether recognizer has been disposed
    disposed: Arc<Mutex<bool>>,
}

impl GestureRecognizerState {
    /// Create new recognizer state with arena
    pub fn new(arena: GestureArena) -> Self {
        Self {
            arena,
            primary_pointer: Arc::new(Mutex::new(None)),
            initial_position: Arc::new(Mutex::new(None)),
            disposed: Arc::new(Mutex::new(false)),
        }
    }

    /// Get the gesture arena
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// Get the primary pointer ID (if tracking one)
    pub fn primary_pointer(&self) -> Option<PointerId> {
        *self.primary_pointer.lock()
    }

    /// Set the primary pointer
    pub fn set_primary_pointer(&self, pointer: Option<PointerId>) {
        *self.primary_pointer.lock() = pointer;
    }

    /// Get the initial position of the primary pointer
    pub fn initial_position(&self) -> Option<Offset> {
        *self.initial_position.lock()
    }

    /// Set the initial position
    pub fn set_initial_position(&self, position: Option<Offset>) {
        *self.initial_position.lock() = position;
    }

    /// Check if recognizer has been disposed
    pub fn is_disposed(&self) -> bool {
        *self.disposed.lock()
    }

    /// Mark as disposed
    pub fn mark_disposed(&self) {
        *self.disposed.lock() = true;
    }

    /// Start tracking a pointer
    ///
    /// Sets this as the primary pointer and stores initial position.
    /// Adds recognizer to gesture arena.
    pub fn start_tracking<T: GestureArenaMember + Clone + 'static>(
        &self,
        pointer: PointerId,
        position: Offset,
        recognizer: &Arc<T>,
    ) {
        if self.is_disposed() {
            return;
        }

        self.set_primary_pointer(Some(pointer));
        self.set_initial_position(Some(position));

        // Add to arena (clone Arc to satisfy trait bounds)
        self.arena.add(pointer, recognizer.clone());
    }

    /// Stop tracking (called on success or rejection)
    pub fn stop_tracking(&self) {
        if let Some(pointer) = self.primary_pointer() {
            self.arena.sweep(pointer);
        }
        self.set_primary_pointer(None);
        self.set_initial_position(None);
    }

    /// Accept this gesture (win the arena)
    pub fn accept<T: GestureArenaMember + Clone + 'static>(&self, recognizer: &Arc<T>) {
        if let Some(pointer) = self.primary_pointer() {
            self.arena.resolve(pointer, Some(recognizer.clone()));
        }
    }

    /// Reject this gesture (lose the arena or explicit rejection)
    pub fn reject(&self) {
        if let Some(pointer) = self.primary_pointer() {
            self.arena.resolve(pointer, None);
            self.stop_tracking();
        }
    }
}

impl std::fmt::Debug for GestureRecognizerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureRecognizerState")
            .field("primary_pointer", &self.primary_pointer())
            .field("initial_position", &self.initial_position())
            .field("disposed", &self.is_disposed())
            .finish()
    }
}

/// Gesture state enum
///
/// Tracks what state a gesture recognizer is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureState {
    /// Ready to start tracking
    Ready,

    /// Possible - might become a gesture
    Possible,

    /// Started - gesture has begun
    Started,

    /// Accepted - won the arena
    Accepted,

    /// Rejected - lost the arena or explicitly rejected
    Rejected,
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
    fn test_recognizer_state_creation() {
        let arena = GestureArena::new();
        let state = GestureRecognizerState::new(arena);

        assert_eq!(state.primary_pointer(), None);
        assert_eq!(state.initial_position(), None);
        assert!(!state.is_disposed());
    }

    #[test]
    fn test_recognizer_state_tracking() {
        let arena = GestureArena::new();
        let state = GestureRecognizerState::new(arena);

        let pointer = PointerId::new(1);
        let position = Offset::new(100.0, 200.0);

        state.set_primary_pointer(Some(pointer));
        state.set_initial_position(Some(position));

        assert_eq!(state.primary_pointer(), Some(pointer));
        assert_eq!(state.initial_position(), Some(position));

        state.stop_tracking();
        assert_eq!(state.primary_pointer(), None);
        assert_eq!(state.initial_position(), None);
    }

    #[test]
    fn test_gesture_state_enum() {
        let state = GestureState::Ready;
        assert_eq!(state, GestureState::Ready);

        let state = GestureState::Started;
        assert_ne!(state, GestureState::Ready);
    }

    #[test]
    fn test_constants() {
        assert!(constants::TAP_SLOP > 0.0);
        assert!(constants::DRAG_SLOP > 0.0);
        assert!(constants::MIN_FLING_VELOCITY > 0.0);
    }
}

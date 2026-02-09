//! One-Sequence Gesture Recognizer
//!
//! Base trait for gesture recognizers that track a single pointer sequence.
//! This is the foundation for most single-finger/single-pointer gestures
//! like tap, long-press, and vertical/horizontal drag.
//!
//! # Architecture
//!
//! ```text
//! GestureArenaMember (trait)
//!     │
//!     └── OneSequenceGestureRecognizer (trait)
//!             │
//!             └── PrimaryPointerGestureRecognizer (trait)
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::recognizers::OneSequenceGestureRecognizer;
//!
//! struct MyRecognizer {
//!     tracking: Option<PointerId>,
//! }
//!
//! impl OneSequenceGestureRecognizer for MyRecognizer {
//!     fn start_tracking_pointer(&mut self, pointer: PointerId) {
//!         self.tracking = Some(pointer);
//!     }
//!
//!     fn stop_tracking_pointer(&mut self, pointer: PointerId) {
//!         if self.tracking == Some(pointer) {
//!             self.tracking = None;
//!         }
//!     }
//!
//!     fn is_tracking_pointer(&self, pointer: PointerId) -> bool {
//!         self.tracking == Some(pointer)
//!     }
//! }
//! ```

use crate::arena::GestureArenaMember;
use crate::ids::PointerId;
use crate::settings::GestureSettings;
use flui_types::geometry::Matrix4;
use std::sync::Arc;

/// Trait for gesture recognizers that track a single pointer sequence.
///
/// A "sequence" is defined as all events from pointer down to pointer up/cancel
/// for a single pointer. This trait provides the infrastructure for:
///
/// - Tracking which pointer is being followed
/// - Storing the initial transform for coordinate conversion
/// - Managing arena participation for the tracked pointer
///
/// # When to Use
///
/// Use this trait when your gesture:
/// - Only cares about one pointer at a time
/// - Needs to track events from down to up/cancel
/// - Wants automatic arena entry management
///
/// # When NOT to Use
///
/// Don't use this trait when your gesture:
/// - Needs to track multiple pointers (use multi-pointer base instead)
/// - Doesn't need arena participation
/// - Is purely passive/observational
pub trait OneSequenceGestureRecognizer: GestureArenaMember {
    /// Start tracking a pointer.
    ///
    /// Called when pointer down is received and the recognizer decides
    /// to track this pointer. Implementations should:
    ///
    /// 1. Store the pointer ID
    /// 2. Add self to the arena for this pointer
    /// 3. Store any initial state (position, timestamp, etc.)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn start_tracking_pointer(&mut self, pointer: PointerId) {
    ///     self.tracked_pointer = Some(pointer);
    ///     self.arena.add(pointer, self.clone_arc());
    ///     self.initial_position = self.last_position;
    /// }
    /// ```
    fn start_tracking_pointer(&mut self, pointer: PointerId);

    /// Stop tracking a pointer.
    ///
    /// Called when:
    /// - Gesture completes (pointer up)
    /// - Gesture is cancelled (pointer cancel)
    /// - Arena rejects this recognizer
    /// - Recognizer decides to give up on the gesture
    ///
    /// Implementations should clean up any tracking state.
    fn stop_tracking_pointer(&mut self, pointer: PointerId);

    /// Check if a pointer is being tracked.
    fn is_tracking_pointer(&self, pointer: PointerId) -> bool;

    /// Get the currently tracked pointer (if any).
    fn tracked_pointer(&self) -> Option<PointerId>;

    /// Get the initial transform when tracking started.
    ///
    /// Used for coordinate conversion between global and local coordinates.
    fn initial_transform(&self) -> Option<&Matrix4>;

    /// Set the initial transform.
    ///
    /// Called by the framework when starting to track a pointer.
    fn set_initial_transform(&mut self, transform: Matrix4);

    /// Get the gesture settings.
    fn settings(&self) -> &GestureSettings;

    /// Set gesture settings.
    fn set_settings(&mut self, settings: GestureSettings);

    /// Stop tracking all pointers and reset state.
    ///
    /// Called when the recognizer needs to completely reset,
    /// such as when being disposed or reused.
    fn stop_tracking_all(&mut self) {
        if let Some(pointer) = self.tracked_pointer() {
            self.stop_tracking_pointer(pointer);
        }
    }

    /// Resolve this recognizer's arena entry.
    ///
    /// Called by the framework to resolve the arena for the tracked pointer.
    fn resolve_arena(&self, arena: &crate::arena::GestureArena, accept: bool) {
        if let Some(pointer) = self.tracked_pointer() {
            if accept {
                // Need to get self as Arc - this is typically done via a stored reference
                // The actual implementation would use the recognizer's stored Arc
                // arena.accept(pointer, self_arc);
                let _ = (arena, pointer); // Placeholder
            } else {
                // arena.reject(pointer, &self_arc);
            }
        }
    }
}

/// Helper struct for managing single-pointer tracking state.
///
/// Provides common state management that most OneSequenceGestureRecognizer
/// implementations need.
#[derive(Debug, Clone)]
pub struct OneSequenceState {
    /// Currently tracked pointer.
    tracked_pointer: Option<PointerId>,
    /// Transform when tracking started.
    initial_transform: Option<Matrix4>,
    /// Gesture settings.
    settings: GestureSettings,
    /// Arena reference for resolving.
    arena: Option<Arc<crate::arena::GestureArena>>,
}

impl Default for OneSequenceState {
    fn default() -> Self {
        Self::new()
    }
}

impl OneSequenceState {
    /// Create new tracking state.
    pub fn new() -> Self {
        Self {
            tracked_pointer: None,
            initial_transform: None,
            settings: GestureSettings::default(),
            arena: None,
        }
    }

    /// Create with specific settings.
    pub fn with_settings(settings: GestureSettings) -> Self {
        Self {
            tracked_pointer: None,
            initial_transform: None,
            settings,
            arena: None,
        }
    }

    /// Create with arena reference.
    pub fn with_arena(arena: Arc<crate::arena::GestureArena>) -> Self {
        Self {
            tracked_pointer: None,
            initial_transform: None,
            settings: GestureSettings::default(),
            arena: Some(arena),
        }
    }

    /// Set the arena.
    pub fn set_arena(&mut self, arena: Arc<crate::arena::GestureArena>) {
        self.arena = Some(arena);
    }

    /// Get arena reference.
    pub fn arena(&self) -> Option<&Arc<crate::arena::GestureArena>> {
        self.arena.as_ref()
    }

    /// Start tracking a pointer.
    pub fn start_tracking(&mut self, pointer: PointerId) {
        self.tracked_pointer = Some(pointer);
    }

    /// Stop tracking a pointer.
    pub fn stop_tracking(&mut self, pointer: PointerId) {
        if self.tracked_pointer == Some(pointer) {
            self.tracked_pointer = None;
            self.initial_transform = None;
        }
    }

    /// Check if tracking a pointer.
    pub fn is_tracking(&self, pointer: PointerId) -> bool {
        self.tracked_pointer == Some(pointer)
    }

    /// Get tracked pointer.
    pub fn tracked_pointer(&self) -> Option<PointerId> {
        self.tracked_pointer
    }

    /// Check if tracking any pointer.
    pub fn is_tracking_any(&self) -> bool {
        self.tracked_pointer.is_some()
    }

    /// Get initial transform.
    pub fn initial_transform(&self) -> Option<&Matrix4> {
        self.initial_transform.as_ref()
    }

    /// Set initial transform.
    pub fn set_initial_transform(&mut self, transform: Matrix4) {
        self.initial_transform = Some(transform);
    }

    /// Get settings.
    pub fn settings(&self) -> &GestureSettings {
        &self.settings
    }

    /// Set settings.
    pub fn set_settings(&mut self, settings: GestureSettings) {
        self.settings = settings;
    }

    /// Reset all state.
    pub fn reset(&mut self) {
        self.tracked_pointer = None;
        self.initial_transform = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_one_sequence_state_new() {
        let state = OneSequenceState::new();
        assert!(state.tracked_pointer().is_none());
        assert!(state.initial_transform().is_none());
        assert!(!state.is_tracking_any());
    }

    #[test]
    fn test_one_sequence_state_tracking() {
        let mut state = OneSequenceState::new();
        let pointer = PointerId::new(1);

        state.start_tracking(pointer);
        assert!(state.is_tracking(pointer));
        assert!(state.is_tracking_any());
        assert_eq!(state.tracked_pointer(), Some(pointer));

        state.stop_tracking(pointer);
        assert!(!state.is_tracking(pointer));
        assert!(!state.is_tracking_any());
        assert!(state.tracked_pointer().is_none());
    }

    #[test]
    fn test_one_sequence_state_wrong_pointer() {
        let mut state = OneSequenceState::new();
        let pointer1 = PointerId::new(1);
        let pointer2 = PointerId::new(2);

        state.start_tracking(pointer1);
        assert!(state.is_tracking(pointer1));
        assert!(!state.is_tracking(pointer2));

        // Stopping wrong pointer doesn't affect tracking
        state.stop_tracking(pointer2);
        assert!(state.is_tracking(pointer1));
    }

    #[test]
    fn test_one_sequence_state_transform() {
        let mut state = OneSequenceState::new();
        assert!(state.initial_transform().is_none());

        let transform = Matrix4::IDENTITY;
        state.set_initial_transform(transform);
        assert!(state.initial_transform().is_some());

        state.reset();
        assert!(state.initial_transform().is_none());
    }

    #[test]
    fn test_one_sequence_state_settings() {
        let mut state = OneSequenceState::new();
        let settings = GestureSettings::mouse_defaults();

        state.set_settings(settings.clone());
        assert_eq!(state.settings().touch_slop(), settings.touch_slop());
    }

    #[test]
    fn test_one_sequence_state_with_settings() {
        let settings = GestureSettings::mouse_defaults();
        let state = OneSequenceState::with_settings(settings.clone());
        assert_eq!(state.settings().touch_slop(), settings.touch_slop());
    }
}

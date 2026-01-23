//! Sealed trait pattern implementation with extension points
//!
//! This module provides sealed traits with **extension points** for custom implementations.
//!
//! # Architecture
//!
//! We use a two-tier trait system:
//!
//! 1. **Sealed traits** (internal) - cannot be implemented directly
//! 2. **Extension traits** (public) - can be implemented by external crates
//!
//! When you implement an extension trait, you automatically get the sealed trait
//! via blanket implementation.
//!
//! # Example: Custom Gesture Recognizer
//!
//! ```rust,ignore
//! use flui_interaction::prelude::*;
//!
//! struct CircularGestureRecognizer {
//!     // Your custom state
//! }
//!
//! impl CustomGestureRecognizer for CircularGestureRecognizer {
//!     fn on_arena_accept(&self, pointer: PointerId) {
//!         println!("Circle gesture accepted!");
//!     }
//!
//!     fn on_arena_reject(&self, pointer: PointerId) {
//!         println!("Circle gesture rejected");
//!     }
//! }
//!
//! // Now you can use it with GestureArena!
//! let arena = GestureArena::new();
//! arena.add(pointer_id, Arc::new(recognizer));
//! ```

use crate::ids::PointerId;
use flui_types::geometry::Pixels;

use flui_types::geometry::Offset;

// ============================================================================
// Extension Traits (PUBLIC - implement these for custom types)
// ============================================================================

/// Extension trait for custom gesture recognizers.
///
/// Implement this trait to create your own gesture recognizers that can
/// participate in the gesture arena conflict resolution system.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::sealed::CustomGestureRecognizer;
/// use flui_interaction::ids::PointerId;
///
/// struct SwipePatternRecognizer {
///     pattern: Vec<Direction>,
///     current_index: usize,
/// }
///
/// impl CustomGestureRecognizer for SwipePatternRecognizer {
///     fn on_arena_accept(&self, pointer: PointerId) {
///         // Called when this recognizer wins the arena
///         println!("Pattern matched!");
///     }
///
///     fn on_arena_reject(&self, pointer: PointerId) {
///         // Called when another recognizer wins
///         self.reset();
///     }
/// }
/// ```
///
/// # Thread Safety
///
/// Custom recognizers must be `Send + Sync` to work with the concurrent
/// gesture arena.
pub trait CustomGestureRecognizer: Send + Sync {
    /// Called when this recognizer wins the gesture arena.
    ///
    /// This means your gesture was recognized and other competing
    /// recognizers have been rejected.
    fn on_arena_accept(&self, pointer: PointerId);

    /// Called when this recognizer loses the gesture arena.
    ///
    /// Another recognizer won, or this recognizer explicitly rejected.
    /// Clean up any state and prepare for the next gesture.
    fn on_arena_reject(&self, pointer: PointerId);
}

/// Extension trait for custom hit-testable types.
///
/// Implement this trait to create custom layers or UI elements that can
/// participate in hit testing.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::sealed::CustomHitTestable;
/// use flui_interaction::hit_test::{HitTestResult, HitTestBehavior, HitTestEntry};
/// use flui_types::geometry::Offset;
///
/// struct CustomLayer {
///     bounds: Rect,
///     children: Vec<CustomLayer>,
/// }
///
/// impl CustomHitTestable for CustomLayer {
///     fn perform_hit_test(&self, position: Offset<Pixels>, result: &mut HitTestResult) -> bool {
///         if !self.bounds.contains(position) {
///             return false;
///         }
///
///         // Test children first
///         for child in &self.children {
///             if child.perform_hit_test(position, result) {
///                 return true;
///             }
///         }
///
///         // Add self to result
///         result.add(HitTestEntry::new(self.element_id, position, self.bounds));
///         true
///     }
///
///     fn get_hit_test_behavior(&self) -> HitTestBehavior {
///         HitTestBehavior::Opaque
///     }
/// }
/// ```
pub trait CustomHitTestable: Send + Sync {
    /// Perform hit testing at the given position.
    ///
    /// Returns `true` if this element (or a child) was hit.
    ///
    /// # Arguments
    ///
    /// * `position` - Point to test, in this element's coordinate space
    /// * `result` - Accumulator for hit test results
    fn perform_hit_test(
        &self,
        position: Offset<Pixels>,
        result: &mut crate::routing::HitTestResult,
    ) -> bool;

    /// Returns the hit test behavior for this element.
    ///
    /// Default is `DeferToChild`.
    fn get_hit_test_behavior(&self) -> crate::routing::HitTestBehavior {
        crate::routing::HitTestBehavior::DeferToChild
    }
}

// ============================================================================
// Sealed Traits (INTERNAL - do not implement directly)
// ============================================================================

/// Sealed trait for hit testable types.
///
/// **Do not implement directly.** Instead, implement [`CustomHitTestable`].
pub mod hit_testable {
    pub trait Sealed {}

    // Blanket impl: any CustomHitTestable automatically gets Sealed
    impl<T: super::CustomHitTestable> Sealed for T {}

    // Test implementations
    #[cfg(test)]
    impl Sealed for crate::routing::event_router::tests::MockLayer {}
}

/// Sealed trait for gesture recognizers.
///
/// **Do not implement directly.** Instead, implement [`CustomGestureRecognizer`].
pub mod gesture_recognizer {
    pub trait Sealed {}
}

/// Sealed trait for arena members.
///
/// **Do not implement directly.** Instead, implement [`CustomGestureRecognizer`].
pub mod arena_member {
    pub trait Sealed {}

    // Blanket impl: any CustomGestureRecognizer automatically gets Sealed
    impl<T: super::CustomGestureRecognizer> Sealed for T {}

    // Built-in gesture recognizers
    impl Sealed for crate::recognizers::TapGestureRecognizer {}
    impl Sealed for crate::recognizers::DoubleTapGestureRecognizer {}
    impl Sealed for crate::recognizers::LongPressGestureRecognizer {}
    impl Sealed for crate::recognizers::DragGestureRecognizer {}
    impl Sealed for crate::recognizers::ScaleGestureRecognizer {}
    impl Sealed for crate::recognizers::MultiTapGestureRecognizer {}
    impl Sealed for crate::recognizers::ForcePressGestureRecognizer {}
}

/// Sealed trait for focus nodes.
///
/// This restricts which types can receive keyboard focus.
pub mod focus_node {
    pub trait Sealed {}
}

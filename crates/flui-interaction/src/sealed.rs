//! Sealed hit-test extension points.
//!
//! Gesture recognizers and arena members implement their open traits directly.

use flui_foundation::geometry::Offset;

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
/// use flui_foundation::geometry::Offset;
///
/// struct CustomLayer {
///     bounds: Rect,
///     children: Vec<CustomLayer>,
/// }
///
/// impl CustomHitTestable for CustomLayer {
///     fn perform_hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
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
        position: Offset<f64>,
        result: &mut crate::routing::HitTestResult,
    ) -> bool;

    /// Returns the hit test behavior for this element.
    ///
    /// Default is `DeferToChild`.
    fn get_hit_test_behavior(&self) -> crate::routing::HitTestBehavior {
        crate::routing::HitTestBehavior::DeferToChild
    }
}

/// Sealed trait for hit testable types.
///
/// **Do not implement directly.** Instead, implement [`CustomHitTestable`].
pub mod hit_testable {
    /// Marker supertrait sealing `HitTestable`; implemented automatically for
    /// every [`CustomHitTestable`](super::CustomHitTestable) via blanket impl.
    pub trait Sealed {}

    // Blanket impl: any CustomHitTestable automatically gets Sealed
    impl<T: super::CustomHitTestable> Sealed for T {}
}

/// Sealed trait for focus nodes.
///
/// This restricts which types can receive keyboard focus.
pub mod focus_node {
    /// Marker supertrait restricting which types can receive keyboard focus.
    pub trait Sealed {}
}

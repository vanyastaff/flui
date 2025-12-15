//! Core Protocol trait definition.
//!
//! The Protocol trait is the foundation of FLUI's type-safe render object system.
//! It defines the associated types that form a complete layout protocol.

use std::fmt::Debug;

/// Core abstraction for render object protocol families.
///
/// The Protocol trait defines four associated types that together form
/// a complete layout protocol:
///
/// - `Object`: The trait object type for render objects in this protocol
/// - `Constraints`: Layout input passed from parent to child
/// - `ParentData`: Metadata stored on each child by the parent
/// - `Geometry`: Layout output returned from child to parent
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's implicit protocol system, but made explicit
/// through Rust's type system for compile-time safety.
///
/// # Implementation Guide
///
/// To create a custom protocol:
///
/// ```ignore
/// use flui_rendering::protocol::Protocol;
///
/// #[derive(Debug, Clone, Copy, Default)]
/// pub struct MyProtocol;
///
/// impl Protocol for MyProtocol {
///     type Object = dyn MyRenderObject;
///     type Constraints = MyConstraints;
///     type ParentData = MyParentData;
///     type Geometry = MyGeometry;
///
///     fn name() -> &'static str {
///         "my_protocol"
///     }
/// }
/// ```
///
/// # Type Safety Benefits
///
/// The Protocol trait enables:
/// - Compile-time verification of parent-child compatibility
/// - Type-safe containers that only accept compatible render objects
/// - Clear separation between layout protocols
/// - Extensibility for custom layout systems
pub trait Protocol: Send + Sync + Debug + Clone + Copy + 'static {
    /// The type of render objects this protocol contains.
    ///
    /// This is a trait object type like `dyn RenderBox` or `dyn RenderSliver`.
    type Object: ?Sized;

    /// Layout input type passed from parent to child.
    ///
    /// For box protocol: `BoxConstraints` (min/max width/height)
    /// For sliver protocol: `SliverConstraints` (scroll position, viewport extent)
    type Constraints: Clone + Debug + Send + Sync + 'static;

    /// Child metadata type stored on each child.
    ///
    /// Used by parent render objects to store child-specific data like
    /// position offsets, flex factors, etc.
    type ParentData: Default + Debug + Send + Sync + 'static;

    /// Layout output type returned from child to parent.
    ///
    /// For box protocol: `Size` (width, height)
    /// For sliver protocol: `SliverGeometry` (scroll extent, paint extent, etc.)
    type Geometry: Clone + Debug + Default + Send + Sync + 'static;

    /// Returns default geometry value for uninitialized state.
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }

    /// Returns protocol name for debugging.
    fn name() -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test that Protocol is object-safe for the parts that matter
    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_protocol_bounds() {
        // Verify Protocol has correct bounds
        fn check_protocol<P: Protocol>() {
            _assert_send_sync::<P>();
        }

        // This will be tested with actual implementations
        let _ = check_protocol::<super::super::BoxProtocol>;
    }
}

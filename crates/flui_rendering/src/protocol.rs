//! Protocol trait and implementations for render object families.
//!
//! The Protocol trait defines the type system for render object families.
//! Each protocol specifies associated types that determine how layout,
//! constraints, and children work within that protocol's domain.
//!
//! # Protocols
//!
//! - [`BoxProtocol`]: 2D cartesian layout with rectangular constraints
//! - [`SliverProtocol`]: Scrollable content with viewport-aware constraints

use std::fmt::Debug;

use flui_types::{BoxConstraints, Size, SliverConstraints, SliverGeometry};

use crate::parent_data::{BoxParentData, SliverParentData};
use crate::traits::{RenderBox, RenderSliver};

// ============================================================================
// Protocol Trait
// ============================================================================

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
/// # Type Flow
///
/// ```text
///                     Protocol Trait
///                          │
///            ┌─────────────┼─────────────┐
///            ▼             ▼             ▼
///     type Object   type Constraints  type Geometry
///            │             │             │
///            ▼             ▼             ▼
///     Container      Layout Input    Layout Output
///      Storage       (parent→child)  (child→parent)
/// ```
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

// ============================================================================
// BoxProtocol
// ============================================================================

/// 2D cartesian layout protocol with rectangular constraints.
///
/// This is the primary layout protocol for most UI widgets. It uses
/// `BoxConstraints` to specify min/max dimensions and returns a `Size`.
///
/// # Layout Model
///
/// 1. Parent passes `BoxConstraints` specifying allowed size range
/// 2. Child computes its size within those constraints
/// 3. Child returns `Size` (width, height) as layout result
/// 4. Parent positions child using `Offset` stored in parent data
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Object = dyn RenderBox;
    type Constraints = BoxConstraints;
    type ParentData = BoxParentData;
    type Geometry = Size;

    fn default_geometry() -> Size {
        Size::ZERO
    }

    fn name() -> &'static str {
        "box"
    }
}

// ============================================================================
// SliverProtocol
// ============================================================================

/// Scrollable content protocol with viewport-aware constraints.
///
/// Used for efficiently rendering scrollable content. Slivers only
/// render the portion of content visible in the viewport.
///
/// # Layout Model
///
/// 1. Parent passes `SliverConstraints` with scroll position and viewport info
/// 2. Child computes what portion is visible and how much space it consumes
/// 3. Child returns `SliverGeometry` with scroll extent, paint extent, etc.
/// 4. Parent composes sliver geometries to build scrollable view
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Object = dyn RenderSliver;
    type Constraints = SliverConstraints;
    type ParentData = SliverParentData;
    type Geometry = SliverGeometry;

    fn default_geometry() -> SliverGeometry {
        SliverGeometry::zero()
    }

    fn name() -> &'static str {
        "sliver"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_protocol_types() {
        assert_eq!(BoxProtocol::name(), "box");
        assert_eq!(BoxProtocol::default_geometry(), Size::ZERO);
    }

    #[test]
    fn test_sliver_protocol_types() {
        assert_eq!(SliverProtocol::name(), "sliver");
        assert_eq!(SliverProtocol::default_geometry(), SliverGeometry::zero());
    }
}

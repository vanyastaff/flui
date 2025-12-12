//! Protocol system for render objects
//!
//! The Protocol trait defines the type system for render object families.
//! Each protocol specifies four associated types that determine how layout,
//! constraints, and children work within that protocol's domain.

use std::fmt::Debug;

use crate::parent_data::ParentData;

/// Core protocol trait defining the type system for a render object family
///
/// This trait uses associated types to provide compile-time type safety across protocols.
/// Each protocol specifies what type of objects it contains, what constraints are used
/// for layout, what metadata is stored on children, and what geometry is produced.
pub trait Protocol: Send + Sync + Debug + 'static {
    /// The type of render objects this protocol contains
    ///
    /// For BoxProtocol: `dyn RenderBox`
    /// For SliverProtocol: `dyn RenderSliver`
    ///
    /// Must be Send + Sync for thread-safe child storage in ArityStorage.
    type Object: ?Sized + Send + Sync;

    /// Layout input type (passed down from parent to child)
    ///
    /// For BoxProtocol: `BoxConstraints` (min/max width/height)
    /// For SliverProtocol: `SliverConstraints` (scroll offset, viewport info)
    type Constraints: Clone + Debug;

    /// Child metadata type (stored on each child by the parent)
    ///
    /// For BoxProtocol: `BoxParentData` (offset)
    /// For SliverProtocol: `SliverParentData` (paint offset)
    type ParentData: ParentData;

    /// Layout output type (returned from layout)
    ///
    /// For BoxProtocol: `Size` (width, height)
    /// For SliverProtocol: `SliverGeometry` (scroll/paint extents)
    type Geometry: Clone + Debug;

    /// Default geometry value for uninitialized state
    fn default_geometry() -> Self::Geometry;

    /// Protocol name for debugging
    fn name() -> &'static str;
}

/// Box protocol for 2D cartesian layout with rectangular constraints
///
/// The Box protocol is used for traditional 2D layouts where objects have
/// a fixed size (width and height) and are positioned using Cartesian coordinates.
///
/// # Type Parameters
/// - **Object**: `dyn RenderBox` - Box render objects
/// - **Constraints**: `BoxConstraints` - Min/max width and height
/// - **ParentData**: `BoxParentData` - Offset position
/// - **Geometry**: `Size` - Width and height
///
/// # Use Cases
/// - Fixed size widgets (Container, SizedBox)
/// - Flexible layouts (Row, Column, Flex)
/// - Effects (Opacity, Transform, Clip)
/// - Custom painting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxProtocol;

/// Sliver protocol for scrollable content with viewport-aware constraints
///
/// The Sliver protocol is used for scrollable content that can extend infinitely
/// along a scroll axis and is viewport-aware for lazy rendering.
///
/// # Type Parameters
/// - **Object**: `dyn RenderSliver` - Sliver render objects
/// - **Constraints**: `SliverConstraints` - Scroll offset and viewport info
/// - **ParentData**: `SliverParentData` - Paint offset along scroll axis
/// - **Geometry**: `SliverGeometry` - Scroll extent and paint extent
///
/// # Use Cases
/// - Scrollable lists (ListView)
/// - Infinite scrolling
/// - Grid layouts in scrollable viewports
/// - Lazy rendering of large data sets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SliverProtocol;

// Note: Protocol implementations for BoxProtocol and SliverProtocol are in
// separate files (box_protocol.rs and sliver_protocol.rs) to avoid circular
// dependencies with RenderBox/RenderSliver trait definitions.

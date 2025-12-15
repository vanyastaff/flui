//! SliverProtocol - Scrollable content layout protocol.
//!
//! The sliver protocol is used for efficiently rendering scrollable content.
//! Slivers only render the portion of content visible in the viewport.

use super::Protocol;
use crate::constraints::{SliverConstraints, SliverGeometry};
use crate::parent_data::SliverParentData;
use crate::traits::RenderSliver;

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
///
/// # Key Concepts
///
/// - **Scroll Offset**: How far the content has been scrolled
/// - **Viewport Main Axis Extent**: Size of the visible area
/// - **Remaining Paint Extent**: Space left for this sliver to paint
/// - **Cache Extent**: Extra area to keep rendered for smooth scrolling
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's sliver layout protocol used by `RenderSliver`.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::protocol::SliverProtocol;
/// use flui_rendering::containers::ChildList;
/// use flui_rendering::parent_data::SliverPhysicalParentData;
///
/// // Create a container for sliver protocol children with physical parent data
/// let children: ChildList<SliverProtocol, flui_tree::arity::Variable, SliverPhysicalParentData> = ChildList::new();
/// ```
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
    fn test_sliver_protocol_name() {
        assert_eq!(SliverProtocol::name(), "sliver");
    }

    #[test]
    fn test_sliver_protocol_default_geometry() {
        let geom = SliverProtocol::default_geometry();
        assert_eq!(geom.scroll_extent, 0.0);
        assert_eq!(geom.paint_extent, 0.0);
        assert_eq!(geom.layout_extent, 0.0);
    }

    #[test]
    fn test_sliver_protocol_is_copy() {
        let p1 = SliverProtocol;
        let _p2 = p1; // Copy - compiles because SliverProtocol is Copy
        assert_eq!(SliverProtocol::name(), "sliver");
    }
}

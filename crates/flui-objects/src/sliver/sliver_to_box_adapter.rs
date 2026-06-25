//! `RenderSliverToBoxAdapter` — Sliver wrapper for one Box child.
//!
//! Mirrors Flutter's `RenderSliverToBoxAdapter`: the Box child is laid out
//! with tight cross-axis constraints derived from the sliver constraint space,
//! then the child's main-axis size becomes the sliver scroll extent.

use flui_foundation::Diagnosticable;
use flui_tree::Single;
use flui_types::{geometry::px, layout::AxisDirection::*};

use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

/// A Sliver-protocol adapter that lays out one Box-protocol child.
///
/// 2B field dedup: `constraints` and `geometry` live solely on
/// `RenderState<SliverProtocol>`. `perform_layout` returns its geometry
/// directly; the `child_main_axis_position` hook receives the incoming
/// `SliverConstraints` as an argument; the paint/hit gates (visibility and
/// `hit_test_extent`) are owned by the pipeline driver.
#[derive(Debug, Clone)]
pub struct RenderSliverToBoxAdapter;

impl RenderSliverToBoxAdapter {
    /// Creates an adapter with no laid-out geometry yet.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for RenderSliverToBoxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// Geometry-only adapter: no configurable fields to surface. The committed
// sliver geometry is layered onto the diagnostics node by the tree walk.
impl Diagnosticable for RenderSliverToBoxAdapter {}
impl RenderSliver for RenderSliverToBoxAdapter {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        if ctx.child_count() == 0 {
            return SliverGeometry::ZERO;
        }

        let child_size = ctx.layout_box_child(0, constraints.unbounded_main_axis_box_constraints());
        let child_extent = match constraints.axis_direction {
            LeftToRight | RightToLeft => child_size.width.get(),
            TopToBottom | BottomToTop => child_size.height.get(),
        };
        let painted_child_size = self.calculate_paint_offset(&constraints, 0.0, child_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, child_extent);

        let geometry = SliverGeometry {
            scroll_extent: child_extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: child_extent,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: child_extent > constraints.remaining_paint_extent
                || constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        let child_paint_offset =
            child_paint_offset(&constraints, &geometry, px(0.0), px(child_extent));
        ctx.position_child(0, child_paint_offset);
        geometry
    }

    fn child_main_axis_position(
        &self,
        constraints: &SliverConstraints,
        _child: &dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::SliverProtocol>,
    ) -> f32 {
        -constraints.scroll_offset
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

//! `RenderSliverToBoxAdapter` — Sliver wrapper for one Box child.
//!
//! Mirrors Flutter's `RenderSliverToBoxAdapter`: the Box child is laid out
//! with tight cross-axis constraints derived from the sliver constraint space,
//! then the child's main-axis size becomes the sliver scroll extent.

use flui_foundation::Diagnosticable;
use flui_tree::Single;
use flui_types::layout::AxisDirection::*;

use crate::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    objects::sliver_helpers::child_paint_offset,
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
};

/// A Sliver-protocol adapter that lays out one Box-protocol child.
#[derive(Debug, Clone)]
pub struct RenderSliverToBoxAdapter {
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl RenderSliverToBoxAdapter {
    /// Creates an adapter with no laid-out geometry yet.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
        }
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
impl PaintEffectsCapability for RenderSliverToBoxAdapter {}
impl SemanticsCapability for RenderSliverToBoxAdapter {}
impl HotReloadCapability for RenderSliverToBoxAdapter {}

impl RenderSliver for RenderSliverToBoxAdapter {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        if ctx.child_count() == 0 {
            self.geometry = SliverGeometry::ZERO;
            ctx.complete(self.geometry);
            return;
        }

        let child_size = ctx.layout_box_child(
            0,
            self.constraints
                .as_box_constraints(0.0, f32::INFINITY, None),
        );
        let child_extent = match self.constraints.axis_direction {
            LeftToRight | RightToLeft => child_size.width.get(),
            TopToBottom | BottomToTop => child_size.height.get(),
        };
        let painted_child_size = self.calculate_paint_offset(&self.constraints, 0.0, child_extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, child_extent);

        let geometry = SliverGeometry {
            scroll_extent: child_extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: child_extent,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: child_extent > self.constraints.remaining_paint_extent
                || self.constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        let child_paint_offset =
            child_paint_offset(&self.constraints, &geometry, 0.0, child_extent);
        ctx.position_child(0, child_paint_offset);
        self.geometry = geometry;
        ctx.complete(geometry);
    }

    fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.geometry = geometry;
    }

    fn child_main_axis_position(
        &self,
        _child: &dyn crate::traits::RenderObject<crate::protocol::SliverProtocol>,
    ) -> f32 {
        -self.constraints.scroll_offset
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if self.geometry.visible {
            ctx.paint_child();
        }
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

const fn empty_sliver_constraints() -> SliverConstraints {
    SliverConstraints {
        axis_direction: TopToBottom,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: crate::view::ScrollDirection::Idle,
        scroll_offset: 0.0,
        preceding_scroll_extent: 0.0,
        overlap: 0.0,
        remaining_paint_extent: 0.0,
        cross_axis_extent: 0.0,
        cross_axis_direction: LeftToRight,
        viewport_main_axis_extent: 0.0,
        remaining_cache_extent: 0.0,
        cache_origin: 0.0,
    }
}

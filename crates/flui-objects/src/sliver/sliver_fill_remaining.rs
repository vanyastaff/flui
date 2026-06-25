//! `SliverFillRemaining` render objects — single Box child slivers that fill
//! remaining viewport space.
//!
//! The scroll-body variant sizes directly from paint extent. The non-scroll
//! variants query the Box child's max intrinsic main-axis extent through the
//! Sliver -> Box intrinsic bridge and mirror Flutter's
//! `RenderSliverFillRemaining` / `RenderSliverFillRemainingAndOverscroll`
//! geometry formulas.

use flui_foundation::Diagnosticable;
use flui_tree::Single;
use flui_types::{Offset, Size, geometry::px, layout::AxisDirection::*};

use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

/// A Sliver-protocol adapter that sizes one non-scrollable Box child to fill
/// the remaining viewport space, but expands to the child's intrinsic extent
/// when the child is larger.
///
/// 2B field dedup: `constraints` and `geometry` live solely on
/// `RenderState<SliverProtocol>` for every fill-remaining variant in this
/// module. `perform_layout` returns its geometry directly; the
/// `child_main_axis_position` hook receives the incoming `SliverConstraints`
/// as an argument; the paint/hit gates are owned by the pipeline driver.
#[derive(Debug, Clone)]
pub struct RenderSliverFillRemaining;

impl RenderSliverFillRemaining {
    /// Creates a non-scroll fill-remaining sliver with no laid-out geometry yet.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for RenderSliverFillRemaining {
    fn default() -> Self {
        Self::new()
    }
}

// Geometry-only fill: no configurable fields. Committed geometry is layered
// onto the diagnostics node by the tree walk.
impl Diagnosticable for RenderSliverFillRemaining {}

impl RenderSliver for RenderSliverFillRemaining {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let mut extent =
            (constraints.viewport_main_axis_extent - constraints.preceding_scroll_extent).max(0.0);

        if ctx.child_count() > 0 {
            let child_extent = child_max_intrinsic_main_extent(ctx, &constraints);
            extent = extent.max(child_extent);
            ctx.layout_box_child(0, constraints.as_box_constraints(extent, extent, None));
        }

        let painted_child_size = self.calculate_paint_offset(&constraints, 0.0, extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, extent);
        let geometry = SliverGeometry {
            scroll_extent: extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: painted_child_size,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: extent > constraints.remaining_paint_extent
                || constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        if ctx.child_count() > 0 {
            ctx.position_child(0, child_paint_offset(&constraints, &geometry));
        }
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

/// Non-scroll fill-remaining sliver that also includes overscroll in its
/// maximum paint extent.
#[derive(Debug, Clone)]
pub struct RenderSliverFillRemainingAndOverscroll;

impl RenderSliverFillRemainingAndOverscroll {
    /// Creates an overscroll-aware fill-remaining sliver.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for RenderSliverFillRemainingAndOverscroll {
    fn default() -> Self {
        Self::new()
    }
}

// Geometry-only fill: no configurable fields (see above).
impl Diagnosticable for RenderSliverFillRemainingAndOverscroll {}

impl RenderSliver for RenderSliverFillRemainingAndOverscroll {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let mut extent =
            (constraints.viewport_main_axis_extent - constraints.preceding_scroll_extent).max(0.0);
        let mut max_extent =
            (constraints.remaining_paint_extent - constraints.overlap.min(0.0)).max(0.0);
        let mut child_main_extent = extent;

        if ctx.child_count() > 0 {
            let child_extent = child_max_intrinsic_main_extent(ctx, &constraints);
            extent = extent.max(child_extent);
            max_extent = max_extent.max(extent);
            let child_size =
                ctx.layout_box_child(0, constraints.as_box_constraints(extent, max_extent, None));
            child_main_extent = size_main_axis_extent(child_size, &constraints);
        }

        let painted_child_size = max_extent.min(constraints.remaining_paint_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, extent);
        let geometry = SliverGeometry {
            scroll_extent: extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: max_extent,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: extent > constraints.remaining_paint_extent
                || constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        if ctx.child_count() > 0 {
            ctx.position_child(
                0,
                child_paint_offset_for_extent(&constraints, &geometry, child_main_extent),
            );
        }
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

/// A Sliver-protocol adapter that sizes one Box child to the remaining paint
/// extent of the viewport.
#[derive(Debug, Clone)]
pub struct RenderSliverFillRemainingWithScrollable;

impl RenderSliverFillRemainingWithScrollable {
    /// Creates a fill-remaining sliver with no laid-out geometry yet.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for RenderSliverFillRemainingWithScrollable {
    fn default() -> Self {
        Self::new()
    }
}

// Geometry-only fill: no configurable fields (see above).
impl Diagnosticable for RenderSliverFillRemainingWithScrollable {}

impl RenderSliver for RenderSliverFillRemainingWithScrollable {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let extent = constraints.remaining_paint_extent - constraints.overlap.min(0.0);
        let cache_extent =
            self.calculate_cache_offset(&constraints, 0.0, constraints.viewport_main_axis_extent);

        if ctx.child_count() > 0 {
            let max_extent = if extent == 0.0 && cache_extent > 0.0 {
                cache_extent
            } else {
                extent
            };
            ctx.layout_box_child(0, constraints.as_box_constraints(extent, max_extent, None));
        }

        let painted_child_size = self.calculate_paint_offset(&constraints, 0.0, extent);
        let geometry = SliverGeometry {
            scroll_extent: constraints.viewport_main_axis_extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: painted_child_size,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: extent > constraints.remaining_paint_extent
                || constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        if ctx.child_count() > 0 {
            ctx.position_child(0, child_paint_offset(&constraints, &geometry));
        }
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

fn child_max_intrinsic_main_extent(
    ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    constraints: &SliverConstraints,
) -> f32 {
    match constraints.axis_direction.axis() {
        flui_types::layout::Axis::Horizontal => {
            ctx.box_child_max_intrinsic_width(0, constraints.cross_axis_extent)
        }
        flui_types::layout::Axis::Vertical => {
            ctx.box_child_max_intrinsic_height(0, constraints.cross_axis_extent)
        }
    }
}

#[inline]
fn size_main_axis_extent(size: Size, constraints: &SliverConstraints) -> f32 {
    match constraints.axis_direction.axis() {
        flui_types::layout::Axis::Horizontal => size.width.get(),
        flui_types::layout::Axis::Vertical => size.height.get(),
    }
}

#[inline]
fn child_paint_offset(constraints: &SliverConstraints, geometry: &SliverGeometry) -> Offset {
    child_paint_offset_for_extent(constraints, geometry, geometry.scroll_extent)
}

#[inline]
fn child_paint_offset_for_extent(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    child_main_extent: f32,
) -> Offset {
    match constraints
        .growth_direction
        .apply_to_axis_direction(constraints.axis_direction)
    {
        TopToBottom => Offset::new(px(0.0), px(-constraints.scroll_offset)),
        LeftToRight => Offset::new(px(-constraints.scroll_offset), px(0.0)),
        BottomToTop => Offset::new(
            px(0.0),
            px(geometry.paint_extent + constraints.scroll_offset - child_main_extent),
        ),
        RightToLeft => Offset::new(
            px(geometry.paint_extent + constraints.scroll_offset - child_main_extent),
            px(0.0),
        ),
    }
}

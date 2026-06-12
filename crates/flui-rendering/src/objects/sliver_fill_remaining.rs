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

use crate::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
};

/// A Sliver-protocol adapter that sizes one non-scrollable Box child to fill
/// the remaining viewport space, but expands to the child's intrinsic extent
/// when the child is larger.
#[derive(Debug, Clone)]
pub struct RenderSliverFillRemaining {
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl RenderSliverFillRemaining {
    /// Creates a non-scroll fill-remaining sliver with no laid-out geometry yet.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Default for RenderSliverFillRemaining {
    fn default() -> Self {
        Self::new()
    }
}

impl Diagnosticable for RenderSliverFillRemaining {}
impl PaintEffectsCapability for RenderSliverFillRemaining {}
impl SemanticsCapability for RenderSliverFillRemaining {}
impl HotReloadCapability for RenderSliverFillRemaining {}

impl RenderSliver for RenderSliverFillRemaining {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        let mut extent = (self.constraints.viewport_main_axis_extent
            - self.constraints.preceding_scroll_extent)
            .max(0.0);

        if ctx.child_count() > 0 {
            let child_extent = child_max_intrinsic_main_extent(ctx, &self.constraints);
            extent = extent.max(child_extent);
            ctx.layout_box_child(0, self.constraints.as_box_constraints(extent, extent, None));
        }

        let painted_child_size = self.calculate_paint_offset(&self.constraints, 0.0, extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, extent);
        let geometry = SliverGeometry {
            scroll_extent: extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: painted_child_size,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: extent > self.constraints.remaining_paint_extent
                || self.constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        if ctx.child_count() > 0 {
            ctx.position_child(0, child_paint_offset(&self.constraints, &geometry));
        }
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

/// Non-scroll fill-remaining sliver that also includes overscroll in its
/// maximum paint extent.
#[derive(Debug, Clone)]
pub struct RenderSliverFillRemainingAndOverscroll {
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl RenderSliverFillRemainingAndOverscroll {
    /// Creates an overscroll-aware fill-remaining sliver.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Default for RenderSliverFillRemainingAndOverscroll {
    fn default() -> Self {
        Self::new()
    }
}

impl Diagnosticable for RenderSliverFillRemainingAndOverscroll {}
impl PaintEffectsCapability for RenderSliverFillRemainingAndOverscroll {}
impl SemanticsCapability for RenderSliverFillRemainingAndOverscroll {}
impl HotReloadCapability for RenderSliverFillRemainingAndOverscroll {}

impl RenderSliver for RenderSliverFillRemainingAndOverscroll {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        let mut extent = (self.constraints.viewport_main_axis_extent
            - self.constraints.preceding_scroll_extent)
            .max(0.0);
        let mut max_extent =
            (self.constraints.remaining_paint_extent - self.constraints.overlap.min(0.0)).max(0.0);
        let mut child_main_extent = extent;

        if ctx.child_count() > 0 {
            let child_extent = child_max_intrinsic_main_extent(ctx, &self.constraints);
            extent = extent.max(child_extent);
            max_extent = max_extent.max(extent);
            let child_size = ctx.layout_box_child(
                0,
                self.constraints
                    .as_box_constraints(extent, max_extent, None),
            );
            child_main_extent = size_main_axis_extent(child_size, &self.constraints);
        }

        let painted_child_size = max_extent.min(self.constraints.remaining_paint_extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, extent);
        let geometry = SliverGeometry {
            scroll_extent: extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: max_extent,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: extent > self.constraints.remaining_paint_extent
                || self.constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        if ctx.child_count() > 0 {
            ctx.position_child(
                0,
                child_paint_offset_for_extent(&self.constraints, &geometry, child_main_extent),
            );
        }
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

/// A Sliver-protocol adapter that sizes one Box child to the remaining paint
/// extent of the viewport.
#[derive(Debug, Clone)]
pub struct RenderSliverFillRemainingWithScrollable {
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl RenderSliverFillRemainingWithScrollable {
    /// Creates a fill-remaining sliver with no laid-out geometry yet.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Default for RenderSliverFillRemainingWithScrollable {
    fn default() -> Self {
        Self::new()
    }
}

impl Diagnosticable for RenderSliverFillRemainingWithScrollable {}
impl PaintEffectsCapability for RenderSliverFillRemainingWithScrollable {}
impl SemanticsCapability for RenderSliverFillRemainingWithScrollable {}
impl HotReloadCapability for RenderSliverFillRemainingWithScrollable {}

impl RenderSliver for RenderSliverFillRemainingWithScrollable {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        let extent = self.constraints.remaining_paint_extent - self.constraints.overlap.min(0.0);
        let cache_extent = self.calculate_cache_offset(
            &self.constraints,
            0.0,
            self.constraints.viewport_main_axis_extent,
        );

        if ctx.child_count() > 0 {
            let max_extent = if extent == 0.0 && cache_extent > 0.0 {
                cache_extent
            } else {
                extent
            };
            ctx.layout_box_child(
                0,
                self.constraints
                    .as_box_constraints(extent, max_extent, None),
            );
        }

        let painted_child_size = self.calculate_paint_offset(&self.constraints, 0.0, extent);
        let geometry = SliverGeometry {
            scroll_extent: self.constraints.viewport_main_axis_extent,
            paint_extent: painted_child_size,
            layout_extent: painted_child_size,
            max_paint_extent: painted_child_size,
            cache_extent,
            hit_test_extent: painted_child_size,
            visible: painted_child_size > 0.0,
            has_visual_overflow: extent > self.constraints.remaining_paint_extent
                || self.constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        if ctx.child_count() > 0 {
            ctx.position_child(0, child_paint_offset(&self.constraints, &geometry));
        }
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

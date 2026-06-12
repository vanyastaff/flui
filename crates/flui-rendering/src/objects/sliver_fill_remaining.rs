//! `RenderSliverFillRemainingWithScrollable` — single Box child sliver that
//! fills the remaining viewport paint extent.
//!
//! This is the scroll-body variant of Flutter's `SliverFillRemaining`: it does
//! not ask the Box child for intrinsic size, and therefore fits FLUI's current
//! Sliver -> Box layout bridge without adding a new intrinsic query channel to
//! `SliverLayoutContext`.

use flui_foundation::Diagnosticable;
use flui_tree::Single;
use flui_types::{
    Offset,
    geometry::px,
    layout::{AxisDirection, AxisDirection::*},
};

use crate::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
};

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

    #[inline]
    fn child_paint_offset(constraints: &SliverConstraints, geometry: &SliverGeometry) -> Offset {
        match apply_growth_direction_to_axis_direction(
            constraints.axis_direction,
            constraints.growth_direction,
        ) {
            TopToBottom => Offset::new(px(0.0), px(-constraints.scroll_offset)),
            LeftToRight => Offset::new(px(-constraints.scroll_offset), px(0.0)),
            BottomToTop => Offset::new(
                px(0.0),
                px(geometry.paint_extent + constraints.scroll_offset - geometry.scroll_extent),
            ),
            RightToLeft => Offset::new(
                px(geometry.paint_extent + constraints.scroll_offset - geometry.scroll_extent),
                px(0.0),
            ),
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
            let child_paint_offset = Self::child_paint_offset(&self.constraints, &geometry);
            ctx.position_child(0, child_paint_offset);
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

const fn apply_growth_direction_to_axis_direction(
    axis_direction: AxisDirection,
    growth_direction: GrowthDirection,
) -> AxisDirection {
    match growth_direction {
        GrowthDirection::Forward => axis_direction,
        GrowthDirection::Reverse => axis_direction.opposite(),
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

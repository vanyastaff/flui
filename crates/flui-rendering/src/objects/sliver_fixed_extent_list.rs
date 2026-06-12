//! `RenderSliverFixedExtentList` — Box children with one fixed main-axis extent.

use flui_foundation::Diagnosticable;
use flui_tree::Variable;
use flui_types::{
    Offset,
    geometry::px,
    layout::{Axis, AxisDirection::*},
};

use crate::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
};

/// A sliver that lays out each direct Box child with the same main-axis extent.
///
/// This is the eager, attached-child FLUI counterpart of Flutter's
/// `RenderSliverFixedExtentList`. Lazy child creation and garbage collection
/// remain deferred to the future multi-box-adaptor layer; attached children are
/// laid out eagerly with fixed extents.
#[derive(Debug, Clone)]
pub struct RenderSliverFixedExtentList {
    item_extent: f32,
    constraints: SliverConstraints,
    geometry: SliverGeometry,
    child_count: usize,
}

impl RenderSliverFixedExtentList {
    /// Creates a fixed-extent sliver list.
    ///
    /// # Panics
    ///
    /// Panics when `item_extent` is not finite or is less than or equal to
    /// zero.
    #[inline]
    #[must_use]
    pub fn new(item_extent: f32) -> Self {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and greater than zero"
        );
        Self {
            item_extent,
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
            child_count: 0,
        }
    }

    /// Main-axis extent assigned to each child.
    #[inline]
    #[must_use]
    pub const fn item_extent(&self) -> f32 {
        self.item_extent
    }

    /// Updates the main-axis extent assigned to each child.
    ///
    /// # Panics
    ///
    /// Panics when `item_extent` is not finite or is less than or equal to
    /// zero.
    #[inline]
    pub fn set_item_extent(&mut self, item_extent: f32) {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and greater than zero"
        );
        self.item_extent = item_extent;
    }
}

impl Diagnosticable for RenderSliverFixedExtentList {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_double("item_extent", self.item_extent, Some("px"));
    }
}
impl PaintEffectsCapability for RenderSliverFixedExtentList {}
impl SemanticsCapability for RenderSliverFixedExtentList {}
impl HotReloadCapability for RenderSliverFixedExtentList {}

impl RenderSliver for RenderSliverFixedExtentList {
    type Arity = Variable;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        self.child_count = ctx.child_count();

        for index in 0..self.child_count {
            ctx.layout_box_child(
                index,
                self.constraints
                    .as_box_constraints(self.item_extent, self.item_extent, None),
            );
        }

        let scroll_extent = self.item_extent * self.child_count as f32;
        let paint_extent = self.calculate_paint_offset(&self.constraints, 0.0, scroll_extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, scroll_extent);
        let geometry = SliverGeometry {
            scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
            cache_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: scroll_extent > self.constraints.remaining_paint_extent
                || self.constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };

        for index in 0..self.child_count {
            let layout_offset = self.item_extent * index as f32;
            ctx.position_child(
                index,
                child_paint_offset(
                    &self.constraints,
                    &geometry,
                    layout_offset,
                    self.item_extent,
                ),
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

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        if self.geometry.visible {
            ctx.paint_children();
        }
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        for index in (0..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(index) {
                return true;
            }
        }
        false
    }
}

fn child_paint_offset(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    layout_offset: f32,
    child_main_extent: f32,
) -> Offset {
    let child_main_axis_position = layout_offset - constraints.scroll_offset;
    let main_axis_delta = if crate::constraints::right_way_up(
        constraints.axis_direction,
        constraints.growth_direction,
    ) {
        child_main_axis_position
    } else {
        geometry.paint_extent - child_main_extent - child_main_axis_position
    };

    match constraints.axis_direction.axis() {
        Axis::Horizontal => Offset::new(px(main_axis_delta), px(0.0)),
        Axis::Vertical => Offset::new(px(0.0), px(main_axis_delta)),
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

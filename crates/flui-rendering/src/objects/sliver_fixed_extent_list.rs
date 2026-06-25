//! `RenderSliverFixedExtentList` — Box children with one fixed main-axis extent.

use flui_foundation::Diagnosticable;
use flui_tree::Variable;
use flui_types::geometry::px;

use crate::{
    constraints::{SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

/// A sliver that lays out each direct Box child with the same main-axis extent.
///
/// This is the eager, attached-child FLUI counterpart of Flutter's
/// `RenderSliverFixedExtentList`. Lazy child creation and garbage collection
/// remain deferred to the future multi-box-adaptor layer; attached children are
/// laid out eagerly with fixed extents.
///
/// 2B field dedup: incoming constraints live only in `perform_layout` and
/// the committed `geometry` lives solely on `RenderState<SliverProtocol>`;
/// `perform_layout` returns its geometry directly and the visibility cull
/// is owned by the pipeline paint driver. `child_count` is retained because
/// the `&self`-only `hit_test` walks the attached children in reverse.
#[derive(Debug, Clone)]
pub struct RenderSliverFixedExtentList {
    item_extent: f32,
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

impl RenderSliver for RenderSliverFixedExtentList {
    type Arity = Variable;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        self.child_count = ctx.child_count();

        for index in 0..self.child_count {
            ctx.layout_box_child(
                index,
                constraints.as_box_constraints(self.item_extent, self.item_extent, None),
            );
        }

        let scroll_extent = self.item_extent * self.child_count as f32;
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, scroll_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, scroll_extent);
        let geometry = SliverGeometry {
            scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
            cache_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: scroll_extent > constraints.remaining_paint_extent
                || constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };

        for index in 0..self.child_count {
            let layout_offset = self.item_extent * index as f32;
            ctx.position_child(
                index,
                child_paint_offset(
                    &constraints,
                    &geometry,
                    px(layout_offset),
                    px(self.item_extent),
                ),
            );
        }

        geometry
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        ctx.paint_children();
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

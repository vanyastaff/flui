//! `RenderSliverFillViewport` — Box children with viewport-fraction extents.

use flui_foundation::Diagnosticable;
use flui_tree::Variable;
use flui_types::geometry::px;

use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

/// A sliver that sizes each direct Box child to a fraction of the viewport's
/// main-axis extent.
///
/// This is the direct-child FLUI counterpart of Flutter's
/// `RenderSliverFillViewport`. Lazy child creation remains deferred to the
/// future multi-box-adaptor layer; attached children are laid out eagerly.
///
/// 2B field dedup: incoming constraints live only in `perform_layout` and
/// the committed `geometry` lives solely on `RenderState<SliverProtocol>`;
/// `perform_layout` returns its geometry directly and the visibility cull
/// is owned by the pipeline paint driver. `child_count` is retained because
/// the `&self`-only `hit_test` walks the attached children in reverse.
#[derive(Debug, Clone)]
pub struct RenderSliverFillViewport {
    viewport_fraction: f32,
    allow_implicit_scrolling: bool,
    child_count: usize,
}

impl RenderSliverFillViewport {
    /// Creates a fill-viewport sliver.
    ///
    /// # Panics
    ///
    /// Panics when `viewport_fraction <= 0.0`.
    #[inline]
    #[must_use]
    pub fn new(viewport_fraction: f32) -> Self {
        assert!(
            viewport_fraction > 0.0,
            "viewport_fraction must be greater than zero"
        );
        Self {
            viewport_fraction,
            allow_implicit_scrolling: true,
            child_count: 0,
        }
    }

    /// Fraction of the viewport occupied by each child in the main axis.
    #[inline]
    #[must_use]
    pub const fn viewport_fraction(&self) -> f32 {
        self.viewport_fraction
    }

    /// Updates the viewport fraction.
    ///
    /// # Panics
    ///
    /// Panics when `viewport_fraction <= 0.0`.
    #[inline]
    pub fn set_viewport_fraction(&mut self, viewport_fraction: f32) {
        assert!(
            viewport_fraction > 0.0,
            "viewport_fraction must be greater than zero"
        );
        self.viewport_fraction = viewport_fraction;
    }

    /// Whether all children should be available to semantics even when outside
    /// the visible viewport.
    #[inline]
    #[must_use]
    pub const fn allow_implicit_scrolling(&self) -> bool {
        self.allow_implicit_scrolling
    }

    /// Sets the implicit-scrolling semantics flag.
    #[inline]
    pub const fn set_allow_implicit_scrolling(&mut self, allow: bool) {
        self.allow_implicit_scrolling = allow;
    }

    #[inline]
    fn item_extent(&self, constraints: &SliverConstraints) -> f32 {
        (constraints.viewport_main_axis_extent * self.viewport_fraction).max(0.0)
    }
}

impl Default for RenderSliverFillViewport {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl Diagnosticable for RenderSliverFillViewport {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_double("viewport_fraction", self.viewport_fraction, None);
    }
}
impl RenderSliver for RenderSliverFillViewport {
    type Arity = Variable;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        self.child_count = ctx.child_count();
        let item_extent = self.item_extent(&constraints);

        for index in 0..self.child_count {
            ctx.layout_box_child(
                index,
                constraints.as_box_constraints(item_extent, item_extent, None),
            );
        }

        let scroll_extent = item_extent * self.child_count as f32;
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
            let layout_offset = item_extent * index as f32;
            ctx.position_child(
                index,
                child_paint_offset(&constraints, &geometry, px(layout_offset), px(item_extent)),
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

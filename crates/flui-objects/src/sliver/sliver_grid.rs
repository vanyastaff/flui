//! `RenderSliverGrid` — 2-D grid sliver: `cross_axis_count` items per main-axis row.

use std::sync::Arc;

use flui_foundation::Diagnosticable;
use flui_tree::Variable;
use flui_types::geometry::px;

use flui_rendering::{
    constraints::{SliverGeometry, grid_child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    delegates::SliverGridDelegate,
    parent_data::SliverGridParentData,
    traits::RenderSliver,
};

/// A sliver that arranges Box children in a 2-D grid.
///
/// Layout geometry (tile size, row count, cross-axis column positions) is
/// delegated to a [`SliverGridDelegate`], which returns a [`SliverGridLayout`]
/// for the incoming [`SliverConstraints`].  Every tile is constrained tightly
/// to the delegate-specified size — tiles do not measure themselves.
///
/// # Windowed layout
///
/// Only the in-band rows (those whose scroll offset intersects the
/// cache-extended viewport window) are laid out per frame.  Out-of-band
/// children are skipped by the pipeline; their slots remain allocated in the
/// arena but receive no layout or paint call.
///
/// # Flutter parity
///
/// This is the eager (all-children-attached) counterpart of Flutter's
/// `RenderSliverGrid`.  Lazy child creation and garbage collection remain
/// deferred to the future multi-box-adaptor layer.
///
/// [`SliverGridDelegate`]: flui_rendering::delegates::SliverGridDelegate
/// [`SliverGridLayout`]: flui_rendering::delegates::SliverGridLayout
/// [`SliverConstraints`]: flui_rendering::constraints::SliverConstraints
pub struct RenderSliverGrid {
    grid_delegate: Arc<dyn SliverGridDelegate>,
    child_count: usize,
}

impl RenderSliverGrid {
    /// Creates a new grid sliver driven by `grid_delegate`.
    #[must_use]
    pub fn new(grid_delegate: Arc<dyn SliverGridDelegate>) -> Self {
        Self {
            grid_delegate,
            child_count: 0,
        }
    }

    /// Replaces the grid delegate.
    ///
    /// `should_relayout` is consulted for documentation and diagnostic
    /// purposes.  In the eager pipeline model the next frame always re-runs
    /// `perform_layout`, so no explicit "mark needs layout" call is required.
    pub fn set_grid_delegate(&mut self, new_delegate: Arc<dyn SliverGridDelegate>) {
        let _relayout_needed = new_delegate.should_relayout(&*self.grid_delegate);
        self.grid_delegate = new_delegate;
    }

    /// Returns the current grid delegate.
    #[must_use]
    pub fn grid_delegate(&self) -> &dyn SliverGridDelegate {
        &*self.grid_delegate
    }
}

impl std::fmt::Debug for RenderSliverGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverGrid")
            .field("grid_delegate", &self.grid_delegate)
            .field("child_count", &self.child_count)
            .finish_non_exhaustive()
    }
}

impl Diagnosticable for RenderSliverGrid {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_int("child_count", self.child_count as i64, None);
    }
}

impl RenderSliver for RenderSliverGrid {
    type Arity = Variable;
    type ParentData = SliverGridParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        self.child_count = ctx.child_count();

        if self.child_count == 0 {
            return SliverGeometry::ZERO;
        }

        let layout = self.grid_delegate.get_layout(constraints);

        // Effective scroll window: expand by cache origin so pre-cache rows
        // are also laid out (mirror of Flutter's performLayout lines 599-603).
        let effective_scroll_offset = constraints.scroll_offset + constraints.cache_origin;
        let target_end = effective_scroll_offset + constraints.remaining_cache_extent;

        let first_in_band = layout.get_min_child_index_for_scroll_offset(effective_scroll_offset);
        let last_in_band = layout
            .get_max_child_index_for_scroll_offset(target_end)
            .min(self.child_count - 1);

        let scroll_extent = layout.compute_max_scroll_offset(self.child_count);

        // Tight box constraints: the delegate forces both axes (tiles never
        // choose their own size).
        let tile_constraints = constraints.as_box_constraints(
            layout.child_main_axis_extent,
            layout.child_main_axis_extent,
            Some(layout.child_cross_axis_extent),
        );

        let leading_scroll_offset = layout.get_scroll_offset_of_child(first_in_band);
        let mut trailing_scroll_offset = leading_scroll_offset;

        for index in first_in_band..=last_in_band {
            if index >= self.child_count {
                break;
            }
            let child_scroll_offset = layout.get_scroll_offset_of_child(index);
            ctx.layout_box_child(index, tile_constraints);
            let tile_trailing = child_scroll_offset + layout.child_main_axis_extent;
            if tile_trailing > trailing_scroll_offset {
                trailing_scroll_offset = tile_trailing;
            }
        }

        // Oracle flutter/rendering/sliver_grid.dart:700-719.
        let from = constraints.scroll_offset.min(leading_scroll_offset);
        let paint_extent = self.calculate_paint_offset(&constraints, from, trailing_scroll_offset);
        let cache_extent = self.calculate_cache_offset(
            &constraints,
            leading_scroll_offset,
            trailing_scroll_offset,
        );
        let geometry = SliverGeometry {
            scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
            cache_extent,
            hit_test_extent: paint_extent,
            has_visual_overflow: scroll_extent > paint_extent
                || constraints.scroll_offset > 0.0
                || constraints.overlap != 0.0,
            ..SliverGeometry::ZERO
        };

        // Position pass: commit paint offset and parent data for each in-band tile.
        for index in first_in_band..=last_in_band {
            if index >= self.child_count {
                break;
            }
            let child_scroll_offset = layout.get_scroll_offset_of_child(index);
            let cross_axis_offset = layout.get_cross_axis_offset_of_child(index);

            ctx.position_child(
                index,
                grid_child_paint_offset(
                    &constraints,
                    &geometry,
                    px(child_scroll_offset),
                    px(layout.child_main_axis_extent),
                    px(cross_axis_offset),
                ),
            );

            if let Some(parent_data) = ctx.child_parent_data_mut(index) {
                parent_data.index = index;
                parent_data.layout_offset = child_scroll_offset;
                parent_data.cross_axis_offset = cross_axis_offset;
            }
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

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
/// arena but receive no layout or paint call — and, because this sliver
/// never evicts them, no eventual cleanup either.  `hit_test` therefore
/// tests only the last-committed window (`laid_out_band`), not every
/// arena-resident child: an out-of-band child's committed offset can be
/// arbitrarily stale (positioned on some earlier pass, at a scroll offset
/// that no longer applies), so testing it against the current pointer
/// position would risk routing the event to content that is not, in fact,
/// what currently paints there.
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
    /// Inclusive `[first, last]` child-index window that `perform_layout`
    /// actually laid out and positioned on its most recent SUCCESSFUL pass —
    /// the exact `first_in_band..=last_in_band` range the position loop
    /// iterates. `None` before the first layout pass, or when that pass's
    /// computed window was empty (e.g. scrolled past all content).
    ///
    /// `hit_test` gates on this instead of `0..child_count`: this sliver is
    /// eager (every child stays arena-resident forever, see the module doc)
    /// and windowed (only in-band children are laid out per pass), so an
    /// index outside the last-committed window still carries whatever
    /// `RenderState.offset` it was positioned at on some earlier pass —
    /// stale, and not necessarily where it paints now (or whether it paints
    /// at all). Retaining the exact window `perform_layout` computed, rather
    /// than re-deriving "what is in band" a second time inside `hit_test`
    /// from the delegate and the current scroll offset, means the two can
    /// never drift apart: there is exactly one computation of the window,
    /// and `hit_test` reads its committed result.
    ///
    /// **The write site is load-bearing, not incidental**: `perform_layout`
    /// commits this field as its LAST statement, after both the layout and
    /// position loops have fully run — never earlier. Both loops call into
    /// caller-supplied `grid_delegate` code that can panic; the pipeline
    /// catches that panic in a `catch_unwind` around this render object's
    /// own `perform_layout` call
    /// (`crates/flui-rendering/src/pipeline/owner/subtree_arena.rs`), which
    /// unwinds through this function WITHOUT rolling back field writes it
    /// already made. Committing at the end means an interrupted pass leaves
    /// this field exactly as the last SUCCESSFUL pass left it — a window
    /// whose offsets that pass actually finished writing — rather than a
    /// window the interrupted pass merely started positioning. Moving the
    /// commit any earlier reopens the stale-rect class this field exists to
    /// close: `hit_test` would trust a band whose offsets a panic left only
    /// partially refreshed.
    laid_out_band: Option<(usize, usize)>,
}

impl RenderSliverGrid {
    /// Creates a new grid sliver driven by `grid_delegate`.
    #[must_use]
    pub fn new(grid_delegate: Arc<dyn SliverGridDelegate>) -> Self {
        Self {
            grid_delegate,
            child_count: 0,
            laid_out_band: None,
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
            .field("laid_out_band", &self.laid_out_band)
            .finish_non_exhaustive()
    }
}

impl Diagnosticable for RenderSliverGrid {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_int("child_count", self.child_count as i64, None);
        match self.laid_out_band {
            Some((first, last)) => {
                properties.add_int("laid_out_band_first", first as i64, None);
                properties.add_int("laid_out_band_last", last as i64, None);
            }
            None => {
                properties.add_flag("laid_out_band", false, "no band laid out yet");
            }
        }
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
            self.laid_out_band = None;
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
            // Every sibling sliver (`RenderSliverFixedExtentList`,
            // `RenderSliverPadding`, `RenderSliverFillViewport`, ...) sets this
            // explicitly; omitting it left `visible` at `SliverGeometry::ZERO`'s
            // `false` unconditionally. The viewport's own hit-test walk gates on
            // it (`sliver_child_is_visible` in
            // `flui-rendering/src/pipeline/owner/accessors.rs`), so a grid with
            // real on-screen content was never reachable by any pointer event —
            // confirmed while porting `SliverGrid`'s hit-test parity cases.
            visible: paint_extent > 0.0,
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

        // Commit the window `hit_test` will read — deliberately the LAST
        // thing this function does, after both loops above (which call into
        // the caller-supplied `grid_delegate` and the pipeline's own child
        // layout/position machinery) have run to completion. `None` when the
        // window is empty (`first_in_band > last_in_band`, e.g. scrolled
        // past all content).
        //
        // Load-bearing ordering, not a style choice: this crate's pipeline
        // wraps each render object's `perform_layout` call in `catch_unwind`
        // (`crates/flui-rendering/src/pipeline/owner/subtree_arena.rs`), so a
        // panic from user code reachable in either loop above (most notably
        // `self.grid_delegate`'s `SliverGridLayout` methods, called
        // per-index) unwinds straight through this function — Rust does not
        // roll back the field writes this function already made, only skips
        // the ones after the panic point. Committing here means an
        // interrupted pass leaves `laid_out_band` exactly as the LAST
        // successful pass left it: still describing a fully-positioned
        // window whose offsets that pass actually wrote, not a window this
        // (aborted) pass merely intended to write. Committing any earlier —
        // e.g. right after computing `first_in_band`/`last_in_band`, before
        // either loop ran — would let a mid-loop panic leave `laid_out_band`
        // pointing at a window whose offsets were only partially (or never)
        // refreshed this pass, reopening the exact stale-rect class this
        // field exists to close.
        self.laid_out_band =
            (first_in_band <= last_in_band).then_some((first_in_band, last_in_band));

        geometry
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        ctx.paint_children();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        // Gated on `laid_out_band`, not `0..child_count`: this sliver never
        // evicts out-of-band children (see the field doc on `laid_out_band`),
        // so an index outside the last-committed window may still carry a
        // `RenderState.offset` from an earlier, no-longer-current pass. Only
        // the committed window is guaranteed fresh this frame.
        let Some((first_in_band, last_in_band)) = self.laid_out_band else {
            return false;
        };
        for index in (first_in_band..=last_in_band).rev() {
            if ctx.hit_test_child_at_layout_offset(index) {
                return true;
            }
        }
        false
    }
}

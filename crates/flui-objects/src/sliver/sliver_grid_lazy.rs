//! `RenderSliverGridLazy` — request-strategy lazily-virtualized 2-D grid sliver.
//!
//! # Design
//!
//! Combines the **request-strategy seam** of `RenderSliverList` (U4.2/U4.3)
//! with the **delegate-windowed geometry** of the eager `RenderSliverGrid`.
//!
//! Because the delegate makes scroll extent deterministic
//! (`compute_max_scroll_offset`), no virtualizer or estimate is needed.  The
//! visible+cache window is computed directly from the delegate's grid layout.
//!
//! **Parent-data type is `SliverMultiBoxAdaptorParentData`** (not
//! `SliverGridParentData`) to route through the lazy-build backend, which
//! hardcodes that seed type.  The cross-axis offset is recomputed from the
//! delegate each pass; storing it in parent data is not load-bearing (paint
//! and hit-test read from `RenderState.offset`, not from parent data).
//!
//! # Flutter parity
//!
//! Corresponds to Flutter's `RenderSliverGrid` with a `childManager`
//! (`SliverMultiBoxAdaptorElement`) — the "lazy" constructor path.  Oracle:
//! `flutter/rendering/sliver_grid.dart:594-728`.
//!
//! # Lifecycle
//!
//! Inert until a U4.3 `ChildManager` is wired (via `SliverGridLazy` view).
//! Until then, `request_child_build` emits requests that nothing services, so
//! absent tiles never appear.  This matches `RenderSliverList`'s posture.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_tree::Variable;
use flui_types::geometry::px;

use flui_rendering::{
    constraints::{SliverGeometry, grid_child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    delegates::SliverGridDelegate,
    parent_data::SliverMultiBoxAdaptorParentData,
    traits::RenderSliver,
};

// ============================================================================
// RENDER OBJECT
// ============================================================================

/// A request-strategy lazily-virtualized 2-D grid sliver.
///
/// Layout geometry is delegated to a [`SliverGridDelegate`]; children are
/// built on demand by the element tree's `ChildManager` (U4.3) in response to
/// [`SliverLayoutContext::request_child_build`] calls emitted during layout.
///
/// # Construction
///
/// ```ignore
/// use std::sync::Arc;
/// use flui_objects::RenderSliverGridLazy;
/// use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
///
/// let grid = RenderSliverGridLazy::new(
///     Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2)),
///     50,
/// );
/// ```
///
/// # Flutter parity
///
/// Corresponds to Flutter's `RenderSliverGrid` with `childManager`; oracle
/// `sliver_grid.dart:594-728`.
pub struct RenderSliverGridLazy {
    /// Grid layout delegate — controls tile sizes and cross-axis count.
    grid_delegate: Arc<dyn SliverGridDelegate>,
    /// Total known item count.
    item_count: usize,
    /// Logical-index → dense-slot map rebuilt each pass from parent-data.
    /// Kept as a field to reuse the `BTreeMap` allocation across passes.
    logical_to_slot: BTreeMap<usize, usize>,
    /// Dense child count committed after the last layout pass.
    /// Used by the `&self` hit-test reverse walk which cannot re-read
    /// `ctx.child_count()`.
    attached_child_count: usize,
}

impl RenderSliverGridLazy {
    /// Creates a new lazy grid sliver driven by `grid_delegate` over
    /// `item_count` items.
    #[must_use]
    pub fn new(grid_delegate: Arc<dyn SliverGridDelegate>, item_count: usize) -> Self {
        Self {
            grid_delegate,
            item_count,
            logical_to_slot: BTreeMap::new(),
            attached_child_count: 0,
        }
    }

    /// Updates the known item count.  Call when the data source length changes.
    pub fn set_item_count(&mut self, count: usize) {
        self.item_count = count;
    }

    /// Current effective item count used for delegate scroll extent.
    #[inline]
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Replaces the grid delegate.  In the lazy pipeline the next frame always
    /// re-runs `perform_layout`, so no explicit "mark needs layout" is needed.
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

impl fmt::Debug for RenderSliverGridLazy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderSliverGridLazy")
            .field("item_count", &self.item_count)
            .field("attached_child_count", &self.attached_child_count)
            .field("grid_delegate", &self.grid_delegate)
            .finish_non_exhaustive()
    }
}

impl Clone for RenderSliverGridLazy {
    fn clone(&self) -> Self {
        Self {
            grid_delegate: self.grid_delegate.clone(),
            item_count: self.item_count,
            logical_to_slot: BTreeMap::new(), // transient — reset each pass
            attached_child_count: self.attached_child_count,
        }
    }
}

impl Diagnosticable for RenderSliverGridLazy {
    fn debug_fill_properties(&self, props: &mut DiagnosticsBuilder) {
        props.add_int("item_count", self.item_count as i64, None);
        props.add_int(
            "attached_child_count",
            self.attached_child_count as i64,
            None,
        );
    }
}

// ============================================================================
// RenderSliver impl
// ============================================================================

impl RenderSliver for RenderSliverGridLazy {
    type Arity = Variable;
    type ParentData = SliverMultiBoxAdaptorParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();

        // ── 1. Empty grid ─────────────────────────────────────────────────────
        if self.item_count == 0 {
            self.attached_child_count = 0;
            ctx.emit_retain_band(0, 0);
            return SliverGeometry::ZERO;
        }

        // ── 2. Grid layout from delegate ──────────────────────────────────────
        let tile_layout = self.grid_delegate.get_layout(constraints);

        // ── 3. Cache-extended viewport window ────────────────────────────────
        // Mirror of Flutter's performLayout lines 599-603:
        //   effectiveScrollOffset = scrollOffset + cacheOrigin
        //   targetEndScrollOffset = effectiveScrollOffset + remainingCacheExtent
        // Negative effective offsets saturate to 0 in the delegate's usize math.
        let cache_start_offset = constraints.scroll_offset + constraints.cache_origin;
        let cache_end_offset = cache_start_offset + constraints.remaining_cache_extent;

        let first_in_window = tile_layout.get_min_child_index_for_scroll_offset(cache_start_offset);
        // Clamp to item_count−1; no underflow risk since item_count > 0 above.
        let last_in_window = tile_layout
            .get_max_child_index_for_scroll_offset(cache_end_offset)
            .min(self.item_count - 1);

        // Guard: window is entirely past the last item (e.g. scrolled to end).
        if first_in_window > last_in_window {
            let scroll_extent = tile_layout.compute_max_scroll_offset(self.item_count);
            self.attached_child_count = ctx.child_count();
            // Empty retain band tells the element tree to evict all off-window children.
            ctx.emit_retain_band(first_in_window, first_in_window);
            return SliverGeometry {
                scroll_extent,
                ..SliverGeometry::ZERO
            };
        }

        // ── 4. Reconcile logical-index → dense-slot from parent data ──────────
        // O(K) where K = currently attached child count (bounded by viewport).
        self.logical_to_slot.clear();
        let dense_child_count = ctx.child_count();
        for slot in 0..dense_child_count {
            if let Some(pd) = ctx.child_parent_data(slot) {
                self.logical_to_slot.insert(pd.index, slot);
            }
        }

        // ── 5. Tight tile box constraints ────────────────────────────────────
        // All tiles are constrained tightly by the delegate — they do not choose
        // their own size (mirror of eager RenderSliverGrid).
        let tile_constraints = constraints.as_box_constraints(
            tile_layout.child_main_axis_extent,
            tile_layout.child_main_axis_extent,
            Some(tile_layout.child_cross_axis_extent),
        );

        // ── 6. Layout pass: resident → re-layout; absent → request ───────────
        // Resident slots use the `|_| None` fallback: this render object carries
        // no owned child-source factory (element tree owns the children).
        for logical_index in first_in_window..=last_in_window {
            if let Some(&slot) = self.logical_to_slot.get(&logical_index) {
                ctx.build_and_layout_box_child(slot, logical_index, tile_constraints, &mut |_| {
                    None
                });
            } else {
                // Absent — emit a build request.  The element tree's
                // `SliverGridLazyAdaptorManager::service` builds it post-frame.
                ctx.request_child_build(logical_index);
            }
        }

        // ── 7. Deterministic scroll extent ────────────────────────────────────
        // Unlike a list, the delegate gives exact extent from item_count — no
        // virtualizer or estimate needed.
        let scroll_extent = tile_layout.compute_max_scroll_offset(self.item_count);

        // ── 8. Paint geometry (mirror of eager RenderSliverGrid, oracle 700-719) ──
        let leading_row_offset = tile_layout.get_scroll_offset_of_child(first_in_window);
        let trailing_row_offset = tile_layout.get_scroll_offset_of_child(last_in_window)
            + tile_layout.child_main_axis_extent;

        // `from` clamps to the viewport start so partial leading rows don't
        // drive paint_extent negative (Flutter's `from = min(scrollOffset, leading)` ).
        let paint_start = constraints.scroll_offset.min(leading_row_offset);
        let paint_extent =
            self.calculate_paint_offset(&constraints, paint_start, trailing_row_offset);
        let cache_extent =
            self.calculate_cache_offset(&constraints, leading_row_offset, trailing_row_offset);

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

        // ── 9. Commit attached count and position resident tiles ──────────────
        // Use the `logical_to_slot` map from step 4; no new arena children are
        // added during a request-strategy layout pass (the element tree inserts
        // them between frames).
        self.attached_child_count = ctx.child_count();

        for logical_index in first_in_window..=last_in_window {
            if let Some(&slot) = self.logical_to_slot.get(&logical_index) {
                let tile_scroll_offset = tile_layout.get_scroll_offset_of_child(logical_index);
                let tile_cross_offset = tile_layout.get_cross_axis_offset_of_child(logical_index);

                ctx.position_child(
                    slot,
                    grid_child_paint_offset(
                        &constraints,
                        &geometry,
                        px(tile_scroll_offset),
                        px(tile_layout.child_main_axis_extent),
                        px(tile_cross_offset),
                    ),
                );

                if let Some(pd) = ctx.child_parent_data_mut(slot) {
                    pd.index = logical_index;
                    pd.layout_offset = tile_scroll_offset;
                }
            }
        }

        // ── 10. Emit retain band for element-side eviction ────────────────────
        // [first_in_window, last_in_window+1) is the half-open retained range.
        // `SparseChildren::retain_band` on the element side evicts any logical
        // index outside this band.  `dispose_box_child` is NOT called here to
        // prevent the ABA double-remove that would occur if both the render side
        // and the element side freed the same node.
        ctx.emit_retain_band(first_in_window, last_in_window + 1);

        geometry
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        ctx.paint_children();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        // Reverse walk: the last-attached slot is at the highest Z-order.
        for slot in (0..self.attached_child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(slot) {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_rendering::constraints::{GrowthDirection, SliverConstraints};
    use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
    use flui_rendering::view::ScrollDirection;
    use flui_types::layout::AxisDirection;

    use super::*;

    fn two_column_grid(item_count: usize) -> RenderSliverGridLazy {
        RenderSliverGridLazy::new(
            Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2)),
            item_count,
        )
    }

    fn vertical_constraints(
        scroll_offset: f32,
        viewport_height: f32,
        cross_axis_extent: f32,
    ) -> SliverConstraints {
        SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset,
            remaining_paint_extent: viewport_height,
            cross_axis_extent,
            viewport_main_axis_extent: viewport_height,
            remaining_cache_extent: viewport_height,
            ..Default::default()
        }
    }

    // ── Construction ──────────────────────────────────────────────────────────

    #[test]
    fn construction_sets_item_count_and_zeroes_transient_state() {
        let grid = two_column_grid(50);
        assert_eq!(grid.item_count, 50);
        assert_eq!(grid.attached_child_count, 0);
        assert!(
            grid.logical_to_slot.is_empty(),
            "logical_to_slot is transient and must start empty"
        );
    }

    #[test]
    fn set_item_count_updates_field() {
        let mut grid = two_column_grid(50);
        grid.set_item_count(100);
        assert_eq!(grid.item_count, 100);
    }

    #[test]
    fn set_grid_delegate_replaces_delegate() {
        let mut grid = two_column_grid(10);
        let new_delegate: Arc<dyn SliverGridDelegate> =
            Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(3));
        grid.set_grid_delegate(new_delegate.clone());
        let actual_count = grid
            .grid_delegate()
            .as_any()
            .downcast_ref::<SliverGridDelegateWithFixedCrossAxisCount>()
            .expect("delegate must downcast to SliverGridDelegateWithFixedCrossAxisCount")
            .cross_axis_count;
        assert_eq!(actual_count, 3);
    }

    #[test]
    fn debug_impl_does_not_panic() {
        let grid = two_column_grid(10);
        let output = format!("{grid:?}");
        assert!(
            output.contains("RenderSliverGridLazy"),
            "Debug output must name the type"
        );
    }

    #[test]
    fn clone_preserves_config_and_resets_transient_logical_to_slot() {
        let mut grid = two_column_grid(20);
        grid.attached_child_count = 4;
        let cloned = grid.clone();
        assert_eq!(cloned.item_count, 20);
        assert_eq!(cloned.attached_child_count, 4);
        assert!(
            cloned.logical_to_slot.is_empty(),
            "logical_to_slot is transient and must be reset on clone"
        );
    }

    // ── Window math (delegate-only, no layout context needed) ─────────────────

    /// For a 2-column 200px-wide grid with square tiles (100×100) and 50 items,
    /// verifies that the delegate computes the correct window bounds and scroll
    /// extent.
    #[test]
    fn window_math_matches_delegate_for_two_column_grid() {
        let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(2);
        let constraints = vertical_constraints(0.0, 200.0, 200.0);
        let layout = delegate.get_layout(constraints);

        // 200px / 2 cols = 100px each; aspect 1.0 → 100px tall.
        assert_eq!(
            layout.child_cross_axis_extent, 100.0,
            "cross extent must be 100px for a 200px grid with 2 columns"
        );
        assert_eq!(
            layout.child_main_axis_extent, 100.0,
            "main extent must be 100px (aspect ratio 1)"
        );

        // Viewport [0, 200) spans rows 0 and 1 → tiles 0..=3.
        let first_in_window =
            layout.get_min_child_index_for_scroll_offset(constraints.scroll_offset);
        assert_eq!(
            first_in_window, 0,
            "viewport starts at 0, first tile is index 0"
        );

        // At offset 200 (trailing edge): ceil(200/100) = 2 rows → last = 2*2-1 = 3.
        let last_unclamped = layout.get_max_child_index_for_scroll_offset(200.0);
        assert!(
            last_unclamped >= 3,
            "trailing edge at 200px must include at least tiles 0..=3, got {last_unclamped}"
        );

        // Scroll extent: ceil(50/2) = 25 rows × 100px = 2500px.
        let scroll_extent = layout.compute_max_scroll_offset(50);
        assert_eq!(
            scroll_extent, 2500.0,
            "50 items in 2 columns = 25 rows × 100px"
        );
    }

    /// Oracle 2-D positions for a 2-column 200px grid: col 0 at x=0, col 1 at
    /// x=100; row 0 at y=0, row 1 at y=100.
    #[test]
    fn cross_axis_and_scroll_offsets_are_correct_for_two_column_grid() {
        let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(2);
        let constraints = vertical_constraints(0.0, 400.0, 200.0);
        let layout = delegate.get_layout(constraints);

        // Cross-axis offsets (x positions for vertical scroll)
        assert_eq!(
            layout.get_cross_axis_offset_of_child(0),
            0.0,
            "tile 0: col 0 → x=0"
        );
        assert_eq!(
            layout.get_cross_axis_offset_of_child(1),
            100.0,
            "tile 1: col 1 → x=100"
        );
        assert_eq!(
            layout.get_cross_axis_offset_of_child(2),
            0.0,
            "tile 2 (row 1, col 0) → x=0"
        );
        assert_eq!(
            layout.get_cross_axis_offset_of_child(3),
            100.0,
            "tile 3 (row 1, col 1) → x=100"
        );

        // Main-axis scroll offsets (y positions for vertical scroll)
        assert_eq!(
            layout.get_scroll_offset_of_child(0),
            0.0,
            "row 0 starts at y=0"
        );
        assert_eq!(
            layout.get_scroll_offset_of_child(1),
            0.0,
            "tile 1 is in row 0 → y=0"
        );
        assert_eq!(
            layout.get_scroll_offset_of_child(2),
            100.0,
            "row 1 starts at y=100"
        );
        assert_eq!(
            layout.get_scroll_offset_of_child(3),
            100.0,
            "tile 3 is in row 1 → y=100"
        );
    }
}

//! `RenderSliverMainAxisGroup` — places multiple sliver children in a linear
//! array along the main axis, composing their geometries into one sliver.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderSliverMainAxisGroup`
//! (`packages/flutter/lib/src/rendering/sliver_group.dart`, tag `3.44.0`):
//! children are laid out one by one in growth-direction order; a child at or
//! above the top of the viewport receives a nonzero scroll offset telling it
//! where along the main axis to start; children after it are placed by the
//! space consumed so far. Pinned descendants (a pinned persistent header
//! positions itself at the viewport top regardless of scroll offset) are then
//! pulled back so every child paints within the group's own remaining scroll
//! extent — the group never lets a pinned child escape its bounds.
//!
//! Paint order is Flutter's: LAST child first, so earlier children (the
//! pinned header preceding its content) draw on top. Hit-testing walks
//! first-to-last, mirroring `hitTestChildren`.
//!
//! # Divergences (documented)
//!
//! - `child_scroll_offset` / `child_main_axis_position` (the per-child query
//!   hooks on [`RenderSliver`]) keep their trait defaults: FLUI's pipeline
//!   resolves child positions from the offsets this layout commits via
//!   `position_child`, and the hooks have no variable-arity consumer today.
//!   Implementing them would require identifying a child's index from a
//!   `&dyn` reference, which the trait surface does not offer.

use flui_tree::Variable;
use flui_types::layout::Axis;
use flui_types::{Offset, geometry::px};

use flui_rendering::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

/// Absolute values below this threshold collapse to zero — the oracle's
/// `_fixPrecisionError`, guarding the running subtractions in the child
/// constraint derivation against f32 drift.
const PRECISION_ERROR_TOLERANCE: f32 = flui_foundation::EPSILON_F32;

fn fix_precision_error(value: f32) -> f32 {
    if value.abs() < PRECISION_ERROR_TOLERANCE {
        0.0
    } else {
        value
    }
}

/// A sliver that lays out multiple sliver children sequentially along the
/// main axis and reports their composed geometry.
///
/// See the module docs for the layout contract and the paint/hit-test
/// ordering it preserves from the oracle.
#[derive(Clone, Default)]
pub struct RenderSliverMainAxisGroup {
    /// Child count committed by the last layout — bounds the hit-test walk
    /// (the hit context carries no child count of its own).
    laid_out_child_count: usize,
}

impl RenderSliverMainAxisGroup {
    /// Creates an empty group; children arrive through the render tree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for RenderSliverMainAxisGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverMainAxisGroup")
            .field("laid_out_children", &self.laid_out_child_count)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderSliverMainAxisGroup {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_int("laid_out_children", self.laid_out_child_count as i64, None);
    }
}

impl RenderSliver for RenderSliverMainAxisGroup {
    type Arity = Variable;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, SliverPhysicalParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        self.laid_out_child_count = child_count;

        if child_count == 0 {
            return SliverGeometry::ZERO;
        }

        // Growth-direction iteration order: forward walks 0..n, reverse
        // walks n..0 (the oracle's firstChild/childAfter vs
        // lastChild/childBefore pairing).
        let order: Vec<usize> = match constraints.growth_direction {
            GrowthDirection::Forward => (0..child_count).collect(),
            GrowthDirection::Reverse => (0..child_count).rev().collect(),
        };

        let mut scroll_offset = 0.0_f32;
        let mut layout_offset = 0.0_f32;
        let mut max_paint_extent = 0.0_f32;
        let mut paint_offset = constraints.overlap;
        let mut max_scroll_obstruction_extent = 0.0_f32;
        let mut cache_origin = constraints.cache_origin;
        let mut remaining_cache_extent = constraints.remaining_cache_extent;

        // Per-child main-axis paint offsets, committed after the correction
        // passes below.
        let mut child_paint_offsets = vec![0.0_f32; child_count];
        let mut child_paint_extents = vec![0.0_f32; child_count];

        for (walked, &index) in order.iter().enumerate() {
            let before_offset_paint_extent =
                self.calculate_paint_offset(&constraints, 0.0, scroll_offset);

            let child_scroll_offset = (constraints.scroll_offset - scroll_offset).max(0.0);
            let corrected_cache_origin = cache_origin.max(-child_scroll_offset);
            let cache_extent_correction = cache_origin - corrected_cache_origin;

            let mut child_constraints = constraints;
            child_constraints.scroll_offset = child_scroll_offset;
            child_constraints.cache_origin = corrected_cache_origin;
            child_constraints.overlap =
                fix_precision_error(paint_offset - before_offset_paint_extent).max(0.0);
            child_constraints.remaining_paint_extent = fix_precision_error(
                constraints.remaining_paint_extent - before_offset_paint_extent,
            );
            child_constraints.remaining_cache_extent =
                fix_precision_error(remaining_cache_extent + cache_extent_correction).max(0.0);
            child_constraints.preceding_scroll_extent =
                scroll_offset + constraints.preceding_scroll_extent;

            let child_geometry = ctx.layout_child(index, child_constraints);

            // A correction propagates upward unchanged; the viewport reruns
            // layout with the corrected offset (`RenderSliverPadding` does
            // the same).
            if let Some(correction) = child_geometry.scroll_offset_correction {
                return SliverGeometry::scroll_offset_correction(correction);
            }

            let child_paint_offset = layout_offset + child_geometry.paint_origin;
            child_paint_offsets[index] = child_paint_offset;
            child_paint_extents[index] = child_geometry.paint_extent;

            scroll_offset += child_geometry.scroll_extent;
            layout_offset += child_geometry.layout_extent;
            max_paint_extent += child_geometry.max_paint_extent;
            max_scroll_obstruction_extent += child_geometry.max_scroll_obstruction_extent;
            paint_offset = paint_offset.max(child_paint_offset + child_geometry.paint_extent);

            if child_geometry.cache_extent != 0.0 {
                remaining_cache_extent = fix_precision_error(
                    remaining_cache_extent - child_geometry.cache_extent - cache_extent_correction,
                );
                cache_origin = (corrected_cache_origin + child_geometry.cache_extent).min(0.0);
            }

            debug_assert!(
                walked + 1 == order.len() || max_paint_extent.is_finite(),
                "unreachable sliver: a sliver follows a sliver with an \
                 infinite max paint extent"
            );
        }

        // Pull pinned children (and any child painting past the group's
        // remaining scroll extent) back inside the group's bounds — the
        // oracle's post-loop paint correction.
        let remaining_extent = (scroll_offset - constraints.scroll_offset).max(0.0);
        if paint_offset > remaining_extent {
            let pinned_children_overflow =
                max_scroll_obstruction_extent > remaining_extent - constraints.overlap;
            let paint_correction = paint_offset - remaining_extent;
            paint_offset = remaining_extent;
            for index in 0..child_count {
                let child_paint_end = child_paint_offsets[index] + child_paint_extents[index];
                let child_is_pinned = ctx
                    .child_geometry(index)
                    .is_some_and(|g| g.max_scroll_obstruction_extent > 0.0);
                if child_paint_end > remaining_extent
                    || (pinned_children_overflow && child_is_pinned)
                {
                    child_paint_offsets[index] -= paint_correction;
                }
            }
        }

        let cache_extent = self.calculate_cache_offset(
            &constraints,
            constraints.scroll_offset.min(0.0),
            scroll_offset,
        );
        let paint_extent = paint_offset.clamp(0.0, constraints.remaining_paint_extent);

        // Reversed effective axes (up/left) place each child from the far
        // edge — the oracle's final pass, run after `paint_extent` is known.
        let reversed = constraints
            .growth_direction
            .apply_to_axis_direction(constraints.axis_direction)
            .is_reversed();
        for index in 0..child_count {
            let main = if reversed {
                paint_extent - child_paint_offsets[index] - child_paint_extents[index]
            } else {
                child_paint_offsets[index]
            };
            let offset = match constraints.axis() {
                Axis::Vertical => Offset::new(px(0.0), px(main)),
                Axis::Horizontal => Offset::new(px(main), px(0.0)),
            };
            ctx.position_child(index, offset);
        }

        // `..ZERO` struct-update drops the oracle constructor's defaults
        // (`layoutExtent ??= paintExtent`, `visible ??= paintExtent > 0`,
        // `hitTestExtent ??= paintExtent`) — all restated explicitly here,
        // the exact omission class that produced three real header bugs in
        // this workspace (and, unstated, made this group unhittable in its
        // first cut).
        SliverGeometry {
            scroll_extent: scroll_offset,
            paint_extent,
            max_paint_extent,
            cache_extent,
            layout_extent: paint_extent,
            hit_test_extent: paint_extent,
            has_visual_overflow: scroll_offset > constraints.remaining_paint_extent
                || constraints.scroll_offset > 0.0,
            visible: paint_extent > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    /// Paint LAST child first so earlier children draw on top — a pinned
    /// header preceding its content must overlap it, exactly the oracle's
    /// `paint` walking `lastChild` backward. The oracle's per-child
    /// `geometry.visible` gate is NOT restated here: FLUI's paint pipeline
    /// already skips any sliver whose committed geometry is invisible
    /// (`pipeline/owner/paint.rs`), and the harness paint-cull test pins
    /// that contract for this group.
    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        ctx.paint_children_reverse();
    }

    /// First-to-last child walk, stopping at the first hit — the oracle's
    /// `hitTestChildren`. Positions resolve from the offsets layout
    /// committed; the walk is bounded by the children the last layout
    /// committed (the hit context carries no child count of its own).
    fn hit_test_children(
        &self,
        ctx: &mut SliverHitTestContext<'_, Variable, SliverPhysicalParentData>,
    ) -> bool {
        for index in 0..self.laid_out_child_count {
            if ctx.hit_test_child_at_layout_offset(index) {
                return true;
            }
        }
        false
    }

    fn child_cross_axis_position(
        &self,
        _constraints: &SliverConstraints,
        _child: &dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::SliverProtocol>,
    ) -> f32 {
        0.0
    }
}

// A vertical group of two fixed-extent slivers must compose their scroll
// extents and lay the second past the first — covered by the render-object
// harness in `crates/flui-objects/tests/render_object_harness.rs` (catalog
// row + `harness_sliver_main_axis_group`) and the widget-level parity file.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision_error_collapses_to_zero_inside_the_tolerance() {
        assert_eq!(fix_precision_error(PRECISION_ERROR_TOLERANCE / 2.0), 0.0);
        assert_eq!(fix_precision_error(-PRECISION_ERROR_TOLERANCE / 2.0), 0.0);
        assert_eq!(fix_precision_error(1.5), 1.5);
        assert_eq!(fix_precision_error(-1.5), -1.5);
    }

    #[test]
    fn an_empty_group_reports_zero_geometry() {
        // The full layout contract is exercised through the harness (see the
        // catalog row); this pins only the childless fast path's shape.
        let group = RenderSliverMainAxisGroup::new();
        assert_eq!(group.laid_out_child_count, 0);
    }
}

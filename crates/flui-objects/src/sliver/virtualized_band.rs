//! Shared virtualizer band-walk for the request-strategy lazily-virtualized
//! sliver list ([`super::sliver_list::RenderSliverList`]).
//!
//! ## Sharing contract
//!
//! [`walk_virtualizer_band`] handles everything the request strategy needs:
//! virtualizer sync, band query, logical↔dense-slot reconciliation, geometry
//! computation, and anchor correction. Off-band eviction is NOT one of
//! those — the element tree owns it, driven by the retained band this
//! function returns (the caller forwards it to `ctx.emit_retain_band`); the
//! render side never disposes a child itself, which is what avoids an ABA
//! double-remove between the render and element sides.
//!
//! The one piece still delegated to the caller is what to do with an
//! in-band index that has **no** currently-attached child:
//!
//! - `on_absent(logical_i, dense_count, box_constraints, ctx)`: decides what
//!   to do with a missing in-band item (request it, etc.).

use std::collections::BTreeMap;

use flui_tree::Variable;
use flui_types::geometry::px;
use flui_types::layout::AxisDirection;

use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry, child_paint_offset},
    context::SliverLayoutContext,
    parent_data::SliverMultiBoxAdaptorParentData,
    protocol::ChildLayout,
    virtualization::{AnchorCorrection, ScrollWindow, Virtualizer},
};

/// How far the running mean of measured extents must drift from the current
/// estimate before the unmeasured items are re-hinted — relative to the
/// estimate, with a 1 px floor. Re-hinting moves every unmeasured offset (and
/// therefore the scrollbar's total), so it is done for a material change,
/// not on every remeasure of a heterogeneous list.
const ADAPTIVE_ESTIMATE_RELATIVE_TOLERANCE: f32 = 0.05;

// ============================================================================
// HELPER FREE FUNCTIONS  (pub(super) — used by sliver_list)
// ============================================================================

/// Adapts [`SliverConstraints`] to the protocol-agnostic [`ScrollWindow`]
/// that [`Virtualizer::query`] expects.
///
/// Field mapping follows Flutter's `RenderSliverMultiBoxAdaptor` semantics:
///
/// | `ScrollWindow` field | `SliverConstraints` field(s)                              |
/// |----------------------|-----------------------------------------------------------|
/// | `offset`             | `scroll_offset`                                           |
/// | `main_extent`        | `remaining_paint_extent`                                  |
/// | `cache_before`       | `(-cache_origin).max(0)` — cache behind the leading edge  |
/// | `cache_after`        | `(remaining_cache_extent - remaining_paint_extent).max(0)`|
///
/// This is a free function that lives *outside* the `virtualization` module
/// (which must stay protocol-agnostic).
#[inline]
pub(super) fn constraints_to_scroll_window(c: &SliverConstraints) -> ScrollWindow {
    let cache_before = (-c.cache_origin).max(0.0);
    let cache_after = (c.remaining_cache_extent - c.remaining_paint_extent).max(0.0);
    ScrollWindow {
        offset: c.scroll_offset,
        main_extent: c.remaining_paint_extent,
        cache_before,
        cache_after,
    }
}

/// Returns the main-axis extent of `size` for `axis_direction`.
#[inline]
pub(super) fn main_axis_extent(size: flui_types::Size, axis_direction: AxisDirection) -> f32 {
    match axis_direction {
        AxisDirection::TopToBottom | AxisDirection::BottomToTop => size.height.get(),
        AxisDirection::LeftToRight | AxisDirection::RightToLeft => size.width.get(),
    }
}

/// Returns `offset_of(logical_i + 1) - offset_of(logical_i)`, i.e. the
/// item's current extent in the virtualizer (measured or estimated).
///
/// Returns `0.0` when `logical_i` is the last item (no successor).
///
/// Complexity: `O(log n)` — two tree prefix-sum queries.
#[inline]
pub(super) fn item_extent_from_virtualizer(v: &Virtualizer, logical_i: usize) -> f32 {
    if logical_i < v.len() {
        v.offset_of(logical_i + 1) - v.offset_of(logical_i)
    } else {
        0.0
    }
}

/// Feeds a [`Virtualizer::set_measured`] result into the anchor-correction
/// accumulator.
#[inline]
pub(super) fn accumulate_anchor_correction(
    pending_correction: &mut f32,
    correction: Option<AnchorCorrection>,
) {
    if let Some(c) = correction {
        *pending_correction += c.delta;
    }
}

/// Takes the accumulated anchor correction for this pass.
///
/// Emitted whenever it is non-zero, in either scroll direction. The anchor
/// (the first visible item) stays pixel-stationary only if the offset moves
/// by exactly the growth above it in the same pass; deferring the correction
/// on a backward scroll — what this used to do, after ADR-0003's consumer
/// note — is itself the one-frame content jump it meant to prevent, and with
/// requests serviced inside the frame every build re-runs layout at an
/// unchanged offset anyway, so the deferral could only ever last one pass.
/// The direction-independent rule is recorded in ADR-0051.
///
/// # Returns
///
/// `Some(delta)` to emit as `SliverGeometry::scroll_offset_correction`;
/// `None` when nothing is pending.
#[inline]
pub(super) fn take_anchor_correction(pending_correction: &mut f32) -> Option<f32> {
    if *pending_correction == 0.0 {
        None
    } else {
        let out = *pending_correction;
        *pending_correction = 0.0;
        Some(out)
    }
}

// Pure-function mirrors of `RenderSliver::calculate_paint_offset` /
// `calculate_cache_offset`.  Identical formulae, but free functions avoid
// requiring `&self` in the shared walk.
#[inline]
fn calc_paint_offset(c: &SliverConstraints, from: f32, to: f32) -> f32 {
    debug_assert!(from <= to);
    let a = c.scroll_offset;
    let b = c.scroll_offset + c.remaining_paint_extent;
    (to.min(b) - from.max(a)).max(0.0)
}

#[inline]
fn calc_cache_offset(c: &SliverConstraints, from: f32, to: f32) -> f32 {
    debug_assert!(from <= to);
    let a = c.scroll_offset + c.cache_origin;
    let b = c.scroll_offset + c.remaining_cache_extent;
    (to.min(b) - from.max(a))
        .max(0.0)
        .min(c.remaining_cache_extent)
}

// ============================================================================
// SHARED BAND-WALK
// ============================================================================

/// Drives the full virtualized-band layout pass for one sliver scroll frame.
///
/// This is the request strategy's shared geometry engine, used by
/// [`super::sliver_list::RenderSliverList`]. The absent-in-band action is the
/// only point delegated to the caller.
///
/// ## Parameters
///
/// - `virtualizer`: per-item extent store with `O(log n)` range queries.
/// - `logical_to_slot`: logical-index → dense-slot map; cleared and rebuilt
///   on each pass.  Kept on the caller to reuse the `BTreeMap` allocation.
/// - `item_count`: total known item count.  May be shrunken mid-pass by the
///   `NoChild` outcome of `on_absent`.
/// - `pending_correction`: the anchor-correction accumulator (see
///   [`super::sliver_list`] module doc).
/// - `attached_child_count`: written with the post-layout dense child count
///   so the `&self` hit-test walk can reverse-iterate without re-querying.
/// - `constraints`: sliver constraints for this layout pass.
/// - `ctx`: live sliver layout context wired to the pipeline.
/// - `on_absent(logical_i, dense_count, box_constraints, ctx)`: strategy for
///   each in-band index that has **no** attached child.  `dense_count` is the
///   pre-loop child count; the request strategy ignores it — the element
///   tree decides placement once it services the request.
///
/// ## Returns
///
/// `(geometry, cache_first, cache_last)`:
/// - `geometry` — the [`SliverGeometry`] for this pass.
/// - `cache_first` / `cache_last` — the `[first, last)` logical-index band
///   that was retained this pass (the `Virtualizer::query` result, clamped
///   by any mid-pass `item_count` shrink via `NoChild`).  The caller forwards
///   these to `ctx.emit_retain_band(cache_first, cache_last)` so the element
///   tree can evict everything outside the band.
pub(super) fn walk_virtualizer_band<'ctx, G>(
    virtualizer: &mut Virtualizer,
    logical_to_slot: &mut BTreeMap<usize, usize>,
    item_count: &mut usize,
    pending_correction: &mut f32,
    attached_child_count: &mut usize,
    constraints: &SliverConstraints,
    ctx: &mut SliverLayoutContext<'ctx, Variable, SliverMultiBoxAdaptorParentData>,
    on_absent: &mut G,
) -> (SliverGeometry, usize, usize)
where
    G: FnMut(
        usize, // logical_i
        usize, // dense_count (pre-loop; ignored by the request strategy)
        BoxConstraints,
        &mut SliverLayoutContext<'ctx, Variable, SliverMultiBoxAdaptorParentData>,
    ) -> ChildLayout,
{
    // ── 1. Sync virtualizer count ──────────────────────────────────────────
    virtualizer.set_count(*item_count);

    // ── 2. Query visible/cache band ────────────────────────────────────────
    let window = constraints_to_scroll_window(constraints);
    let range = virtualizer.query(&window);
    let cache_first = range.cache_first;
    let cache_last = range.cache_last;

    // ── 3. Build logical → dense-slot map from current parent data ─────────
    // O(K) where K = currently attached child count (bounded by viewport).
    logical_to_slot.clear();
    let dense_count = ctx.child_count();
    for slot in 0..dense_count {
        if let Some(pd) = ctx.child_parent_data(slot) {
            let previous = logical_to_slot.insert(pd.index, slot);
            // Two attached children carrying one logical index would leave
            // one of them unpositioned and painted at a stale offset. Every
            // path that stamps an index (adopt-time, relocation, a keyed
            // remap) owes uniqueness; a collision here is a bug in one of
            // them, not a state to tolerate quietly.
            debug_assert!(
                previous.is_none(),
                "BUG: lazy sliver has two attached children stamped with logical index {} \
                 (dense slots {:?} and {slot})",
                pd.index,
                previous,
            );
        }
    }

    // ── 4. Lay out in-band children + dispatch the absent strategy ─────────
    // Box constraints: cross axis tight, main axis unbounded (child sizes itself).
    let box_constraints = constraints.as_box_constraints(0.0, f32::INFINITY, None);
    // Anchor = first visible item this pass.  Feeds `set_measured` so that
    // re-measuring an item above the viewport emits an `AnchorCorrection`
    // that keeps the viewport pixel-stationary.
    let anchor = (range.first, 0.0_f32);

    for logical_i in cache_first..cache_last {
        if logical_i >= *item_count {
            break;
        }

        if let Some(&slot) = logical_to_slot.get(&logical_i) {
            // Present: lay out and record the real extent.
            let size = ctx.layout_box_child(slot, box_constraints);
            let extent = main_axis_extent(size, constraints.axis_direction);
            let correction = virtualizer.set_measured(logical_i, extent, anchor);
            accumulate_anchor_correction(pending_correction, correction);
        } else {
            // Absent: strategy owns the complete decision.
            // match_same_arms: `Scheduled`'s empty body is kept separate from
            // the `#[non_exhaustive]` forward-compat wildcard on purpose — the
            // arm exists to document this variant's semantics, and merging it
            // into `_` would silently absorb future `ChildLayout` variants.
            #[expect(clippy::match_same_arms)]
            match on_absent(logical_i, dense_count, box_constraints, ctx) {
                ChildLayout::Scheduled => {
                    // Parked for the element tree to service between layout
                    // passes of this frame's fixpoint. Use the virtualizer
                    // estimate this pass; the real extent arrives once the
                    // request is serviced and a later pass lays it out.
                }
                ChildLayout::NoChild => {
                    // Strategy declined — end of data.  Clamp count to actual.
                    *item_count = logical_i;
                    virtualizer.set_count(logical_i);
                    break;
                }
                ChildLayout::Unwired => {
                    // No request sink wired — expected in Direct/test contexts;
                    // a production consumer that hits this arm has a wiring bug.
                    break;
                }
                // ChildLayout is #[non_exhaustive]; forward-compat wildcard.
                _ => {}
            }
        }
    }

    // ── 4a. Adapt the estimate for still-unmeasured items ──────────────────
    // The caller's `default_extent_estimate` seeds the first pass only. From
    // then on the unmeasured items are hinted with the mean of the band's
    // own measured children (Flutter's `_extrapolateMaxScrollOffset`
    // averages the same set) — the band's, not all history's, so a jump from
    // tall items into short ones adapts on the first measured batch instead
    // of after hundreds. Without this a band under an over-estimate
    // converges geometrically — each pass only requests the few items the
    // stale hint says still fit — which is a many-frame pop-in on the old
    // post-frame service path and a bounded-fixpoint overrun on the in-frame
    // one. Re-hinting the items above the anchor moves the anchor's offset,
    // so the delta goes through the same correction accumulator a remeasure
    // does: the anchored content stays pixel-stationary.
    if let Some(mean) = virtualizer.measured_mean_in(cache_first..cache_last) {
        let current = virtualizer.default_estimate();
        let tolerance = (current * ADAPTIVE_ESTIMATE_RELATIVE_TOLERANCE).max(1.0);
        if (mean - current).abs() > tolerance {
            let correction = virtualizer.adapt_default_estimate(mean, anchor);
            accumulate_anchor_correction(pending_correction, correction);
        }
    }

    // ── 4a'. Re-query the band until the measurements stop moving it ───────
    // Steps 2–4 sized the band with pre-measure hints. Measuring (and any
    // re-hint above) moves every offset after the first changed item, so
    // the window now covers indices the first query did not see. Those must
    // be handled in THIS pass: the frame's fixpoint only runs another
    // pass when the manager built or evicted something, so an item that
    // came into band purely through measurement would otherwise wait for
    // the next frame — with a 1000 px seed over 1 px items, the whole
    // viewport would. When a correction is pending the viewport re-runs
    // this layout at the corrected offset in the same pass and that run
    // re-queries on its own; a request against the pre-correction window
    // is at worst a child the re-run evicts next pass, never a lost one.
    //
    // An index the widening pulls in that is ALREADY attached is laid out
    // here, exactly as step 4 lays out the first query's residents: step 10
    // positions every in-band child from the virtualizer's extents, and a
    // resident positioned from an extent this pass never measured (its
    // view changed size, or the pass adapted the hint under it) would be
    // painted at a stale offset — and everything after it with it — until
    // the next frame's first query happened to cover it. Measuring those
    // residents can move the window once more, so the query repeats until
    // the covered range stops growing: a band whose newly covered indices
    // were all attached and all measured smaller than their hints builds
    // and evicts nothing, so nothing else would schedule the pass that
    // requests what the last widening exposed. The covered range only ever
    // grows and is bounded by the item count, so the loop terminates.
    let mut covered_first = cache_first;
    let mut covered_last = cache_last;
    let mut stop = false;
    while !stop {
        let widened = virtualizer.query(&window);
        let next_first = widened.cache_first.min(covered_first);
        let next_last = widened.cache_last.max(covered_last).min(*item_count);
        if next_first == covered_first && next_last == covered_last {
            break;
        }
        let newly_covered = (next_first..covered_first).chain(covered_last..next_last);
        for logical_i in newly_covered {
            if let Some(&slot) = logical_to_slot.get(&logical_i) {
                let size = ctx.layout_box_child(slot, box_constraints);
                let extent = main_axis_extent(size, constraints.axis_direction);
                let correction = virtualizer.set_measured(logical_i, extent, anchor);
                accumulate_anchor_correction(pending_correction, correction);
                continue;
            }
            match on_absent(logical_i, dense_count, box_constraints, ctx) {
                ChildLayout::NoChild => {
                    *item_count = logical_i;
                    virtualizer.set_count(logical_i);
                    stop = true;
                    break;
                }
                ChildLayout::Unwired => {
                    stop = true;
                    break;
                }
                _ => {}
            }
        }
        covered_first = next_first;
        covered_last = next_last;
    }
    let cache_first = covered_first;
    let cache_last = covered_last;

    // ── 4b. Clamp cache_last after possible mid-pass item_count shrink ──────
    // The NoChild branch above may call `virtualizer.set_count(logical_i)`,
    // shrinking `*item_count`.  Shadow `cache_last` so every downstream gate
    // uses the tighter bound; stale high-index children fall outside the
    // in-band check → disposed instead of panicking on `offset_of`.
    let cache_last = cache_last.min(*item_count);

    // ── 5. Snapshot attached count for hit-test (takes &self) ─────────────
    // Off-band eviction is not this function's job: the element tree drives
    // it via `SparseChildren::retain_band` using the `cache_first`/
    // `cache_last` band this function returns, which is what avoids an ABA
    // double-remove between the render and element sides.
    *attached_child_count = ctx.child_count();

    // ── 6. Build slot → logical map for positioning ───────────────────────
    // Rebuilt after the layout pass so newly-materialized children are
    // included.
    let slot_to_logical: Vec<Option<usize>> = (0..*attached_child_count)
        .map(|slot| ctx.child_parent_data(slot).map(|pd| pd.index))
        .collect();

    // ── 7. Write layout_offset to parent data ─────────────────────────────
    // O(K · log n): K slot reads, each offset_of O(log n).
    for (slot, maybe_logical) in slot_to_logical.iter().enumerate() {
        let Some(&logical_i) = maybe_logical.as_ref() else {
            continue;
        };
        let in_band = logical_i >= cache_first && logical_i < cache_last;
        if !in_band {
            continue;
        }
        let layout_offset = virtualizer.offset_of(logical_i);
        if let Some(pd) = ctx.child_parent_data_mut(slot) {
            pd.index = logical_i;
            pd.layout_offset = layout_offset;
        }
    }

    // ── 8. Compute geometry ────────────────────────────────────────────────
    let scroll_extent = virtualizer.total_extent().value();
    let paint_extent = calc_paint_offset(constraints, 0.0, scroll_extent);
    let cache_extent = calc_cache_offset(constraints, 0.0, scroll_extent);
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

    // ── 9. Position in-band children ───────────────────────────────────────
    // Run after geometry is known so `child_paint_offset` clips correctly.
    // O(K · log n): K slots, each offset_of + item_extent_from_virtualizer O(log n).
    for (slot, maybe_logical) in slot_to_logical.iter().enumerate() {
        let Some(&logical_i) = maybe_logical.as_ref() else {
            continue;
        };
        let in_band = logical_i >= cache_first && logical_i < cache_last;
        if !in_band {
            continue;
        }
        let layout_offset = virtualizer.offset_of(logical_i);
        let item_extent = item_extent_from_virtualizer(virtualizer, logical_i);
        let paint_offset =
            child_paint_offset(constraints, &geometry, px(layout_offset), px(item_extent));
        ctx.position_child(slot, paint_offset);
    }

    // ── 10. Anchor correction ───────────────────────────────────────────────
    let scroll_offset_correction = take_anchor_correction(pending_correction);

    let geometry = SliverGeometry {
        scroll_offset_correction,
        ..geometry
    };

    // Return the geometry and the retained band so the caller can forward it
    // to `ctx.emit_retain_band`.
    (geometry, cache_first, cache_last)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::{
        constraints::{GrowthDirection, SliverConstraints},
        view::ScrollDirection,
    };
    use flui_types::layout::AxisDirection;

    fn vertical(
        scroll_offset: f32,
        remaining_paint_extent: f32,
        remaining_cache_extent: f32,
        cache_origin: f32,
    ) -> SliverConstraints {
        SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: remaining_paint_extent,
            remaining_cache_extent,
            cache_origin,
        }
    }

    // ── constraints_to_scroll_window ─────────────────────────────────────────

    #[test]
    fn adapter_at_scroll_origin_no_cache() {
        let c = vertical(0.0, 600.0, 600.0, 0.0);
        let w = constraints_to_scroll_window(&c);
        assert_eq!(w.offset, 0.0);
        assert_eq!(w.main_extent, 600.0);
        assert_eq!(w.cache_before, 0.0);
        assert_eq!(w.cache_after, 0.0);
    }

    #[test]
    fn adapter_with_cache_before_and_after() {
        let c = vertical(100.0, 600.0, 1000.0, -200.0);
        let w = constraints_to_scroll_window(&c);
        assert_eq!(w.offset, 100.0);
        assert_eq!(w.main_extent, 600.0);
        assert_eq!(w.cache_before, 200.0); // (-(-200)).max(0)
        assert_eq!(w.cache_after, 400.0); // (1000-600).max(0)
    }

    #[test]
    fn adapter_negative_cache_origin_positive_is_zero() {
        // cache_origin > 0 means cache does not extend behind leading edge
        let c = vertical(0.0, 600.0, 600.0, 50.0);
        let w = constraints_to_scroll_window(&c);
        assert_eq!(w.cache_before, 0.0); // (-50).max(0) == 0
        assert_eq!(w.cache_after, 0.0);
    }

    // ── take_anchor_correction ────────────────────────────────────────────
    #[test]
    fn correction_emits_whatever_is_pending_and_resets() {
        let mut correction = 10.0_f32;
        assert_eq!(take_anchor_correction(&mut correction), Some(10.0));
        assert_eq!(correction, 0.0);
    }
    #[test]
    fn correction_zero_pending_emits_none() {
        let mut correction = 0.0_f32;
        assert_eq!(take_anchor_correction(&mut correction), None);
    }
    #[test]
    fn correction_is_direction_independent() {
        // The old state machine withheld a pending correction on a backward
        // scroll; the accumulator is now drained on every pass, so the
        // caller's scroll direction is not even an input.
        let mut correction = -8.0_f32;
        assert_eq!(take_anchor_correction(&mut correction), Some(-8.0));
        assert_eq!(correction, 0.0);
        assert_eq!(take_anchor_correction(&mut correction), None);
    }
    // ── accumulate_anchor_correction ─────────────────────────────────────────

    #[test]
    fn accumulate_adds_delta_when_some() {
        let mut pending = 0.0_f32;
        accumulate_anchor_correction(&mut pending, Some(AnchorCorrection { delta: 3.0 }));
        accumulate_anchor_correction(&mut pending, Some(AnchorCorrection { delta: 7.0 }));
        assert_eq!(pending, 10.0);
    }

    #[test]
    fn accumulate_noop_on_none() {
        let mut pending = 5.0_f32;
        accumulate_anchor_correction(&mut pending, None);
        assert_eq!(pending, 5.0);
    }
}

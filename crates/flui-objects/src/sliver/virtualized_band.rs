//! Shared virtualizer band-walk for lazily-virtualized sliver layout.
//!
//! Extracted from `RenderSliverListLazy::perform_layout` (U3b) so both the
//! build strategy ([`super::sliver_list_lazy::RenderSliverListLazy`]) and the
//! request strategy ([`super::sliver_list::RenderSliverList`]) drive
//! the same geometry engine without duplicating per-frame virtualizer
//! bookkeeping.
//!
//! ## Sharing contract
//!
//! [`walk_virtualizer_band`] handles everything that is identical across
//! strategies: virtualizer sync, band query, logical↔dense-slot reconciliation,
//! off-band disposal, geometry computation, and anchor correction.  The
//! strategy-specific piece — what to do with an in-band index that has **no**
//! currently-attached child — is delegated to the caller via three closures:
//!
//! - `resident_build_fallback(logical_i)`: called when laying out an already-
//!   attached child, in case the backend concurrently evicted the slot.
//! - `on_absent(logical_i, dense_count, box_constraints, ctx)`: decides what
//!   to do with a missing in-band item (build it, request it, etc.).
//! - `on_dispose(logical_i)`: called for each off-band child after the
//!   deferred removal is enqueued, for caller-side cleanup.

use std::collections::BTreeMap;

use flui_tree::Variable;
use flui_types::geometry::px;
use flui_types::layout::AxisDirection;

use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry, child_paint_offset},
    context::SliverLayoutContext,
    parent_data::SliverMultiBoxAdaptorParentData,
    protocol::{BoxChildRef, BoxProtocol, ChildLayout},
    traits::RenderObject,
    virtualization::{AnchorCorrection, ScrollWindow, Virtualizer},
};

// ============================================================================
// OFF-BAND DISPOSAL STRATEGY
// ============================================================================

/// Controls whether `walk_virtualizer_band` disposes off-band children from
/// the render tree or leaves that to the element tree.
///
/// - `RenderOwned`: the render object owns its children (built via
///   `build_and_layout_box_child`); `dispose_box_child` enqueues the removal.
///   Used by [`super::sliver_list_lazy::RenderSliverListLazy`].
///
/// - `ElementOwned`: the element tree owns the children; the render sliver
///   must NOT call `dispose_box_child` (that would evict the render node from
///   under the element's feet, causing an ABA double-remove on the next
///   element-side eviction). Instead the caller reads back the band indices
///   returned by [`walk_virtualizer_band`] and signals the element tree via
///   `ctx.emit_retain_band`.  Used by
///   [`super::sliver_list::RenderSliverList`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OffBandDisposal {
    /// Caller is `RenderSliverListLazy`; dispose via `ctx.dispose_box_child`.
    RenderOwned,
    /// Caller is `RenderSliverList`; skip dispose — element tree handles it.
    ElementOwned,
}

// ============================================================================
// HELPER FREE FUNCTIONS  (pub(super) — used by sliver_list_lazy + sliver_list)
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

/// Applies the anchor-correction state machine.
///
/// Policy:
/// - **Backward scroll** (`current < last`): suppress emission and preserve
///   the accumulator — apply it when the user scrolls forward again.
/// - **Forward / idle / stationary**: if `pending_correction != 0`, emit and
///   reset.
///
/// Always updates `last_scroll_offset` to `current_scroll_offset`.
///
/// # Returns
///
/// `Some(delta)` to emit as `SliverGeometry::scroll_offset_correction`;
/// `None` when the correction is suppressed.
#[inline]
pub(super) fn resolve_anchor_correction(
    pending_correction: &mut f32,
    last_scroll_offset: &mut f32,
    current_scroll_offset: f32,
) -> Option<f32> {
    let is_backward = current_scroll_offset < *last_scroll_offset;
    *last_scroll_offset = current_scroll_offset;
    if is_backward || *pending_correction == 0.0 {
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
/// This is the shared algorithm for `RenderSliverListLazy` (build strategy,
/// U3b) and `RenderSliverList` (request strategy).  Both share the
/// virtualizer geometry bookkeeping; the absent-in-band action is the only
/// point of divergence and is delegated to the caller.
///
/// ## Parameters
///
/// - `virtualizer`: per-item extent store with `O(log n)` range queries.
/// - `logical_to_slot`: logical-index → dense-slot map; cleared and rebuilt
///   on each pass.  Kept on the caller to reuse the `BTreeMap` allocation.
/// - `item_count`: total known item count.  May be shrunken mid-pass by the
///   `NoChild` outcome of `on_absent`.
/// - `pending_correction` / `last_scroll_offset`: anchor-correction state
///   machine (see [`super::sliver_list_lazy`] module doc).
/// - `attached_child_count`: written with the post-layout dense child count
///   so the `&self` hit-test walk can reverse-iterate without re-querying.
/// - `constraints`: sliver constraints for this layout pass.
/// - `ctx`: live sliver layout context wired to the pipeline.
/// - `resident_build_fallback(logical_i)`: factory supplied to
///   [`SliverLayoutContext::build_and_layout_box_child`] for in-band children
///   that are **already** attached, covering the rare case where the backend
///   concurrently evicted the slot.  Return `None` for request-only consumers.
/// - `on_absent(logical_i, dense_count, box_constraints, ctx)`: strategy for
///   each in-band index that has **no** attached child.  The closure owns the
///   complete decision, including whether to use `dense_count` as the
///   deferred-insert position.  `dense_count` is the pre-loop child count
///   (the correct append index for the build strategy; a request-only strategy
///   ignores it — the element decides placement at service time).
/// - `off_band_disposal`: whether to call `ctx.dispose_box_child` for off-band
///   children ([`OffBandDisposal::RenderOwned`]) or to skip that call and let
///   the element tree handle removal via the retain-band channel
///   ([`OffBandDisposal::ElementOwned`]).
/// - `on_dispose(logical_i)`: called for each off-band child **after**
///   `ctx.dispose_box_child` enqueues the deferred removal (only fires for
///   `RenderOwned`).  Use this to fire caller-side cleanup hooks (e.g.
///   `dispose_hook` in `RenderSliverListLazy`).
///
/// ## Returns
///
/// `(geometry, cache_first, cache_last)`:
/// - `geometry` — the [`SliverGeometry`] for this pass.
/// - `cache_first` / `cache_last` — the `[first, last)` logical-index band
///   that was retained this pass (the `Virtualizer::query` result, clamped
///   by any mid-pass `item_count` shrink via `NoChild`).  `ElementOwned`
///   callers forward these to `ctx.emit_retain_band(cache_first, cache_last)`.
pub(super) fn walk_virtualizer_band<'ctx, F, G, H>(
    virtualizer: &mut Virtualizer,
    logical_to_slot: &mut BTreeMap<usize, usize>,
    item_count: &mut usize,
    pending_correction: &mut f32,
    last_scroll_offset: &mut f32,
    attached_child_count: &mut usize,
    constraints: &SliverConstraints,
    ctx: &mut SliverLayoutContext<'ctx, Variable, SliverMultiBoxAdaptorParentData>,
    off_band_disposal: OffBandDisposal,
    resident_build_fallback: &mut F,
    on_absent: &mut G,
    on_dispose: &mut H,
) -> (SliverGeometry, usize, usize)
where
    F: FnMut(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>>,
    G: FnMut(
        usize, // logical_i
        usize, // dense_count (pre-loop, the deferred-insert position)
        BoxConstraints,
        &mut SliverLayoutContext<'ctx, Variable, SliverMultiBoxAdaptorParentData>,
    ) -> ChildLayout<BoxChildRef>,
    H: FnMut(usize), // logical_i of each off-band disposed child (RenderOwned only)
{
    let scroll_offset = constraints.scroll_offset;

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
            logical_to_slot.insert(pd.index, slot);
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
            // Child already exists — build closure is unreachable on the Ready
            // arm, but the backend may call it if the slot was concurrently
            // evicted.  `resident_build_fallback` is a disjoint borrow from
            // `virtualizer`; Rust-2021 disjoint capture applies at the call site.
            let result =
                ctx.build_and_layout_box_child(slot, logical_i, box_constraints, &mut |_| {
                    resident_build_fallback(logical_i)
                });
            if let ChildLayout::Ready(BoxChildRef { size, .. }) = result {
                let extent = main_axis_extent(size, constraints.axis_direction);
                let correction = virtualizer.set_measured(logical_i, extent, anchor);
                accumulate_anchor_correction(pending_correction, correction);
            }
        } else {
            // Absent: strategy owns the complete decision.
            let result = on_absent(logical_i, dense_count, box_constraints, ctx);
            // match_same_arms: the `Scheduled | Ready(_)` no-op arm is kept separate
            // from the `#[non_exhaustive]` forward-compat wildcard on purpose — the
            // arm exists to document the per-variant semantics, and merging it into
            // `_` would silently absorb future `ChildLayout` variants.
            #[allow(clippy::match_same_arms)]
            match result {
                ChildLayout::Scheduled | ChildLayout::Ready(_) => {
                    // Scheduled = parked for next frame (v1 next-frame backend).
                    // Ready     = laid out in this pass (future mid-pass backend).
                    // Both: use the virtualizer estimate this pass; real extent
                    // arrives on the next layout pass.
                }
                ChildLayout::NoChild => {
                    // Strategy declined — end of data.  Clamp count to actual.
                    *item_count = logical_i;
                    virtualizer.set_count(logical_i);
                    break;
                }
                ChildLayout::Unwired => {
                    // No backend wired — expected in Direct/test contexts; a
                    // production consumer that hits this arm has a wiring bug.
                    break;
                }
                // ChildLayout is #[non_exhaustive]; forward-compat wildcard.
                _ => {}
            }
        }
    }

    // ── 4b. Clamp cache_last after possible mid-pass item_count shrink ──────
    // The NoChild branch above may call `virtualizer.set_count(logical_i)`,
    // shrinking `*item_count`.  Shadow `cache_last` so every downstream gate
    // uses the tighter bound; stale high-index children fall outside the
    // in-band check → disposed instead of panicking on `offset_of`.
    let cache_last = cache_last.min(*item_count);

    // ── 5. Dispose off-band children ──────────────────────────────────────
    // For `RenderOwned` slivers, `dispose_box_child` enqueues the render-node
    // removal (U3c D2: Remove → Insert ordering D3 in `layout_dirty_root`).
    // For `ElementOwned` slivers, we skip this entirely: the element tree drives
    // eviction via `SparseChildren::retain_band` using the `cache_first`/
    // `cache_last` band returned below, preventing the ABA double-remove that
    // would occur if both the render side and the element side tried to free the
    // same node.
    if off_band_disposal == OffBandDisposal::RenderOwned {
        for (&logical_i, &slot) in logical_to_slot.iter() {
            let in_band = logical_i >= cache_first && logical_i < cache_last;
            if !in_band {
                let keep_alive = ctx
                    .child_parent_data(slot)
                    .is_some_and(|pd| pd.keep_alive.keep_alive);
                if keep_alive {
                    continue;
                }
                if let Some(id) = ctx.child_id(slot) {
                    ctx.dispose_box_child(id);
                }
                on_dispose(logical_i);
            }
        }
    }

    // ── 6. Snapshot attached count for hit-test (takes &self) ─────────────
    *attached_child_count = ctx.child_count();

    // ── 7. Build slot → logical map for positioning ───────────────────────
    // Rebuilt after the layout pass so newly-materialized Ready children are
    // included.
    let slot_to_logical: Vec<Option<usize>> = (0..*attached_child_count)
        .map(|slot| ctx.child_parent_data(slot).map(|pd| pd.index))
        .collect();

    // ── 8. Write layout_offset to parent data ─────────────────────────────
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

    // ── 9. Compute geometry ────────────────────────────────────────────────
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

    // ── 10. Position in-band children ─────────────────────────────────────
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

    // ── 11. Anchor correction ──────────────────────────────────────────────
    let scroll_offset_correction =
        resolve_anchor_correction(pending_correction, last_scroll_offset, scroll_offset);

    let geometry = SliverGeometry {
        scroll_offset_correction,
        ..geometry
    };

    // Return the geometry and the retained band so element-owned callers can
    // forward it to `ctx.emit_retain_band`.
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

    // ── resolve_anchor_correction ─────────────────────────────────────────────

    #[test]
    fn correction_idle_forward_emits_and_resets() {
        let mut correction = 10.0_f32;
        let mut last = 200.0_f32;
        let result = resolve_anchor_correction(&mut correction, &mut last, 200.0);
        assert_eq!(result, Some(10.0));
        assert_eq!(correction, 0.0);
        assert_eq!(last, 200.0);
    }

    #[test]
    fn correction_backward_suppresses_and_preserves_accumulator() {
        let mut correction = 5.0_f32;
        let mut last = 200.0_f32;
        let result = resolve_anchor_correction(&mut correction, &mut last, 100.0);
        assert_eq!(result, None);
        assert_eq!(correction, 5.0, "accumulator must be preserved");
        assert_eq!(last, 100.0);
    }

    #[test]
    fn correction_zero_pending_emits_none() {
        let mut correction = 0.0_f32;
        let mut last = 100.0_f32;
        let result = resolve_anchor_correction(&mut correction, &mut last, 200.0);
        assert_eq!(result, None);
    }

    #[test]
    fn correction_forward_after_backward_emits() {
        let mut correction = 8.0_f32;
        let mut last = 200.0_f32;
        // backward: suppress
        resolve_anchor_correction(&mut correction, &mut last, 100.0);
        assert_eq!(correction, 8.0);
        // forward: emit
        let result = resolve_anchor_correction(&mut correction, &mut last, 300.0);
        assert_eq!(result, Some(8.0));
        assert_eq!(correction, 0.0);
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

//! `RenderSliverListLazy` — lazily-virtualized list of Box children.
//!
//! Implements the U3b lazy `SliverList` consumer: drives
//! [`Virtualizer`] for `O(log n)` range queries, builds only the
//! visible-plus-cache band via the re-entrant build contract
//! (ADR-0003 Decision 2, [`SliverLayoutContext::build_and_layout_box_child`]),
//! and signals off-band children for garbage-collection via the dispose hook.
//!
//! # Design (Model B — logical-identity-keyed)
//!
//! Each attached Box child carries its *logical* item index in
//! `SliverMultiBoxAdaptorParentData::index`. The list owns the
//! logical ↔ dense-slot reconciliation:
//!
//! 1. On every `perform_layout` pass, scan dense slots `0..child_count()`
//!    and read `child_parent_data(slot).index` to build a `logical → slot`
//!    map.
//! 2. For each logical `i` in the cache band: if a slot exists, lay the
//!    child out and feed the real extent back to the Virtualizer; if absent,
//!    call the build hook — the v1 next-frame backend parks the request.
//! 3. For children NOT in the cache band: honour `keep_alive` and otherwise
//!    fire the dispose hook (the owning pipeline performs the actual removal).
//! 4. Compute [`SliverGeometry`] with `scroll_offset_correction` from the
//!    accumulated anchor-correction state machine.
//!
//! # Anchor-correction state machine
//!
//! `Virtualizer::set_measured` emits `Some(AnchorCorrection)` when a
//! re-measured item above the scroll anchor shifts the anchored pixel
//! position. Consumer policy:
//!
//! - **Backward scroll** (offset decreased): suppress emission and preserve
//!   the accumulator — apply it when the user scrolls forward again.
//! - **Forward / idle / stationary**: emit `scroll_offset_correction` if
//!   non-zero and reset the accumulator.
//!
//! # Next-frame latency
//!
//! Newly-requested children arrive on the *next* pass (next-frame backend).
//! Single-frame completeness is not guaranteed; settling is typically 1–2
//! frames for a fresh scroll band.
//!
//! # Thread-affinity
//!
//! Control-plane: `Arc<dyn Fn…>` is `Send + Sync` (required by the
//! `RenderSliver` supertrait) but the render object itself is not designed
//! for concurrent mutation.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_tree::Variable;
use flui_types::geometry::px;
use flui_types::layout::AxisDirection;

use flui_rendering::{
    constraints::{SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverMultiBoxAdaptorParentData,
    protocol::{BoxChildRef, BoxProtocol, ChildLayout},
    traits::{RenderObject, RenderSliver},
    virtualization::{AnchorCorrection, ScrollWindow, Virtualizer},
};

// ============================================================================
// CONSTRAINTS → SCROLL WINDOW ADAPTER
// ============================================================================

/// Adapts [`flui_rendering::constraints::SliverConstraints`] to the protocol-agnostic [`ScrollWindow`]
/// that [`Virtualizer::query`] expects.
///
/// Field mapping follows Flutter's `RenderSliverMultiBoxAdaptor` semantics:
///
/// | `ScrollWindow` field | `SliverConstraints` field(s)                               |
/// |----------------------|-------------------------------------------------------------|
/// | `offset`             | `scroll_offset`                                             |
/// | `main_extent`        | `remaining_paint_extent`                                    |
/// | `cache_before`       | `(-cache_origin).max(0)` — cache behind the leading edge    |
/// | `cache_after`        | `(remaining_cache_extent - remaining_paint_extent).max(0)`  |
///
/// This is a free function that lives *outside* the `virtualization` module
/// (which must stay protocol-agnostic) and is tested directly.
#[inline]
fn constraints_to_scroll_window(
    c: &flui_rendering::constraints::SliverConstraints,
) -> ScrollWindow {
    let cache_before = (-c.cache_origin).max(0.0);
    let cache_after = (c.remaining_cache_extent - c.remaining_paint_extent).max(0.0);
    ScrollWindow {
        offset: c.scroll_offset,
        main_extent: c.remaining_paint_extent,
        cache_before,
        cache_after,
    }
}

/// Returns the main-axis extent of `size` for the given `axis_direction`.
#[inline]
fn main_axis_extent(size: flui_types::Size, axis_direction: AxisDirection) -> f32 {
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
/// Complexity: `O(log n)` average and worst case (two tree prefix-sum queries;
/// `n` is bounded by `Virtualizer::len()`).
#[inline]
fn item_extent_from_virtualizer(v: &Virtualizer, logical_i: usize) -> f32 {
    if logical_i < v.len() {
        v.offset_of(logical_i + 1) - v.offset_of(logical_i)
    } else {
        0.0
    }
}

// ============================================================================
// RENDER OBJECT
// ============================================================================

/// A lazily-virtualized `SliverList`: builds Box children on demand, only
/// within the visible-plus-cache window, and disposes off-band children.
///
/// Unlike [`RenderSliverFixedExtentList`](super::sliver_fixed_extent_list::RenderSliverFixedExtentList),
/// which lays out all pre-attached children eagerly at a fixed extent, this
/// object:
///
/// - Calls the pluggable `child_source` closure to materialize children
///   as needed (via the re-entrant build contract).
/// - Feeds real measured extents back to a [`Virtualizer`] so the
///   scrollbar total converges incrementally from estimates.
/// - Fires `dispose_hook` for children that scroll out of the cache band
///   (the owning pipeline performs the actual tree removal).
///
/// # Construction
///
/// ```ignore
/// use std::sync::Arc;
/// use flui_objects::RenderSliverListLazy;
///
/// let list = RenderSliverListLazy::new(
///     10_000,
///     48.0, // estimate: each item is roughly 48 px tall
///     Arc::new(move |logical_index| {
///         if logical_index >= 10_000 { return None; }
///         Some(Box::new(/* your render object */) as Box<_>)
///     }),
///     None, // optional dispose hook
/// );
/// ```
pub struct RenderSliverListLazy {
    // ── data source ──────────────────────────────────────────────────────────
    /// Total known item count (may be updated at runtime via `set_item_count`).
    item_count: usize,

    /// Pluggable factory: logical index → Box render object (or `None`).
    ///
    /// `Arc<dyn Fn>` (not `FnMut`) keeps it `Clone` and `Send + Sync`, matching
    /// the `RenderSliver` supertrait's `Send + Sync + 'static` requirement.
    child_source: Arc<dyn Fn(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>> + Send + Sync>,

    /// Optional hook fired just before a child is signalled for disposal
    /// (scrolled out of the cache band without keep-alive). The hook receives
    /// the logical index so the caller can clean up associated widget state.
    /// Actual tree removal is done by the owning pipeline, not here.
    dispose_hook: Option<Arc<dyn Fn(usize) + Send + Sync>>,

    // ── virtualization state ─────────────────────────────────────────────────
    /// Protocol-agnostic windowing engine.
    virtualizer: Virtualizer,

    /// Logical → dense-slot map rebuilt from parent-data on every pass.
    /// Kept as a field to reuse the allocation across passes.
    logical_to_slot: BTreeMap<usize, usize>,

    // ── anchor-correction state machine ─────────────────────────────────────
    /// Accumulated anchor-correction delta not yet emitted to the viewport.
    pending_correction: f32,

    /// Scroll offset at the end of the previous layout pass; used to detect
    /// backward scrolling (offset decreased → suppress emission).
    last_scroll_offset: f32,

    // ── hit-test support ────────────────────────────────────────────────────
    /// Dense child count committed after the last layout pass. Used by the
    /// `&self` hit-test reverse-walk which cannot re-read `ctx.child_count()`.
    attached_child_count: usize,
}

impl fmt::Debug for RenderSliverListLazy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderSliverListLazy")
            .field("item_count", &self.item_count)
            .field("attached_child_count", &self.attached_child_count)
            .field("pending_correction", &self.pending_correction)
            .field("last_scroll_offset", &self.last_scroll_offset)
            // closures intentionally omitted — not Debug
            .finish_non_exhaustive()
    }
}

// Manual Clone: `Arc` clones are cheap reference-count bumps; `BTreeMap` is
// reset before every pass, so cloning its transient state is acceptable.
impl Clone for RenderSliverListLazy {
    fn clone(&self) -> Self {
        Self {
            item_count: self.item_count,
            child_source: self.child_source.clone(),
            dispose_hook: self.dispose_hook.clone(),
            virtualizer: self.virtualizer.clone(),
            logical_to_slot: self.logical_to_slot.clone(),
            pending_correction: self.pending_correction,
            last_scroll_offset: self.last_scroll_offset,
            attached_child_count: self.attached_child_count,
        }
    }
}

impl RenderSliverListLazy {
    /// Creates a new lazy sliver list.
    ///
    /// # Parameters
    ///
    /// - `item_count`: initial total number of items.
    /// - `default_extent_estimate`: main-axis pixel estimate seeded into the
    ///   [`Virtualizer`] for every not-yet-measured item. Must be finite and > 0.
    /// - `child_source`: factory `logical_index → Box<dyn RenderObject<BoxProtocol>>`.
    ///   `None` signals end-of-data (used for unknown-length sources).
    ///   Must be `Send + Sync`.
    /// - `dispose_hook`: optional callback fired before a child is disposed.
    ///
    /// # Panics
    ///
    /// Panics if `default_extent_estimate` is not finite or is ≤ 0.
    #[must_use]
    pub fn new(
        item_count: usize,
        default_extent_estimate: f32,
        child_source: Arc<
            dyn Fn(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>> + Send + Sync,
        >,
        dispose_hook: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    ) -> Self {
        assert!(
            default_extent_estimate.is_finite() && default_extent_estimate > 0.0,
            "default_extent_estimate must be finite and > 0 so the virtualizer \
             can seed non-zero estimates; got {default_extent_estimate}"
        );
        Self {
            item_count,
            child_source,
            dispose_hook,
            virtualizer: Virtualizer::new(item_count, default_extent_estimate),
            logical_to_slot: BTreeMap::new(),
            pending_correction: 0.0,
            last_scroll_offset: 0.0,
            attached_child_count: 0,
        }
    }

    /// Updates the total item count. The Virtualizer is resized in-place
    /// (`O(|delta| · log n)` average and worst case — tree edits, not a
    /// full rebuild).
    #[inline]
    pub fn set_item_count(&mut self, n: usize) {
        self.item_count = n;
        self.virtualizer.set_count(n);
    }

    /// Current total item count.
    #[inline]
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Read access to the underlying [`Virtualizer`] (for inspection / tests).
    #[inline]
    #[must_use]
    pub fn virtualizer(&self) -> &Virtualizer {
        &self.virtualizer
    }

    // ── anchor-correction helpers ────────────────────────────────────────────

    /// Feeds a `set_measured` result into the accumulator.
    #[inline]
    fn accumulate_correction(&mut self, correction: Option<AnchorCorrection>) {
        if let Some(c) = correction {
            self.pending_correction += c.delta;
        }
    }

    /// Applies the correction state machine and returns the
    /// `scroll_offset_correction` value for [`SliverGeometry`].
    ///
    /// Policy:
    /// - **Backward scroll** (`scroll_offset < last_scroll_offset`): suppress
    ///   emission and preserve the accumulator — the correction will be applied
    ///   once the user scrolls forward again.
    /// - **Forward / idle / stationary**: if `pending_correction != 0`, emit
    ///   it and reset.
    ///
    /// Always updates `last_scroll_offset`.
    #[inline]
    fn resolve_correction(&mut self, scroll_offset: f32) -> Option<f32> {
        let is_backward = scroll_offset < self.last_scroll_offset;
        self.last_scroll_offset = scroll_offset;

        if is_backward || self.pending_correction == 0.0 {
            None
        } else {
            let out = self.pending_correction;
            self.pending_correction = 0.0;
            Some(out)
        }
    }
}

// ============================================================================
// Diagnosticable + capability impls
// ============================================================================

impl Diagnosticable for RenderSliverListLazy {
    fn debug_fill_properties(&self, props: &mut DiagnosticsBuilder) {
        props.add_int("item_count", self.item_count as i64, None);
        props.add_int(
            "attached_child_count",
            self.attached_child_count as i64,
            None,
        );
        props.add_double("pending_correction", self.pending_correction, Some("px"));
    }
}

// ============================================================================
// RenderSliver impl
// ============================================================================

impl RenderSliver for RenderSliverListLazy {
    type Arity = Variable;
    type ParentData = SliverMultiBoxAdaptorParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let scroll_offset = constraints.scroll_offset;

        // ── 1. Sync virtualizer count ──────────────────────────────────────
        self.virtualizer.set_count(self.item_count);

        // ── 2. Query visible/cache band ────────────────────────────────────
        let window = constraints_to_scroll_window(&constraints);
        let range = self.virtualizer.query(&window);
        let cache_first = range.cache_first;
        let cache_last = range.cache_last;

        // ── 3. Build logical → dense-slot map from current parent data ─────
        // O(K) where K = currently attached child count (bounded by viewport).
        self.logical_to_slot.clear();
        let dense_count = ctx.child_count();
        for slot in 0..dense_count {
            if let Some(pd) = ctx.child_parent_data(slot) {
                self.logical_to_slot.insert(pd.index, slot);
            }
        }

        // ── 4. Lay out in-band children + request absent ones ──────────────
        // Box constraints: cross axis tight, main axis unbounded (child sizes itself).
        let box_constraints = constraints.as_box_constraints(0.0, f32::INFINITY, None);
        // Anchor = first VISIBLE item this pass.  Using `range.first` makes
        // `set_measured` emit an `AnchorCorrection` whenever an item above the
        // viewport is re-measured with a different extent than its estimate —
        // the correction keeps the viewport pixel-stationary.  The old
        // `self.virtualizer.anchor_item()` always returned `(0, 0.0)` (the
        // virtualizer's default), so `index < anchor.0` was always false →
        // AnchorCorrection was never emitted (dead code).
        let anchor = (range.first, 0.0);

        for logical_i in cache_first..cache_last {
            if logical_i >= self.item_count {
                break;
            }

            if let Some(&slot) = self.logical_to_slot.get(&logical_i) {
                // Child already attached: lay it out and record the real extent.
                let result = ctx.build_and_layout_box_child(
                    slot,
                    logical_i,
                    box_constraints,
                    // Child already exists — build closure is unreachable on the
                    // Ready arm, but the backend may call it if the child was
                    // concurrently removed. Provide a valid factory. Borrow the
                    // source directly (no per-frame Arc::clone) — Rust-2021 disjoint
                    // capture borrows only `child_source`, released when the call
                    // returns, before the `&mut self.virtualizer` use below.
                    &mut |_| (self.child_source)(logical_i),
                );
                if let ChildLayout::Ready(BoxChildRef { size, .. }) = result {
                    let extent = main_axis_extent(size, constraints.axis_direction);
                    let correction = self.virtualizer.set_measured(logical_i, extent, anchor);
                    self.accumulate_correction(correction);
                }
            } else {
                // Child absent: request it via the build hook.
                // The append position in the dense list is `dense_count`: deferred
                // inserts cannot grow the dense count mid-pass, so every absent child
                // in the band parks its request with the SAME `index = dense_count`
                // this pass. This is safe — not a collision — because the Insert phase
                // of the deferred drain applies serially and `apply_deferred_mutation`
                // appends each new child then clamps its position to
                // `min(index, parent.child_count())`; `child_count` grows by one per
                // insert, so the clamp resolves to the current tail every time and the
                // children land in consecutive slots in request order (D3 keeps Remove
                // before Insert, so any removed slots are already compacted away).
                let result = ctx.build_and_layout_box_child(
                    dense_count,
                    logical_i,
                    box_constraints,
                    // Borrow the source directly (no per-frame Arc::clone).
                    &mut |_| (self.child_source)(logical_i),
                );
                match result {
                    ChildLayout::Scheduled | ChildLayout::Ready(_) => {
                        // Scheduled = parked for next frame (v1 next-frame backend).
                        // Ready = laid out in this pass (future mid-pass backend).
                        // Both: use the estimate this pass; real extent arrives next.
                    }
                    ChildLayout::NoChild => {
                        // Builder declined — end of data. Clamp count to actual.
                        self.item_count = logical_i;
                        self.virtualizer.set_count(logical_i);
                        break;
                    }
                    ChildLayout::Unwired => {
                        // No build backend wired — expected in leaf test contexts;
                        // a production consumer that hits this arm has a wiring bug.
                        break;
                    }
                    // ChildLayout is #[non_exhaustive]; forward-compat wildcard.
                    _ => {}
                }
            }
        }

        // ── 4b. Clamp cache_last after possible mid-pass item_count shrink ──
        // The build loop above may call `self.virtualizer.set_count(logical_i)`
        // when the source returns `NoChild`, shrinking `self.item_count` mid-pass.
        // Steps 5/8/10 gate on `in_band` using the PRE-shrink `cache_last`: any
        // child whose logical index is ≥ the new count is still treated as in-band
        // → `offset_of(logical_i)` / `offset_of(logical_i+1)` asserts
        // `index <= len()` and panics.  The same hazard applies via the public
        // `set_item_count` shrink path.  Shadow here so every downstream gate uses
        // the tighter bound, causing stale high-index children to fall outside the
        // in-band check → disposed (step 5) / skipped (steps 8/10) instead of
        // panicking.
        let cache_last = cache_last.min(self.item_count);

        // ── 5. Dispose off-band children ──────────────────────────────────
        // Children whose logical index is outside [cache_first, cache_last) are
        // no longer needed.  For each, read the keep-alive flag from parent-data
        // (if present) and skip disposal when set.  Otherwise enqueue a deferred
        // remove via `ctx.dispose_box_child` and fire the optional `dispose_hook`
        // for widget-state cleanup.
        //
        // U3c D2: `ctx.dispose_box_child(id)` pushes `id` into the
        // `pending_removes` sink that `layout_dirty_root` drains (Remove → Insert
        // ordering per D3) after the walk releases its borrows.  This is the
        // symmetric partner of `build_and_layout_box_child` / `pending_builds`.
        for (&logical_i, &slot) in &self.logical_to_slot {
            let in_band = logical_i >= cache_first && logical_i < cache_last;
            if !in_band {
                // Keep-alive gate: read parent data from the layout context.
                // `pd.keep_alive` is a `KeepAliveParentDataMixin`; the inner
                // `keep_alive` bool is the flag set by the child.
                let keep_alive = ctx
                    .child_parent_data(slot)
                    .map(|pd| pd.keep_alive.keep_alive)
                    .unwrap_or(false);
                if keep_alive {
                    continue;
                }
                // Enqueue deferred removal.
                if let Some(id) = ctx.child_id(slot) {
                    ctx.dispose_box_child(id);
                }
                // Fire the optional hook for widget-state cleanup.
                if let Some(ref hook) = self.dispose_hook {
                    hook(logical_i);
                }
            }
        }

        // ── 6. Snapshot attached count for hit-test (takes &self) ──────────
        self.attached_child_count = ctx.child_count();

        // ── 7. Build slot → logical map for positioning ───────────────────
        // Rebuild after the layout pass so newly-materialized Ready children
        // are included.
        let slot_to_logical: Vec<Option<usize>> = (0..self.attached_child_count)
            .map(|slot| ctx.child_parent_data(slot).map(|pd| pd.index))
            .collect();

        // ── 8. Write layout_offset to parent data ─────────────────────────
        // Commit the logical index and layout offset so hit-test and paint
        // drivers can read them. O(K · log n): K slot reads, each offset_of O(log n).
        for (slot, maybe_logical) in slot_to_logical.iter().enumerate() {
            let Some(&logical_i) = maybe_logical.as_ref() else {
                continue;
            };
            let in_band = logical_i >= cache_first && logical_i < cache_last;
            if !in_band {
                continue;
            }
            let layout_offset = self.virtualizer.offset_of(logical_i);
            if let Some(pd) = ctx.child_parent_data_mut(slot) {
                pd.index = logical_i;
                pd.layout_offset = layout_offset;
            }
        }

        // ── 9. Compute geometry ────────────────────────────────────────────
        let scroll_extent = self.virtualizer.total_extent().value();
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

        // ── 10. Position in-band children ─────────────────────────────────
        // Run after geometry is known so child_paint_offset clips correctly.
        // O(K · log n): K slots, each offset_of + item_extent_from_virtualizer O(log n).
        for (slot, maybe_logical) in slot_to_logical.iter().enumerate() {
            let Some(&logical_i) = maybe_logical.as_ref() else {
                continue;
            };
            let in_band = logical_i >= cache_first && logical_i < cache_last;
            if !in_band {
                continue;
            }
            let layout_offset = self.virtualizer.offset_of(logical_i);
            let item_extent = item_extent_from_virtualizer(&self.virtualizer, logical_i);
            let paint_offset =
                child_paint_offset(&constraints, &geometry, px(layout_offset), px(item_extent));
            ctx.position_child(slot, paint_offset);
        }

        // ── 11. Anchor correction ──────────────────────────────────────────
        let scroll_offset_correction = self.resolve_correction(scroll_offset);

        SliverGeometry {
            scroll_offset_correction,
            ..geometry
        }
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        ctx.paint_children();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>) -> bool {
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
    use super::*;
    use flui_rendering::{
        constraints::{GrowthDirection, SliverConstraints},
        view::ScrollDirection,
    };
    use flui_types::layout::AxisDirection;

    // ── helpers ──────────────────────────────────────────────────────────────

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

    // ── ScrollWindow adapter ──────────────────────────────────────────────────

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
    fn adapter_mid_scroll_with_cache() {
        // cache_origin = -250 → 250 px cache BEFORE the leading edge.
        // remaining_cache_extent = 1100 > remaining_paint_extent = 600 →
        // cache_after = 1100 - 600 = 500.
        let c = vertical(1000.0, 600.0, 1100.0, -250.0);
        let w = constraints_to_scroll_window(&c);
        assert_eq!(w.offset, 1000.0);
        assert_eq!(w.main_extent, 600.0);
        assert_eq!(w.cache_before, 250.0);
        assert_eq!(w.cache_after, 500.0);
    }

    #[test]
    fn adapter_cache_after_zero_when_cache_eq_paint() {
        // remaining_cache_extent == remaining_paint_extent → cache_after = 0.
        let c = vertical(100.0, 600.0, 600.0, 0.0);
        let w = constraints_to_scroll_window(&c);
        assert_eq!(w.cache_before, 0.0);
        assert_eq!(w.cache_after, 0.0);
    }

    #[test]
    fn adapter_cache_before_zero_when_cache_origin_positive() {
        // cache_origin should normally be <= 0; degenerate positive value
        // clamps cache_before to 0.
        let c = vertical(100.0, 600.0, 600.0, 10.0);
        let w = constraints_to_scroll_window(&c);
        assert_eq!(w.cache_before, 0.0);
    }

    /// Table-driven check across several constraint combinations.
    #[test]
    fn adapter_table() {
        struct Case {
            scroll_offset: f32,
            remaining_paint_extent: f32,
            remaining_cache_extent: f32,
            cache_origin: f32,
            want_offset: f32,
            want_main: f32,
            want_before: f32,
            want_after: f32,
        }
        let cases = [
            Case {
                scroll_offset: 0.0,
                remaining_paint_extent: 600.0,
                remaining_cache_extent: 600.0,
                cache_origin: 0.0,
                want_offset: 0.0,
                want_main: 600.0,
                want_before: 0.0,
                want_after: 0.0,
            },
            Case {
                scroll_offset: 500.0,
                remaining_paint_extent: 400.0,
                remaining_cache_extent: 900.0,
                cache_origin: -250.0,
                want_offset: 500.0,
                want_main: 400.0,
                want_before: 250.0,
                want_after: 500.0,
            },
            Case {
                scroll_offset: 0.0,
                remaining_paint_extent: 300.0,
                remaining_cache_extent: 600.0,
                cache_origin: 0.0,
                want_offset: 0.0,
                want_main: 300.0,
                want_before: 0.0,
                want_after: 300.0,
            },
        ];
        for c in &cases {
            let constraints = vertical(
                c.scroll_offset,
                c.remaining_paint_extent,
                c.remaining_cache_extent,
                c.cache_origin,
            );
            let w = constraints_to_scroll_window(&constraints);
            assert_eq!(w.offset, c.want_offset, "offset mismatch");
            assert_eq!(w.main_extent, c.want_main, "main_extent mismatch");
            assert_eq!(
                w.cache_before, c.want_before,
                "cache_before for scroll={}",
                c.scroll_offset
            );
            assert_eq!(
                w.cache_after, c.want_after,
                "cache_after for scroll={}",
                c.scroll_offset
            );
        }
    }

    // ── anchor-correction state machine ───────────────────────────────────────

    fn make_list() -> RenderSliverListLazy {
        RenderSliverListLazy::new(1000, 48.0, Arc::new(|_| None), None)
    }

    #[test]
    fn correction_idle_forward_emits_and_resets() {
        let mut list = make_list();
        list.pending_correction = 16.0;
        list.last_scroll_offset = 100.0;

        // Offset increased → not backward → should emit.
        let out = list.resolve_correction(200.0);
        assert_eq!(out, Some(16.0), "should emit the accumulated correction");
        assert_eq!(list.pending_correction, 0.0, "should reset after emission");
        assert_eq!(list.last_scroll_offset, 200.0);
    }

    #[test]
    fn correction_backward_suppresses_and_preserves_accumulator() {
        let mut list = make_list();
        list.pending_correction = 16.0;
        list.last_scroll_offset = 200.0;

        // Offset decreased → backward scroll → suppress.
        let out = list.resolve_correction(100.0);
        assert_eq!(out, None, "backward scroll must suppress correction");
        // Accumulator is preserved so it can be emitted on the next forward pass.
        assert_eq!(
            list.pending_correction, 16.0,
            "accumulator must survive backward scroll"
        );
        assert_eq!(list.last_scroll_offset, 100.0);
    }

    #[test]
    fn correction_zero_pending_emits_none() {
        let mut list = make_list();
        list.pending_correction = 0.0;
        list.last_scroll_offset = 0.0;

        let out = list.resolve_correction(100.0);
        assert_eq!(out, None);
    }

    #[test]
    fn correction_stationary_offset_emits_if_pending() {
        // Offset did not change (equal) → not backward → emit.
        let mut list = make_list();
        list.pending_correction = 16.0;
        list.last_scroll_offset = 100.0;

        let out = list.resolve_correction(100.0);
        assert_eq!(out, Some(16.0));
    }

    #[test]
    fn correction_backward_then_forward_emits_accumulated() {
        let mut list = make_list();
        list.pending_correction = 0.0;
        list.last_scroll_offset = 200.0;

        // Accumulate during backward pass.
        list.accumulate_correction(Some(AnchorCorrection { delta: 8.0 }));
        let suppressed = list.resolve_correction(100.0); // backward
        assert_eq!(suppressed, None);
        assert_eq!(
            list.pending_correction, 8.0,
            "accumulator survives backward"
        );

        // Forward pass emits the correction.
        list.accumulate_correction(Some(AnchorCorrection { delta: 4.0 }));
        let emitted = list.resolve_correction(150.0); // forward
        assert_eq!(
            emitted,
            Some(12.0),
            "forward pass emits total accumulated delta"
        );
        assert_eq!(list.pending_correction, 0.0);
    }

    #[test]
    fn accumulate_correction_adds_delta() {
        let mut list = make_list();
        list.accumulate_correction(None);
        assert_eq!(list.pending_correction, 0.0);

        list.accumulate_correction(Some(AnchorCorrection { delta: 8.0 }));
        assert_eq!(list.pending_correction, 8.0);

        list.accumulate_correction(Some(AnchorCorrection { delta: -3.0 }));
        assert_eq!(list.pending_correction, 5.0);
    }

    // ── constructor validation ────────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "default_extent_estimate must be finite")]
    fn new_panics_on_non_finite_estimate() {
        let _ = RenderSliverListLazy::new(10, f32::INFINITY, Arc::new(|_| None), None);
    }

    #[test]
    #[should_panic(expected = "default_extent_estimate must be finite")]
    fn new_panics_on_zero_estimate() {
        let _ = RenderSliverListLazy::new(10, 0.0, Arc::new(|_| None), None);
    }

    #[test]
    fn debug_impl_does_not_panic() {
        let list = make_list();
        let s = format!("{list:?}");
        assert!(s.contains("RenderSliverListLazy"));
    }

    #[test]
    fn clone_preserves_state() {
        let mut list = make_list();
        list.pending_correction = 12.5;
        list.last_scroll_offset = 500.0;
        let cloned = list.clone();
        assert_eq!(cloned.pending_correction, 12.5);
        assert_eq!(cloned.last_scroll_offset, 500.0);
        assert_eq!(cloned.item_count, 1000);
    }
}

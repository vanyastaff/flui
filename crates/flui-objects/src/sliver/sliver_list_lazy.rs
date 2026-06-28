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

use flui_rendering::{
    constraints::SliverGeometry,
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverMultiBoxAdaptorParentData,
    protocol::BoxProtocol,
    traits::{RenderObject, RenderSliver},
    virtualization::Virtualizer,
};

use super::virtualized_band::walk_virtualizer_band;

// Only used by the test-only helper methods `accumulate_correction` /
// `resolve_correction`, which exist so test code can exercise the
// correction state-machine without running a full layout pass.
#[cfg(test)]
use super::virtualized_band::{accumulate_anchor_correction, resolve_anchor_correction};
#[cfg(test)]
use flui_rendering::virtualization::AnchorCorrection;

// ============================================================================
// CONSTRAINTS → SCROLL WINDOW ADAPTER  (retained for doc reference)
// ============================================================================

// Adapts `SliverConstraints` to the `ScrollWindow` that `Virtualizer::query`
// expects.  Field mapping follows Flutter's `RenderSliverMultiBoxAdaptor`
// semantics; the implementation lives in
// `super::virtualized_band::constraints_to_scroll_window`.

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

    /// Feeds a `set_measured` result into the anchor-correction accumulator.
    ///
    /// Delegates to [`accumulate_anchor_correction`] in the shared band-walk
    /// module so test code can exercise the correction state-machine without
    /// running a full layout pass.  Production code calls this indirectly via
    /// [`walk_virtualizer_band`].
    #[cfg(test)]
    #[inline]
    fn accumulate_correction(&mut self, correction: Option<AnchorCorrection>) {
        accumulate_anchor_correction(&mut self.pending_correction, correction);
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
    /// Always updates `last_scroll_offset`. Test-only companion to
    /// [`accumulate_correction`]; production code goes through
    /// [`walk_virtualizer_band`].
    #[cfg(test)]
    #[inline]
    fn resolve_correction(&mut self, scroll_offset: f32) -> Option<f32> {
        resolve_anchor_correction(
            &mut self.pending_correction,
            &mut self.last_scroll_offset,
            scroll_offset,
        )
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

        // Borrow `child_source` separately (disjoint from the `&mut` fields
        // passed to `walk_virtualizer_band`) — Rust-2021 disjoint capture
        // borrows only `child_source` here, released when each closure call
        // returns.  No per-frame `Arc::clone`.
        let child_source = &self.child_source;

        walk_virtualizer_band(
            &mut self.virtualizer,
            &mut self.logical_to_slot,
            &mut self.item_count,
            &mut self.pending_correction,
            &mut self.last_scroll_offset,
            &mut self.attached_child_count,
            &constraints,
            ctx,
            // Resident-build fallback: child is already attached; this factory
            // fires only if the backend concurrently evicted the slot.
            //
            // The append position in the dense list is `dense_count`: deferred
            // inserts cannot grow the dense count mid-pass, so every absent
            // child in the band parks with the SAME `index = dense_count` this
            // pass. This is safe — not a collision — because Insert applies
            // serially, and `apply_deferred_mutation` clamps each new child's
            // position to `min(index, parent.child_count())`, so children land
            // in consecutive slots in request order (D3 keeps Remove before
            // Insert, so evicted slots are compacted before insertion).
            &mut |logical_i| child_source(logical_i),
            // Absent strategy: build the child via the re-entrant build contract.
            // `dense_count` is the correct deferred-insert position (see comment
            // on `resident_build_fallback` above).
            &mut |logical_i, dense_count, box_constraints, ctx| {
                ctx.build_and_layout_box_child(dense_count, logical_i, box_constraints, &mut |_| {
                    child_source(logical_i)
                })
            },
            // Dispose hook: fire the optional caller-side cleanup callback.
            &mut |logical_i| {
                if let Some(ref hook) = self.dispose_hook {
                    hook(logical_i);
                }
            },
        )
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
    use flui_rendering::virtualization::AnchorCorrection;

    // Adapter tests live in `super::super::virtualized_band::tests`; this
    // module tests the methods on `RenderSliverListLazy` that delegate to the
    // shared helpers.

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

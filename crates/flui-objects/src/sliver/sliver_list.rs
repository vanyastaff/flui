//! `RenderSliverList` — request-strategy lazily-virtualized list of Box children.
//!
//! # U4.2 — producer half only (INERT without U4.3)
//!
//! This type uses the **request-strategy seam** introduced in U4.2: when an
//! in-band child is absent it calls [`SliverLayoutContext::request_child_build`],
//! which pushes `(sliver_id, logical_index)` into the arena's request sink.
//! After the layout pass the pipeline moves those requests to
//! `PipelineOwner::take_pending_child_requests` — the binding-layer entry point
//! that the element tree (U4.3) will consume.
//!
//! **`RenderSliverList` is inert until U4.3 wires up a child manager** — it
//! emits requests but nothing services them, so absent children never appear.
//! This matches Flutter's `RenderSliverList` behavior without a `childManager`:
//! the list reports the virtualizer's estimate geometry but renders nothing for
//! unbuilt slots.  Document the inert behavior loudly; do not paper over it.
//!
//! This seam is **provisional**: U4.3 end-to-end validation may refine the
//! `(RenderId, usize)` payload or the drain ordering.  The rework surface is
//! intentionally small.
//!
//! # Design notes
//!
//! Unlike [`RenderSliverListLazy`](super::sliver_list_lazy::RenderSliverListLazy),
//! this object carries **no `child_source`** — it cannot build render objects
//! directly.  The element tree's child manager owns the construction.  Existing
//! arena-resident children (built in a prior pass) are laid out normally; only
//! absent in-band children generate requests.

use std::collections::BTreeMap;
use std::fmt;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_tree::Variable;

use flui_rendering::{
    constraints::SliverGeometry,
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverMultiBoxAdaptorParentData,
    traits::RenderSliver,
    virtualization::Virtualizer,
};

use super::virtualized_band::{OffBandDisposal, walk_virtualizer_band};

// ============================================================================
// RENDER OBJECT
// ============================================================================

/// A request-strategy lazily-virtualized `SliverList` (U4.2 producer half).
///
/// Lays out arena-resident children from the visible-plus-cache band and emits
/// build requests for absent slots via
/// [`SliverLayoutContext::request_child_build`].  The requests accumulate in
/// `PipelineOwner::take_pending_child_requests` for the element tree (U4.3).
///
/// **Inert without a U4.3 child manager** — absent children are requested but
/// never built until U4.3 wires the response path.
///
/// # Flutter parity
///
/// Corresponds to Flutter's `RenderSliverList` whose `childManager`
/// (`SliverMultiBoxAdaptorElement`) services `createChild` calls.  In FLUI the
/// element manager is the U4.3 `LazySliverElement`; this object is the render
/// half of that split.
///
/// # Construction
///
/// ```ignore
/// use flui_objects::RenderSliverList;
///
/// let list = RenderSliverList::new(10_000, 48.0);
/// ```
///
/// `default_extent_estimate` must be finite and positive; it seeds the
/// [`Virtualizer`] until real measurements arrive from laid-out children.
pub struct RenderSliverList {
    // ── item count ───────────────────────────────────────────────────────────
    /// Total known item count (may be updated via `set_item_count` once
    /// U4.3 learns the real count from the data source).
    item_count: usize,

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
    /// backward scrolling (offset decreased → suppress correction emission).
    last_scroll_offset: f32,

    // ── hit-test support ────────────────────────────────────────────────────
    /// Dense child count committed after the last layout pass. Used by the
    /// `&self` hit-test reverse-walk which cannot re-read `ctx.child_count()`.
    attached_child_count: usize,
}

impl RenderSliverList {
    /// Creates a new `RenderSliverList`.
    ///
    /// `default_extent_estimate` must be finite and positive; it seeds the
    /// [`Virtualizer`] until real measurements arrive from laid-out children.
    ///
    /// # Panics
    ///
    /// Panics if `default_extent_estimate` is not finite or is zero/negative —
    /// a zero estimate would produce a virtualizer with infinite band width.
    #[must_use]
    pub fn new(item_count: usize, default_extent_estimate: f32) -> Self {
        assert!(
            default_extent_estimate.is_finite() && default_extent_estimate > 0.0,
            "default_extent_estimate must be finite and positive, got {default_extent_estimate}",
        );
        Self {
            item_count,
            virtualizer: Virtualizer::new(item_count, default_extent_estimate),
            logical_to_slot: BTreeMap::new(),
            pending_correction: 0.0,
            last_scroll_offset: 0.0,
            attached_child_count: 0,
        }
    }

    /// Updates the known item count.  Call when the data source length changes.
    pub fn set_item_count(&mut self, count: usize) {
        self.item_count = count;
        self.virtualizer.set_count(count);
    }
}

impl fmt::Debug for RenderSliverList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderSliverList")
            .field("item_count", &self.item_count)
            .field("attached_child_count", &self.attached_child_count)
            .field("pending_correction", &self.pending_correction)
            .finish_non_exhaustive()
    }
}

impl Clone for RenderSliverList {
    fn clone(&self) -> Self {
        Self {
            item_count: self.item_count,
            virtualizer: self.virtualizer.clone(),
            logical_to_slot: BTreeMap::new(), // transient — reset each pass
            pending_correction: self.pending_correction,
            last_scroll_offset: self.last_scroll_offset,
            attached_child_count: self.attached_child_count,
        }
    }
}

// ============================================================================
// Diagnosticable + capability impls
// ============================================================================

impl Diagnosticable for RenderSliverList {
    fn debug_fill_properties(&self, props: &mut DiagnosticsBuilder) {
        props.add_int("item_count", self.item_count as i64, None);
        props.add_int(
            "attached_child_count",
            self.attached_child_count as i64,
            None,
        );
        props.add_double("pending_correction", self.pending_correction, Some("px"));
        // Always-set flag to make the inert-until-U4.3 state visible in
        // diagnostics output during development.
        props.add_flag(
            "inert_without_child_manager",
            true,
            "no U4.3 child manager wired — requests emit but are unserviced",
        );
    }
}

// ============================================================================
// RenderSliver impl
// ============================================================================

impl RenderSliver for RenderSliverList {
    type Arity = Variable;
    type ParentData = SliverMultiBoxAdaptorParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();

        let (geometry, cache_first, cache_last) = walk_virtualizer_band(
            &mut self.virtualizer,
            &mut self.logical_to_slot,
            &mut self.item_count,
            &mut self.pending_correction,
            &mut self.last_scroll_offset,
            &mut self.attached_child_count,
            &constraints,
            ctx,
            // Element tree owns the children: skip `dispose_box_child` to
            // prevent the ABA double-remove.  The element tree drives eviction
            // via `SparseChildren::retain_band` using the band indices below.
            OffBandDisposal::ElementOwned,
            // Resident-build fallback: returns `None` because this type carries
            // no owned child-source factory.  A resident child is already in the
            // arena; the fallback fires only if the slot was concurrently evicted,
            // which should not occur on the layout thread.  Returning `None`
            // signals NoChild, matching the honestly-inert posture of this type.
            &mut |_logical_i| None,
            // Absent strategy: emit a request via the U4.2 seam.  The element
            // tree (U4.3) services it post-frame.  `dense_count` is ignored —
            // the element tree decides the insert position.
            &mut |logical_i, _dense_count, _box_constraints, ctx| {
                ctx.request_child_build(logical_i)
            },
            // Dispose hook: no-op — `ElementOwned` skips the dispose path so
            // this closure never fires.
            &mut |_logical_i| {},
        );
        // Signal the retained band to the element tree via the pending_retain_bands
        // channel.  The binding layer forwards this to `SparseChildren::retain_band`
        // post-frame so out-of-band lazy children are evicted on the element side.
        ctx.emit_retain_band(cache_first, cache_last);
        geometry
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

    fn make_list() -> RenderSliverList {
        RenderSliverList::new(100, 48.0)
    }

    #[test]
    fn new_initializes_item_count() {
        let list = make_list();
        assert_eq!(list.item_count, 100);
    }

    #[test]
    #[should_panic(expected = "default_extent_estimate must be finite")]
    fn new_panics_on_infinite_estimate() {
        let _ = RenderSliverList::new(10, f32::INFINITY);
    }

    #[test]
    #[should_panic(expected = "default_extent_estimate must be finite")]
    fn new_panics_on_zero_estimate() {
        let _ = RenderSliverList::new(10, 0.0);
    }

    #[test]
    fn debug_impl_does_not_panic() {
        let list = make_list();
        let formatted = format!("{list:?}");
        assert!(formatted.contains("RenderSliverList"));
    }

    #[test]
    fn clone_preserves_item_count_and_correction() {
        let mut list = make_list();
        list.pending_correction = 8.0;
        list.last_scroll_offset = 200.0;
        let cloned = list.clone();
        assert_eq!(cloned.item_count, 100);
        assert_eq!(cloned.pending_correction, 8.0);
        assert_eq!(cloned.last_scroll_offset, 200.0);
        // logical_to_slot is reset on clone (transient state).
        assert!(cloned.logical_to_slot.is_empty());
    }

    #[test]
    fn set_item_count_updates_field() {
        let mut list = make_list();
        list.set_item_count(42);
        assert_eq!(list.item_count, 42);
    }
}

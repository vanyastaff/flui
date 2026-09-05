//! `RenderSliverList` — request-strategy lazily-virtualized list of Box children.
//!
//! # Request strategy
//!
//! When an in-band child is absent this object calls
//! [`SliverLayoutContext::request_child_build`], which parks
//! `(sliver_id, logical_index)` in the arena's request sink, and after the walk
//! it declares the retained band through [`SliverLayoutContext::emit_retain_band`].
//! The element tree — `SliverAdaptorElement<RenderSliverList>` in `flui-view`, registered as
//! this sliver's `ChildManager` — drains both signals between layout passes of
//! the frame's layout↔build fixpoint: it mounts the requested children, evicts
//! the off-band ones, and marks this sliver for the next pass, so a fresh band
//! is laid out and painted in the same frame that requested it. Children are
//! never built here; this object carries no child source.
//!
//! Without a registered manager (a render-only harness, a `Direct` layout
//! context) the requests are parked and never serviced — the list lays out the
//! estimate geometry and renders nothing for unbuilt slots. That is the
//! render-only harness's expected shape, not a production state.
//!
//! # Design notes
//!
//! This object carries **no `child_source`** — it cannot build render objects
//! directly. The element tree's child manager owns the construction. Existing
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

use super::virtualized_band::walk_virtualizer_band;

// ============================================================================
// RENDER OBJECT
// ============================================================================

/// A request-strategy lazily-virtualized `SliverList` (the render half).
///
/// Lays out arena-resident children from the visible-plus-cache band and emits
/// build requests for absent slots via
/// [`SliverLayoutContext::request_child_build`]. The element tree services
/// them between layout passes of the same frame (see the module doc).
///
/// # Flutter parity
///
/// Corresponds to Flutter's `RenderSliverList` whose `childManager`
/// (`SliverMultiBoxAdaptorElement`) services `createChild` calls. In FLUI the
/// manager is `SliverAdaptorElement<RenderSliverList>` (`flui-view`); this object is the
/// render half of that split.
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
    /// the child manager learns the real count from the data source).
    item_count: usize,

    /// Estimate currently assigned to unmeasured children.
    default_extent_estimate: f32,

    // ── virtualization state ─────────────────────────────────────────────────
    /// Protocol-agnostic windowing engine.
    virtualizer: Virtualizer,

    /// Logical → dense-slot map rebuilt from parent-data on every pass.
    /// Kept as a field to reuse the allocation across passes.
    logical_to_slot: BTreeMap<usize, usize>,

    // ── anchor correction ───────────────────────────────────────────────────
    /// Accumulated anchor-correction delta not yet emitted to the viewport.
    pending_correction: f32,

    // ── hit-test support ────────────────────────────────────────────────────
    /// Dense child count committed after the last layout pass. Used by the
    /// `&self` hit-test reverse-walk which cannot re-read `ctx.child_count()`.
    attached_child_count: usize,

    /// The item count already warned about for an unbounded main axis, so the
    /// truncation warns once rather than once per frame.
    warned_unbounded: Option<usize>,
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
            default_extent_estimate,
            virtualizer: Virtualizer::new(item_count, default_extent_estimate),
            logical_to_slot: BTreeMap::new(),
            pending_correction: 0.0,
            attached_child_count: 0,
            warned_unbounded: None,
        }
    }

    /// The known item count (the data source length as last told).
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Updates the known item count.  Call when the data source length changes.
    pub fn set_item_count(&mut self, count: usize) -> flui_rendering::RenderUpdateImpact {
        if self.item_count == count {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.item_count = count;
        self.virtualizer.set_count(count);
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Updates the estimate for unmeasured children without discarding
    /// measurements already committed by layout.
    pub fn set_default_extent_estimate(
        &mut self,
        estimate: f32,
    ) -> flui_rendering::RenderUpdateImpact {
        assert!(
            estimate.is_finite() && estimate > 0.0,
            "default_extent_estimate must be finite and positive, got {estimate}",
        );
        if self.default_extent_estimate == estimate {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.default_extent_estimate = estimate;
        let changed = self.virtualizer.set_default_estimate(estimate);
        debug_assert!(changed);
        flui_rendering::RenderUpdateImpact::LAYOUT
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
            default_extent_estimate: self.default_extent_estimate,
            virtualizer: self.virtualizer.clone(),
            logical_to_slot: BTreeMap::new(), // transient — reset each pass
            pending_correction: self.pending_correction,
            attached_child_count: self.attached_child_count,
            warned_unbounded: None,
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
            &mut self.attached_child_count,
            &mut self.warned_unbounded,
            &constraints,
            ctx,
            // Absent strategy: emit a request via the request-strategy seam.
            // The element tree services it between layout passes of this
            // frame's fixpoint.  `dense_count` is ignored — the element tree
            // decides the insert position.
            &mut |logical_i, _dense_count, _box_constraints, ctx| {
                ctx.request_child_build(logical_i)
            },
        );
        // Signal the retained band to the element tree via the pending_retain_bands
        // channel.  The binding layer forwards this to `SparseChildren::retain_band`
        // between layout passes of this frame's fixpoint, evicting everything
        // outside the band.
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
        let cloned = list.clone();
        assert_eq!(cloned.item_count, 100);
        assert_eq!(cloned.pending_correction, 8.0);
        // logical_to_slot is reset on clone (transient state).
        assert!(cloned.logical_to_slot.is_empty());
    }

    #[test]
    fn set_item_count_updates_field() {
        let mut list = make_list();
        assert_eq!(
            list.set_item_count(42),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );
        assert_eq!(list.item_count, 42);
    }
}

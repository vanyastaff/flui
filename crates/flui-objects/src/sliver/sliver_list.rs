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

use flui_foundation::Variable;
use flui_foundation::inspect::{Fields, Inspect, Unit};

use flui_rendering::{
    constraints::{SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverMultiBoxAdaptorParentData,
    pipeline::LocalRenderInvalidationHandle,
    protocol::ChildLayout,
    traits::RenderSliver,
    virtualization::Virtualizer,
};

use super::pinned::{PinnedChildren, PinnedPlace};
use super::placement::{Placed, Placement, PlacementTrack};
use super::virtualized_band::{item_extent_from_virtualizer, walk_virtualizer_band};

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
/// # Child manager
///
/// The manager that services child-build requests is
/// `SliverAdaptorElement<RenderSliverList>` (`flui-view`); this object is the
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
    collection: Option<flui_rendering::semantics::SemanticsCollection>,
    /// The collection's accessible name, published on its list or grid node.
    collection_label: Option<std::sync::Arc<str>>,

    /// Estimate currently assigned to unmeasured children.
    default_extent_estimate: f64,

    // ── virtualization state ─────────────────────────────────────────────────
    /// Protocol-agnostic windowing engine.
    virtualizer: Virtualizer,

    /// Logical → dense-slot map rebuilt from parent-data on every pass.
    /// Kept as a field to reuse the allocation across passes.
    logical_to_slot: BTreeMap<usize, usize>,

    // ── anchor correction ───────────────────────────────────────────────────
    /// Accumulated anchor-correction delta not yet emitted to the viewport.
    pending_correction: f64,

    // ── hit-test support ────────────────────────────────────────────────────
    /// Dense child count committed after the last layout pass. Used by the
    /// `&self` hit-test reverse-walk which cannot re-read `ctx.child_count()`.
    attached_child_count: usize,

    /// The item count already warned about for an unbounded main axis, so the
    /// truncation warns once rather than once per frame.
    warned_unbounded: Option<usize>,

    // ── placement ───────────────────────────────────────────────────────────
    /// Animates the children of an animated list into their new positions.
    placement: PlacementTrack,
    /// The children the last pass positioned. Kept to reuse the allocation.
    placed: Vec<Placed>,
    /// The pinned residents outside the band (`set_pinned_indices`).
    pinned: PinnedChildren,
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
    pub fn new(item_count: usize, default_extent_estimate: f64) -> Self {
        assert!(
            default_extent_estimate.is_finite() && default_extent_estimate > 0.0,
            "default_extent_estimate must be finite and positive, got {default_extent_estimate}",
        );
        Self {
            item_count,
            collection: None,
            collection_label: None,
            default_extent_estimate,
            virtualizer: Virtualizer::new(item_count, default_extent_estimate),
            logical_to_slot: BTreeMap::new(),
            pending_correction: 0.0,
            attached_child_count: 0,
            warned_unbounded: None,
            placement: PlacementTrack::default(),
            placed: Vec::new(),
            pinned: PinnedChildren::default(),
        }
    }

    /// The known item count (the data source length as last told).
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Names the collection for assistive technology: the label of the list
    /// or grid node this sliver publishes.
    pub fn set_collection_label(
        &mut self,
        label: Option<&std::sync::Arc<str>>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.collection_label.as_ref() == label {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.collection_label = label.cloned();
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Overrides the declared accessible member count independently of logical
    /// slots and residency. `None` declares an unknown total.
    pub fn set_semantic_set_size(
        &mut self,
        count: Option<usize>,
    ) -> flui_rendering::RenderUpdateImpact {
        use flui_rendering::semantics::{SemanticsCollection, SemanticsCollectionKind};
        let collection = SemanticsCollection::new(SemanticsCollectionKind::List, count);
        if self.collection == Some(collection) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.collection = Some(collection);
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Updates the known item count.  Call when the data source length changes.
    pub fn set_item_count(&mut self, count: usize) -> flui_rendering::RenderUpdateImpact {
        if self.item_count == count {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.item_count = count;
        self.virtualizer.set_count(count);
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Updates the estimate for unmeasured children without discarding
    /// measurements already committed by layout.
    pub fn set_default_extent_estimate(
        &mut self,
        estimate: f64,
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

    /// Animate children into new positions after the owner changes its
    /// roster (see [`Placement`]); `None` places them at once.
    pub fn set_placement(
        &mut self,
        placement: Option<&Placement>,
    ) -> flui_rendering::RenderUpdateImpact {
        self.placement.set(placement)
    }

    /// The logical indices of pinned residents outside the band. Each is laid
    /// out after the band at its estimated offset, adds nothing to the
    /// geometry, the anchor or the measured extents, and is skipped by paint
    /// and hit testing; semantics keeps it.
    pub fn set_pinned_indices(
        &mut self,
        pinned: &std::collections::BTreeSet<usize>,
    ) -> flui_rendering::RenderUpdateImpact {
        self.pinned.set(pinned)
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
            collection: self.collection,
            collection_label: self.collection_label.clone(),
            default_extent_estimate: self.default_extent_estimate,
            virtualizer: self.virtualizer.clone(),
            logical_to_slot: BTreeMap::new(), // transient — reset each pass
            pending_correction: self.pending_correction,
            attached_child_count: self.attached_child_count,
            warned_unbounded: None,
            placement: self.placement.clone(),
            placed: Vec::new(),
            pinned: self.pinned.clone(),
        }
    }
}

// ============================================================================
// Inspect + capability impls
// ============================================================================

impl Inspect for RenderSliverList {
    fn kind(&self) -> &'static str {
        "RenderSliverList"
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        out.field("item_count", &self.item_count);
        out.field("attached_child_count", &self.attached_child_count);
        out.field("pending_correction", &self.pending_correction)
            .unit(Unit::LogicalPx);
    }
}

// ============================================================================
// RenderSliver impl
// ============================================================================

impl RenderSliver for RenderSliverList {
    type Arity = Variable;
    type ParentData = SliverMultiBoxAdaptorParentData;

    fn describe_semantics_configuration(
        &self,
        config: &mut flui_rendering::semantics::SemanticsConfiguration,
    ) {
        use flui_rendering::semantics::{SemanticsCollection, SemanticsCollectionKind};
        config.set_lazy_collection(self.collection.unwrap_or_else(|| {
            SemanticsCollection::new(SemanticsCollectionKind::List, Some(self.item_count))
        }));
        if let Some(label) = &self.collection_label {
            config.set_label(label.as_ref());
        }
    }

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> flui_rendering::RenderResult<SliverGeometry> {
        let constraints = *ctx.constraints();

        let mut requested = false;
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
                let outcome = ctx.request_child_build(logical_i);
                requested |= matches!(outcome, ChildLayout::Scheduled);
                outcome
            },
            &mut self.placed,
        )?;
        if self.placement.is_active() {
            let settled = !requested && geometry.scroll_offset_correction.is_none();
            self.placement
                .place(&self.placed, settled, |child, main, _cross| {
                    ctx.position_child(
                        child.slot,
                        child_paint_offset(&constraints, &geometry, main, child.extent),
                    );
                });
        }
        let item_count = self.item_count;
        let virtualizer = &self.virtualizer;
        let pinned_constraints = constraints.as_box_constraints(0.0, f64::INFINITY, None);
        self.pinned.lay_out(cache_first..cache_last, ctx, |index| {
            (index < item_count).then(|| {
                let layout_offset = virtualizer.offset_of(index);
                let extent = item_extent_from_virtualizer(virtualizer, index);
                PinnedPlace {
                    constraints: pinned_constraints,
                    layout_offset,
                    paint_offset: child_paint_offset(
                        &constraints,
                        &geometry,
                        layout_offset,
                        extent,
                    ),
                }
            })
        })?;
        // Signal the retained band to the element tree via the pending_retain_bands
        // channel.  The binding layer forwards this to `SparseChildren::retain_band`
        // between layout passes of this frame's fixpoint, evicting everything
        // outside the band.
        ctx.emit_retain_band(cache_first, cache_last);
        Ok(geometry)
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        // A pinned child is placed, so only this skip keeps it off screen.
        for slot in 0..ctx.child_count() {
            if !self.pinned.is_pinned_slot(slot) {
                ctx.paint_child(slot);
            }
        }
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.placement.attach(handle);
    }

    fn detach(&mut self) {
        self.placement.detach();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        for slot in (0..self.attached_child_count).rev() {
            if !self.pinned.is_pinned_slot(slot) && ctx.hit_test_child_at_layout_offset(slot) {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

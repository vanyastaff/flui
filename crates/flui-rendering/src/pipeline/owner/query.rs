//! Memoizing intrinsic/dry-layout query walks and the per-node slot type.
//!
//! These walks share the same take-out-slot skeleton: each node is moved
//! OUT of its slot while its own computation runs, making re-entry through
//! a cyclic child link detectable rather than UB.

use flui_foundation::RenderId;
#[cfg(any(test, feature = "testing"))]
use rustc_hash::FxHashMap;

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::parent_data::ParentData;
use crate::pipeline::phase::PipelinePhase;
use crate::storage::RenderTree;

use super::{
    PipelineOwner,
    poison::{LayoutFailureKind, LayoutPoison},
    subtree_arena::ensure_stack,
};

// ============================================================================
// Phase-generic query methods on PipelineOwner
// ============================================================================

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// One intrinsic dimension of a box subtree, memoized per node.
    ///
    /// The walk mirrors Flutter's `getMinIntrinsicWidth`-family wrapper
    /// layer (box.dart `_computeIntrinsics`): every node's answer for
    /// `(dimension, extent)` is cached in its `RenderState` layout
    /// cache, and `mark_needs_layout` clears the cache with
    /// boundary-crossing escalation. Repeated probes of the same child
    /// at the same extent — the canonical N-child container pattern —
    /// cost one computation each.
    ///
    /// Average O(subtree) on a cold cache, O(1) per cached node;
    /// worst case adds the hash-collision factor of the per-node maps.
    ///
    /// # Errors
    ///
    /// [`RenderError::NodeNotFound`](crate::error::RenderError::NodeNotFound)
    /// for a stale/foreign id,
    /// [`RenderError::ProtocolMismatch`](crate::error::RenderError::ProtocolMismatch)
    /// if the subtree contains a sliver node (box intrinsics are
    /// undefined there).
    pub fn box_intrinsic_dimension(
        &mut self,
        id: RenderId,
        dimension: crate::storage::IntrinsicDimension,
        extent: f32,
    ) -> crate::error::RenderResult<f32> {
        #[cfg(any(test, feature = "testing"))]
        let parent_data_seeds = self.parent_data_seeds.clone();
        let Self {
            render_tree,
            layout_poison,
            ..
        } = self;
        let mut slots = acquire_query_slots(render_tree, id)?;
        let mut cx = QueryPoisonCx::new(layout_poison);
        let result = intrinsic_query(
            &mut slots,
            &mut cx,
            id,
            dimension,
            extent,
            #[cfg(any(test, feature = "testing"))]
            &parent_data_seeds,
            #[cfg(not(any(test, feature = "testing")))]
            &(),
        );
        // The walk root's own failure is recorded exactly like a dirty
        // root's in `run_layout`: the failure closures only see
        // parent/child pairs, so the root of the query must be counted
        // here or it would never accrue budget itself.
        if let Err(e) = &result {
            cx.note_failure(id, id, e);
        }
        drop(slots);
        let QueryPoisonCx {
            failures,
            successes,
            ..
        } = cx;
        apply_query_poison_drain(render_tree, layout_poison, failures, successes);
        result
    }

    /// The size a box subtree WOULD take under `constraints`, memoized
    /// per `(node, constraints)` — Flutter's `getDryLayout`.
    ///
    /// # Errors
    ///
    /// Same surface as [`Self::box_intrinsic_dimension`].
    pub fn box_dry_layout(
        &mut self,
        id: RenderId,
        constraints: crate::constraints::BoxConstraints,
    ) -> crate::error::RenderResult<flui_types::Size> {
        #[cfg(any(test, feature = "testing"))]
        let parent_data_seeds = self.parent_data_seeds.clone();
        let Self {
            render_tree,
            layout_poison,
            ..
        } = self;
        let mut slots = acquire_query_slots(render_tree, id)?;
        let mut cx = QueryPoisonCx::new(layout_poison);
        let result = dry_layout_query(
            &mut slots,
            &mut cx,
            id,
            constraints,
            #[cfg(any(test, feature = "testing"))]
            &parent_data_seeds,
            #[cfg(not(any(test, feature = "testing")))]
            &(),
        );
        drop(slots);
        let QueryPoisonCx {
            failures,
            successes,
            ..
        } = cx;
        apply_query_poison_drain(render_tree, layout_poison, failures, successes);
        result
    }

    /// The dry baseline of a box node for `constraints`, memoized per
    /// `(constraints, baseline)` — Flutter's `getDryBaseline`. The
    /// computed answer may be `None` ("no baseline"); that answer is
    /// cached too.
    ///
    /// # Errors
    ///
    /// Same surface as [`Self::box_intrinsic_dimension`].
    pub fn box_dry_baseline(
        &mut self,
        id: RenderId,
        constraints: crate::constraints::BoxConstraints,
        baseline: crate::traits::TextBaseline,
    ) -> crate::error::RenderResult<Option<f32>> {
        #[cfg(any(test, feature = "testing"))]
        let parent_data_seeds = self.parent_data_seeds.clone();
        let Self {
            render_tree,
            layout_poison,
            ..
        } = self;
        let mut slots = acquire_query_slots(render_tree, id)?;
        let mut cx = QueryPoisonCx::new(layout_poison);
        let result = dry_baseline_query(
            &mut slots,
            &mut cx,
            id,
            constraints,
            baseline,
            #[cfg(any(test, feature = "testing"))]
            &parent_data_seeds,
            #[cfg(not(any(test, feature = "testing")))]
            &(),
        );
        drop(slots);
        let QueryPoisonCx {
            failures,
            successes,
            ..
        } = cx;
        apply_query_poison_drain(render_tree, layout_poison, failures, successes);
        result
    }
}

// ============================================================================
// QuerySlot, poison context, and free query functions
// ============================================================================

/// Acquires the take-out borrow map for a memoizing query walk:
/// disjoint `&mut` over the subtree (the same `get_subtree_mut`
/// primitive the layout walk uses) plus each node's child-id
/// snapshot. A node is moved OUT of its slot while its own
/// computation runs, so re-entry — a child-link cycle — is
/// detectable instead of UB.
///
/// Free function (not a method) so callers can hold a
/// `&mut LayoutPoison` from the same owner across the walk.
fn acquire_query_slots(
    render_tree: &mut RenderTree,
    id: RenderId,
) -> crate::error::RenderResult<rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>> {
    let ids = render_tree.collect_subtree_ids(id);
    let nodes = render_tree
        .get_subtree_mut(&ids)
        .ok_or(crate::error::RenderError::NodeNotFound(id))?;
    Ok(ids
        .iter()
        .zip(nodes)
        .map(|(&node_id, node)| {
            let children = node.children().to_vec();
            (
                node_id,
                QuerySlot {
                    node: Some(node),
                    children,
                },
            )
        })
        .collect())
}

/// One node's slot in a memoizing query walk: the disjoint `&mut`
/// borrow plus a snapshot of the node's child ids. The node is moved
/// OUT (`node.take()`) while its own computation runs, so re-entry —
/// which only a cyclic child link can produce — is detected instead of
/// aliasing the borrow.
pub(super) struct QuerySlot<'a> {
    pub(super) node: Option<&'a mut crate::storage::RenderNode>,
    pub(super) children: Vec<RenderId>,
}

/// Poison context threaded through the query walks: a read-only view
/// of the owner's [`LayoutPoison`] for skip checks plus the failure /
/// success sinks the walk records into. Kept as one bundle so the
/// recursive free functions stay readable. Records are drained into
/// the owner's poison table by [`apply_query_poison_drain`] after the
/// walk's borrows are released.
pub(super) struct QueryPoisonCx<'a> {
    poison: &'a LayoutPoison,
    failures: Vec<(RenderId, RenderId, LayoutFailureKind)>,
    successes: Vec<RenderId>,
}

impl<'a> QueryPoisonCx<'a> {
    fn new(poison: &'a LayoutPoison) -> Self {
        Self {
            poison,
            failures: Vec::new(),
            successes: Vec::new(),
        }
    }

    /// True while `id` is layout-poisoned (the intrinsic query must
    /// skip it, returning its last-good cached value or 0.0).
    #[inline]
    fn is_poisoned(&self, id: RenderId) -> bool {
        self.poison.is_poisoned(id)
    }

    /// Records a child-query failure swallowed at a recursion closure:
    /// `parent` is the node whose own measurement invoked the query,
    /// `failed` the child whose query returned `err`.
    fn note_failure(
        &mut self,
        parent: RenderId,
        failed: RenderId,
        err: &crate::error::RenderError,
    ) {
        self.failures
            .push((parent, failed, LayoutFailureKind::of(err)));
    }

    /// Records a child-query success, but only for a node with open
    /// (not yet poisoned) failure records — the poison skip returns
    /// cached/0.0 through the same `Ok` channel, and that stand-in
    /// must not be misread as a recovery.
    fn note_success(&mut self, id: RenderId) {
        if self.poison.has_open_failures(id) {
            self.successes.push(id);
        }
    }
}

/// Feeds a finished query walk's failure/success sinks into the
/// owner's poison table. On a 0 → 1 poison transition the failed
/// node's `NEEDS_LAYOUT` is cleared (its last committed geometry
/// stands, so paint shows last-good content rather than skipping the
/// node forever); parent flags are NOT touched here — unlike the
/// layout walk, an out-of-layout probe never set them.
///
/// Takes the sinks unpacked (rather than the whole [`QueryPoisonCx`])
/// so the caller's shared borrow of the poison table has ended before
/// `layout_poison` is taken mutably.
fn apply_query_poison_drain(
    render_tree: &RenderTree,
    layout_poison: &mut LayoutPoison,
    failures: Vec<(RenderId, RenderId, LayoutFailureKind)>,
    successes: Vec<RenderId>,
) {
    for succeeded in successes {
        layout_poison.note_success(succeeded);
    }
    for (parent, failed, kind, first_report) in layout_poison.note_failures(failures) {
        if let Some(node) = render_tree.get(failed) {
            node.clear_needs_layout();
        }
        if first_report {
            tracing::error!(
                ?failed,
                ?parent,
                ?kind,
                "layout poison engaged: intrinsic query failed with a structural \
                 error (or exhausted its retry budget); the node is skipped in \
                 later queries until freshly invalidated (mark_needs_layout). \
                 Its last-good cached value (or 0.0) stands in.",
            );
        } else {
            tracing::debug!(
                ?failed,
                ?parent,
                ?kind,
                "layout poison re-engaged after a fresh invalidation; the \
                 node's intrinsic query still fails and stays skipped.",
            );
        }
    }
}

/// Builds the per-child parent-data slice for the current node's children,
/// reading from the slot map (production) with harness seed overlay (test/testing).
///
/// The slice is built from owned `Box<dyn ParentData>` values so it can
/// coexist with the `&mut slots` that the recursion closure needs.
fn build_child_parent_data(
    slots: &rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    children: &[RenderId],
    #[cfg(any(test, feature = "testing"))] seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] _seeds: &(),
) -> Vec<Option<Box<dyn ParentData>>> {
    children
        .iter()
        .map(|child_id| {
            // Harness seeds overlay production parent data so headless tests
            // can provide widget-level configuration without an element tree.
            #[cfg(any(test, feature = "testing"))]
            if let Some(seed) = seeds.get(child_id) {
                return Some(seed.to_box());
            }
            // Production: clone from the child node's committed parent data.
            slots
                .get(child_id)
                .and_then(|s| s.node.as_ref())
                .and_then(|n| n.parent_data())
                .map(dyn_clone::clone_box)
        })
        .collect()
}

/// Borrows the owned parent-data boxes as a `&[Option<&dyn ParentData>]`
/// suitable for passing to `*_raw` methods.
fn parent_data_refs(owned: &[Option<Box<dyn ParentData>>]) -> Vec<Option<&dyn ParentData>> {
    owned.iter().map(|opt| opt.as_deref()).collect()
}

/// Recursive memoized intrinsic query over the take-out slot map.
///
/// Per node: cache peek → on miss, run the object's `intrinsic_raw`
/// with a child callback that recurses through this same function →
/// store the result. Errors inside the child callback are stashed and
/// re-raised after the object call returns (the raw callback channel
/// is infallible by design — same convention as the hit-test walk).
///
/// `cx` carries the layout-poison table: a poisoned node is skipped
/// (its last-good cached value, or 0.0 when it never succeeded, stands
/// in) and child-query failures feed the same retry budget the layout
/// walk uses.
pub(super) fn intrinsic_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f32,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<f32> {
    ensure_stack(|| intrinsic_query_impl(slots, cx, id, dimension, extent, parent_data_seeds))
}

/// Body of [`intrinsic_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn intrinsic_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f32,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<f32> {
    let Some(slot) = slots.get_mut(&id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };
    let Some(node) = slot.node.take() else {
        // Only reachable through a cyclic child link: the node's own
        // computation is still on the stack. Degenerate-but-defined in
        // release; loud in debug (collect_subtree_ids already refuses
        // to loop, so the cycle must close through duplicate child
        // indices).
        debug_assert!(
            false,
            "intrinsic query re-entered node {id:?} mid-computation — cyclic child links"
        );
        return Ok(0.0);
    };

    // Layout-poison skip: do not re-measure a node that exhausted its
    // retry budget until it is freshly invalidated. The cache is only
    // written on success, so a hit is a real previously computed
    // answer; a miss falls back to 0.0 — the same value the
    // error-swallow path used before the poison mechanism existed. The
    // slot is restored before returning, exactly like the error path
    // below.
    if cx.is_poisoned(id) {
        let value = node
            .as_box()
            .and_then(|entry| {
                entry
                    .state()
                    .layout_cache()
                    .peek_intrinsic(dimension, extent)
            })
            .unwrap_or(0.0);
        if let Some(slot) = slots.get_mut(&id) {
            slot.node = Some(node);
        }
        return Ok(value);
    }

    let children = slot.children.clone();

    let result = (|| {
        let Some(entry) = node.as_box_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol: "sliver",
                constraints_protocol: "box",
            });
        };
        if let Some(hit) = entry
            .state()
            .layout_cache()
            .peek_intrinsic(dimension, extent)
        {
            return Ok(hit);
        }

        // Build the per-child parent-data slice before creating the recursion
        // closure. The owned boxes coexist with the &mut slots the closure needs.
        let child_parent_data_owned = build_child_parent_data(slots, &children, parent_data_seeds);
        let child_parent_data_refs = parent_data_refs(&child_parent_data_owned);

        let mut child_err: Option<crate::error::RenderError> = None;
        let value = {
            let child_err = &mut child_err;
            let mut child_query =
                |index: usize, dim: crate::storage::IntrinsicDimension, ext: f32| -> f32 {
                    let Some(&child_id) = children.get(index) else {
                        let err = crate::error::RenderError::contract_violation(
                            "intrinsic child query",
                            "child index out of range for this node's children",
                        );
                        cx.note_failure(id, id, &err);
                        child_err.get_or_insert(err);
                        return 0.0;
                    };
                    match intrinsic_query(slots, cx, child_id, dim, ext, parent_data_seeds) {
                        Ok(v) => {
                            cx.note_success(child_id);
                            v
                        }
                        Err(err) => {
                            cx.note_failure(id, child_id, &err);
                            child_err.get_or_insert(err);
                            0.0
                        }
                    }
                };
            entry.render_object().intrinsic_raw(
                dimension,
                extent,
                children.len(),
                &child_parent_data_refs,
                &mut child_query,
            )
        };
        if let Some(err) = child_err {
            return Err(err);
        }
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_intrinsic(dimension, extent, value);
        Ok(value)
    })();

    // Restore the slot even on the error path — sibling queries in the
    // same walk must still find the node.
    if let Some(slot) = slots.get_mut(&id) {
        slot.node = Some(node);
    }
    result
}

/// Recursive memoized dry-layout query; same skeleton as
/// [`intrinsic_query`] with `(constraints → Size)` payloads.
///
/// `cx` is threaded through only so intrinsic sub-queries keep their
/// poison skip; dry-layout's own collapse sites do not feed the retry
/// budget (out of the intrinsic channel's scope).
pub(super) fn dry_layout_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<flui_types::Size> {
    ensure_stack(|| dry_layout_query_impl(slots, cx, id, constraints, parent_data_seeds))
}

/// Body of [`dry_layout_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn dry_layout_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<flui_types::Size> {
    let Some(slot) = slots.get_mut(&id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };
    let Some(node) = slot.node.take() else {
        debug_assert!(
            false,
            "dry-layout query re-entered node {id:?} mid-computation — cyclic child links"
        );
        return Ok(flui_types::Size::ZERO);
    };
    let children = slot.children.clone();

    let result = (|| {
        let Some(entry) = node.as_box_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol: "sliver",
                constraints_protocol: "box",
            });
        };
        if let Some(hit) = entry.state().layout_cache().peek_dry_layout(constraints) {
            return Ok(hit);
        }

        // Build the per-child parent-data slice strictly before the recursion
        // closure — no new re-entrancy surface, no aliasing with &mut slots.
        let child_parent_data_owned = build_child_parent_data(slots, &children, parent_data_seeds);
        let child_parent_data_refs = parent_data_refs(&child_parent_data_owned);

        let mut child_err: Option<crate::error::RenderError> = None;
        let value = {
            let child_err = &mut child_err;
            let mut child_query = |index: usize,
                                   request: crate::context::DryLayoutChildRequest|
             -> crate::context::DryLayoutChildResponse {
                use crate::context::{DryLayoutChildRequest, DryLayoutChildResponse};
                let Some(&child_id) = children.get(index) else {
                    child_err.get_or_insert(crate::error::RenderError::contract_violation(
                        "dry-layout child query",
                        "child index out of range for this node's children",
                    ));
                    return match request {
                        DryLayoutChildRequest::DryLayout(_) => {
                            DryLayoutChildResponse::DryLayout(flui_types::Size::ZERO)
                        }
                        DryLayoutChildRequest::Intrinsic(_, _) => {
                            DryLayoutChildResponse::Intrinsic(0.0)
                        }
                    };
                };
                match request {
                    DryLayoutChildRequest::DryLayout(c) => {
                        match dry_layout_query(slots, cx, child_id, c, parent_data_seeds) {
                            Ok(v) => DryLayoutChildResponse::DryLayout(v),
                            Err(err) => {
                                child_err.get_or_insert(err);
                                DryLayoutChildResponse::DryLayout(flui_types::Size::ZERO)
                            }
                        }
                    }
                    DryLayoutChildRequest::Intrinsic(dim, e) => {
                        match intrinsic_query(slots, cx, child_id, dim, e, parent_data_seeds) {
                            Ok(v) => DryLayoutChildResponse::Intrinsic(v),
                            Err(err) => {
                                child_err.get_or_insert(err);
                                DryLayoutChildResponse::Intrinsic(0.0)
                            }
                        }
                    }
                }
            };
            entry.render_object().dry_layout_raw(
                constraints,
                children.len(),
                &child_parent_data_refs,
                &mut child_query,
            )
        };
        if let Some(err) = child_err {
            return Err(err);
        }
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_dry_layout(constraints, value);
        Ok(value)
    })();

    if let Some(slot) = slots.get_mut(&id) {
        slot.node = Some(node);
    }
    result
}

/// Recursive memoized dry-baseline query; same skeleton as
/// [`dry_layout_query`] with `(constraints, baseline → Option<f32>)`
/// payloads. `cx` is threaded through for intrinsic sub-queries only
/// (see [`dry_layout_query`]).
pub(super) fn dry_baseline_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    baseline: crate::traits::TextBaseline,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<Option<f32>> {
    ensure_stack(|| {
        dry_baseline_query_impl(slots, cx, id, constraints, baseline, parent_data_seeds)
    })
}

/// Body of [`dry_baseline_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn dry_baseline_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    baseline: crate::traits::TextBaseline,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<Option<f32>> {
    let Some(slot) = slots.get_mut(&id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };
    let Some(node) = slot.node.take() else {
        debug_assert!(
            false,
            "dry-baseline query re-entered node {id:?} mid-computation — cyclic child links"
        );
        return Ok(None);
    };
    let children = slot.children.clone();

    let result = (|| {
        let Some(entry) = node.as_box_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol: "sliver",
                constraints_protocol: "box",
            });
        };
        if let Some(hit) = entry
            .state()
            .layout_cache()
            .peek_dry_baseline(constraints, baseline)
        {
            return Ok(hit);
        }

        // Build the per-child parent-data slice strictly before the recursion
        // closure — no new re-entrancy surface, no aliasing with &mut slots.
        let child_parent_data_owned = build_child_parent_data(slots, &children, parent_data_seeds);
        let child_parent_data_refs = parent_data_refs(&child_parent_data_owned);

        let mut child_err: Option<crate::error::RenderError> = None;
        let value = {
            let child_err = &mut child_err;
            let mut child_query = |index: usize,
                                   request: crate::context::DryBaselineChildRequest|
             -> crate::context::DryBaselineChildResponse {
                use crate::context::{DryBaselineChildRequest, DryBaselineChildResponse};
                let Some(&child_id) = children.get(index) else {
                    child_err.get_or_insert(crate::error::RenderError::contract_violation(
                        "dry-baseline child query",
                        "child index out of range for this node's children",
                    ));
                    return match request {
                        DryBaselineChildRequest::Baseline(_, _) => {
                            DryBaselineChildResponse::Baseline(None)
                        }
                        DryBaselineChildRequest::DryLayout(_) => {
                            DryBaselineChildResponse::DryLayout(flui_types::Size::ZERO)
                        }
                        DryBaselineChildRequest::Intrinsic(_, _) => {
                            DryBaselineChildResponse::Intrinsic(0.0)
                        }
                    };
                };
                match request {
                    DryBaselineChildRequest::Baseline(c, b) => {
                        match dry_baseline_query(slots, cx, child_id, c, b, parent_data_seeds) {
                            Ok(v) => DryBaselineChildResponse::Baseline(v),
                            Err(err) => {
                                child_err.get_or_insert(err);
                                DryBaselineChildResponse::Baseline(None)
                            }
                        }
                    }
                    DryBaselineChildRequest::DryLayout(c) => {
                        match dry_layout_query(slots, cx, child_id, c, parent_data_seeds) {
                            Ok(v) => DryBaselineChildResponse::DryLayout(v),
                            Err(err) => {
                                child_err.get_or_insert(err);
                                DryBaselineChildResponse::DryLayout(flui_types::Size::ZERO)
                            }
                        }
                    }
                    DryBaselineChildRequest::Intrinsic(dim, e) => {
                        match intrinsic_query(slots, cx, child_id, dim, e, parent_data_seeds) {
                            Ok(v) => DryBaselineChildResponse::Intrinsic(v),
                            Err(err) => {
                                child_err.get_or_insert(err);
                                DryBaselineChildResponse::Intrinsic(0.0)
                            }
                        }
                    }
                }
            };
            entry.render_object().dry_baseline_raw(
                constraints,
                baseline,
                children.len(),
                &child_parent_data_refs,
                &mut child_query,
            )
        };
        if let Some(err) = child_err {
            return Err(err);
        }
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_dry_baseline(constraints, baseline, value);
        Ok(value)
    })();

    if let Some(slot) = slots.get_mut(&id) {
        slot.node = Some(node);
    }
    result
}

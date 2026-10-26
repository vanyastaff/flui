//! Memoizing intrinsic/dry-layout query walks.
//!
//! These walks share one skeleton: a [`QueryWalk`] lease on the tree enters each node as the walk
//! reaches it and holds its in-flight bit for the node's own computation, making re-entry through
//! a cyclic child link detectable rather than UB.

use flui_foundation::RenderId;
#[cfg(any(test, feature = "testing"))]
use rustc_hash::FxHashMap;

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::parent_data::ParentData;
use crate::pipeline::TextLender;
use crate::pipeline::phase::PipelinePhase;
use crate::storage::RenderTree;

use super::{
    PipelineOwner,
    poison::{LayoutFailureKind, LayoutPoison},
    subtree_arena::{QueryRefusal, QueryWalk, ensure_stack},
};

// ============================================================================
// Phase-generic query methods on PipelineOwner
// ============================================================================

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// One intrinsic dimension of a box subtree, memoized per node.
    ///
    /// Every node's answer for `(dimension, extent)` is cached in its
    /// `RenderState` layout cache, and `mark_needs_layout` clears the cache with
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
        extent: f64,
    ) -> crate::error::RenderResult<f64> {
        // The query fills layout caches and may poison (and unflag) nodes.
        self.scheduler.forget_layout_walks();
        #[cfg(any(test, feature = "testing"))]
        let parent_data_seeds = self.parent_data_seeds.clone();
        let text = self.text_handle();
        let Self {
            render_tree,
            layout_poison,
            text_measurers,
            text_sizing,
            ..
        } = self;
        if !render_tree.contains(id) {
            return Err(crate::error::RenderError::NodeNotFound(id));
        }
        let walk = QueryWalk::new(render_tree);
        let mut cx = QueryPoisonCx::new(
            layout_poison,
            TextLender::new(text.cell(), text_measurers, text_sizing),
        );
        let result = intrinsic_query(
            &walk,
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
        let QueryPoisonCx {
            failures,
            successes,
            ..
        } = cx;
        apply_query_poison_drain(render_tree, layout_poison, failures, successes);
        result
    }

    /// The size a box subtree WOULD take under `constraints`, memoized
    /// per `(node, constraints)` (a dry layout).
    ///
    /// # Errors
    ///
    /// Same surface as [`Self::box_intrinsic_dimension`].
    pub fn box_dry_layout(
        &mut self,
        id: RenderId,
        constraints: crate::constraints::BoxConstraints,
    ) -> crate::error::RenderResult<flui_foundation::geometry::Size> {
        // The query fills layout caches and may poison (and unflag) nodes.
        self.scheduler.forget_layout_walks();
        #[cfg(any(test, feature = "testing"))]
        let parent_data_seeds = self.parent_data_seeds.clone();
        let text = self.text_handle();
        let Self {
            render_tree,
            layout_poison,
            text_measurers,
            text_sizing,
            ..
        } = self;
        if !render_tree.contains(id) {
            return Err(crate::error::RenderError::NodeNotFound(id));
        }
        let walk = QueryWalk::new(render_tree);
        let mut cx = QueryPoisonCx::new(
            layout_poison,
            TextLender::new(text.cell(), text_measurers, text_sizing),
        );
        let result = dry_layout_query(
            &walk,
            &mut cx,
            id,
            constraints,
            #[cfg(any(test, feature = "testing"))]
            &parent_data_seeds,
            #[cfg(not(any(test, feature = "testing")))]
            &(),
        );
        let QueryPoisonCx {
            failures,
            successes,
            ..
        } = cx;
        apply_query_poison_drain(render_tree, layout_poison, failures, successes);
        result
    }

    /// The dry baseline of a box node for `constraints`, memoized per
    /// `(constraints, baseline)` (a dry baseline). The
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
    ) -> crate::error::RenderResult<Option<f64>> {
        // The query fills layout caches and may poison (and unflag) nodes.
        self.scheduler.forget_layout_walks();
        #[cfg(any(test, feature = "testing"))]
        let parent_data_seeds = self.parent_data_seeds.clone();
        let text = self.text_handle();
        let Self {
            render_tree,
            layout_poison,
            text_measurers,
            text_sizing,
            ..
        } = self;
        if !render_tree.contains(id) {
            return Err(crate::error::RenderError::NodeNotFound(id));
        }
        let walk = QueryWalk::new(render_tree);
        let mut cx = QueryPoisonCx::new(
            layout_poison,
            TextLender::new(text.cell(), text_measurers, text_sizing),
        );
        let result = dry_baseline_query(
            &walk,
            &mut cx,
            id,
            constraints,
            baseline,
            #[cfg(any(test, feature = "testing"))]
            &parent_data_seeds,
            #[cfg(not(any(test, feature = "testing")))]
            &(),
        );
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
// Poison context and free query functions
// ============================================================================

/// Poison context threaded through the query walks: a read-only view
/// of the owner's [`LayoutPoison`] for skip checks plus the failure /
/// success sinks the walk records into. Kept as one bundle so the
/// recursive free functions stay readable. Records are drained into
/// the owner's poison table by [`apply_query_poison_drain`] after the
/// walk's borrows are released.
pub(super) struct QueryPoisonCx<'a> {
    poison: &'a LayoutPoison,
    /// The UI runtime's text context, lent to every node the query measures;
    /// each loan records the node it was made for.
    text: TextLender<'a>,
    failures: Vec<(
        RenderId,
        RenderId,
        LayoutFailureKind,
        Option<crate::storage::ErasedConstraints>,
    )>,
    successes: Vec<RenderId>,
}

impl<'a> QueryPoisonCx<'a> {
    fn new(poison: &'a LayoutPoison, text: TextLender<'a>) -> Self {
        Self {
            poison,
            text,
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
    /// `failed` the child whose query returned `err`. Intrinsic queries
    /// carry no layout constraints, so the attempt slot is `None` and
    /// the entry's previous last-attempt constraints stay intact.
    fn note_failure(
        &mut self,
        parent: RenderId,
        failed: RenderId,
        err: &crate::error::RenderError,
    ) {
        if err.is_recoverable_layout_error() {
            return;
        }
        self.failures
            .push((parent, failed, LayoutFailureKind::of(err), None));
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
    failures: Vec<(
        RenderId,
        RenderId,
        LayoutFailureKind,
        Option<crate::storage::ErasedConstraints>,
    )>,
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
/// reading from the walk (production) with harness seed overlay (test/testing).
///
/// The slice is built from owned `Box<dyn ParentData>` values so it can
/// coexist with the entered nodes that the recursion closure needs.
fn build_child_parent_data(
    walk: &QueryWalk<'_>,
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
            walk.child_parent_data(*child_id)
        })
        .collect()
}

/// Borrows the owned parent-data boxes as a `&[Option<&dyn ParentData>]`
/// suitable for passing to `*_raw` methods.
fn parent_data_refs(owned: &[Option<Box<dyn ParentData>>]) -> Vec<Option<&dyn ParentData>> {
    owned.iter().map(|opt| opt.as_deref()).collect()
}

/// Recursive memoized intrinsic query over the leased tree.
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
    walk: &QueryWalk<'_>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f64,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<f64> {
    ensure_stack(|| intrinsic_query_impl(walk, cx, id, dimension, extent, parent_data_seeds))
}

/// Body of [`intrinsic_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn intrinsic_query_impl(
    walk: &QueryWalk<'_>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f64,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<f64> {
    let mut held = match walk.enter(id) {
        Ok(held) => held,
        Err(QueryRefusal::Missing) => return Err(crate::error::RenderError::NodeNotFound(id)),
        Err(QueryRefusal::InFlight) => {
            // Only reachable through a cyclic child link: the node's own
            // computation is still on the stack. Degenerate-but-defined in
            // release; loud in debug.
            debug_assert!(
                false,
                "intrinsic query re-entered node {id:?} mid-computation — cyclic child links"
            );
            return Ok(0.0);
        }
    };
    let node = held.node();

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
        return Ok(value);
    }

    let children = node.children().to_vec();

    (|| {
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
        // closure. The owned boxes coexist with the &walk the closure needs.
        let child_parent_data_owned = build_child_parent_data(walk, &children, parent_data_seeds);
        let child_parent_data_refs = parent_data_refs(&child_parent_data_owned);

        let value = {
            let text = cx.text;
            let mut child_query =
                |index: usize, dim: crate::storage::IntrinsicDimension, ext: f64| {
                    let Some(&child_id) = children.get(index) else {
                        let err = crate::error::RenderError::contract_violation(
                            "intrinsic child query",
                            "child index out of range for this node's children",
                        );
                        cx.note_failure(id, id, &err);
                        return Err(err);
                    };
                    match intrinsic_query(walk, cx, child_id, dim, ext, parent_data_seeds) {
                        Ok(v) => {
                            cx.note_success(child_id);
                            Ok(v)
                        }
                        Err(err) => {
                            cx.note_failure(id, child_id, &err);
                            Err(err)
                        }
                    }
                };
            let mut query = crate::context::BoxIntrinsicsCtx::new(
                children.len(),
                &child_parent_data_refs,
                &mut child_query,
                text.source(id),
            );
            entry
                .render_object()
                .intrinsic_raw(dimension, extent, &mut query)
        }?;
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_intrinsic(dimension, extent, value);
        Ok(value)
    })()
}

/// Recursive memoized dry-layout query; same skeleton as
/// [`intrinsic_query`] with `(constraints → Size)` payloads.
///
/// `cx` is threaded through only so intrinsic sub-queries keep their
/// poison skip; dry-layout's own collapse sites do not feed the retry
/// budget (out of the intrinsic channel's scope).
pub(super) fn dry_layout_query(
    walk: &QueryWalk<'_>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<flui_foundation::geometry::Size> {
    ensure_stack(|| dry_layout_query_impl(walk, cx, id, constraints, parent_data_seeds))
}

/// Body of [`dry_layout_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn dry_layout_query_impl(
    walk: &QueryWalk<'_>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<flui_foundation::geometry::Size> {
    let mut held = match walk.enter(id) {
        Ok(held) => held,
        Err(QueryRefusal::Missing) => return Err(crate::error::RenderError::NodeNotFound(id)),
        Err(QueryRefusal::InFlight) => {
            debug_assert!(
                false,
                "dry-layout query re-entered node {id:?} mid-computation — cyclic child links"
            );
            return Ok(flui_foundation::geometry::Size::ZERO);
        }
    };
    let node = held.node();
    let children = node.children().to_vec();

    (|| {
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
        // closure — no new re-entrancy surface, no aliasing with the walk.
        let child_parent_data_owned = build_child_parent_data(walk, &children, parent_data_seeds);
        let child_parent_data_refs = parent_data_refs(&child_parent_data_owned);

        let value = {
            let text = cx.text;
            let mut child_query = |index: usize,
                                   request: crate::context::DryLayoutChildRequest|
             -> crate::error::RenderResult<
                crate::context::DryLayoutChildResponse,
            > {
                use crate::context::{DryLayoutChildRequest, DryLayoutChildResponse};
                let Some(&child_id) = children.get(index) else {
                    return Err(crate::error::RenderError::contract_violation(
                        "dry-layout child query",
                        "child index out of range for this node's children",
                    ));
                };
                match request {
                    DryLayoutChildRequest::DryLayout(c) => {
                        dry_layout_query(walk, cx, child_id, c, parent_data_seeds)
                            .map(DryLayoutChildResponse::DryLayout)
                    }
                    DryLayoutChildRequest::Intrinsic(dim, e) => {
                        intrinsic_query(walk, cx, child_id, dim, e, parent_data_seeds)
                            .map(DryLayoutChildResponse::Intrinsic)
                    }
                    DryLayoutChildRequest::Baseline(c, b) => {
                        dry_baseline_query(walk, cx, child_id, c, b, parent_data_seeds)
                            .map(DryLayoutChildResponse::Baseline)
                    }
                }
            };
            let mut query = crate::context::BoxDryLayoutCtx::new(
                children.len(),
                &child_parent_data_refs,
                &mut child_query,
                text.source(id),
            );
            entry
                .render_object()
                .dry_layout_raw(constraints, &mut query)
        }?;
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_dry_layout(constraints, value);
        Ok(value)
    })()
}

/// Recursive memoized dry-baseline query; same skeleton as
/// [`dry_layout_query`] with `(constraints, baseline → Option<f64>)`
/// payloads. `cx` is threaded through for intrinsic sub-queries only
/// (see [`dry_layout_query`]).
pub(super) fn dry_baseline_query(
    walk: &QueryWalk<'_>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    baseline: crate::traits::TextBaseline,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<Option<f64>> {
    ensure_stack(|| dry_baseline_query_impl(walk, cx, id, constraints, baseline, parent_data_seeds))
}

/// Body of [`dry_baseline_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn dry_baseline_query_impl(
    walk: &QueryWalk<'_>,
    cx: &mut QueryPoisonCx<'_>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    baseline: crate::traits::TextBaseline,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<Option<f64>> {
    let mut held = match walk.enter(id) {
        Ok(held) => held,
        Err(QueryRefusal::Missing) => return Err(crate::error::RenderError::NodeNotFound(id)),
        Err(QueryRefusal::InFlight) => {
            debug_assert!(
                false,
                "dry-baseline query re-entered node {id:?} mid-computation — cyclic child links"
            );
            return Ok(None);
        }
    };
    let node = held.node();
    let children = node.children().to_vec();

    (|| {
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
        // closure — no new re-entrancy surface, no aliasing with the walk.
        let child_parent_data_owned = build_child_parent_data(walk, &children, parent_data_seeds);
        let child_parent_data_refs = parent_data_refs(&child_parent_data_owned);

        let value = {
            let text = cx.text;
            let mut child_query = |index: usize,
                                   request: crate::context::DryBaselineChildRequest|
             -> crate::error::RenderResult<
                crate::context::DryBaselineChildResponse,
            > {
                use crate::context::{DryBaselineChildRequest, DryBaselineChildResponse};
                let Some(&child_id) = children.get(index) else {
                    return Err(crate::error::RenderError::contract_violation(
                        "dry-baseline child query",
                        "child index out of range for this node's children",
                    ));
                };
                match request {
                    DryBaselineChildRequest::Baseline(c, b) => {
                        dry_baseline_query(walk, cx, child_id, c, b, parent_data_seeds)
                            .map(DryBaselineChildResponse::Baseline)
                    }
                    DryBaselineChildRequest::DryLayout(c) => {
                        dry_layout_query(walk, cx, child_id, c, parent_data_seeds)
                            .map(DryBaselineChildResponse::DryLayout)
                    }
                    DryBaselineChildRequest::Intrinsic(dim, e) => {
                        intrinsic_query(walk, cx, child_id, dim, e, parent_data_seeds)
                            .map(DryBaselineChildResponse::Intrinsic)
                    }
                }
            };
            let mut query = crate::context::BoxDryBaselineCtx::new(
                children.len(),
                &child_parent_data_refs,
                &mut child_query,
                text.source(id),
            );
            entry
                .render_object()
                .dry_baseline_raw(constraints, baseline, &mut query)
        }?;
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_dry_baseline(constraints, baseline, value);
        Ok(value)
    })()
}

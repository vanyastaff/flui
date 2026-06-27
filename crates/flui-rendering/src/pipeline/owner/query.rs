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

use crate::pipeline::phase::PipelinePhase;

use super::{PipelineOwner, subtree_arena::child_flex_from_seeds, subtree_arena::ensure_stack};

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
        let mut slots = self.acquire_query_slots(id)?;
        intrinsic_query(
            &mut slots,
            id,
            dimension,
            extent,
            #[cfg(any(test, feature = "testing"))]
            &parent_data_seeds,
            #[cfg(not(any(test, feature = "testing")))]
            &(),
        )
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
        let mut slots = self.acquire_query_slots(id)?;
        dry_layout_query(&mut slots, id, constraints)
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
        let mut slots = self.acquire_query_slots(id)?;
        dry_baseline_query(&mut slots, id, constraints, baseline)
    }

    /// Acquires the take-out borrow map for a memoizing query walk:
    /// disjoint `&mut` over the subtree (the same `get_subtree_mut`
    /// primitive the layout walk uses) plus each node's child-id
    /// snapshot. A node is moved OUT of its slot while its own
    /// computation runs, so re-entry — a child-link cycle — is
    /// detectable instead of UB.
    fn acquire_query_slots(
        &mut self,
        id: RenderId,
    ) -> crate::error::RenderResult<rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>> {
        let ids = self.render_tree.collect_subtree_ids(id);
        let nodes = self
            .render_tree
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
}

// ============================================================================
// QuerySlot and free query functions
// ============================================================================

/// One node's slot in a memoizing query walk: the disjoint `&mut`
/// borrow plus a snapshot of the node's child ids. The node is moved
/// OUT (`node.take()`) while its own computation runs, so re-entry —
/// which only a cyclic child link can produce — is detected instead of
/// aliasing the borrow.
pub(super) struct QuerySlot<'a> {
    pub(super) node: Option<&'a mut crate::storage::RenderNode>,
    pub(super) children: Vec<RenderId>,
}

/// Recursive memoized intrinsic query over the take-out slot map.
///
/// Per node: cache peek → on miss, run the object's `intrinsic_raw`
/// with a child callback that recurses through this same function →
/// store the result. Errors inside the child callback are stashed and
/// re-raised after the object call returns (the raw callback channel
/// is infallible by design — same convention as the hit-test walk).
pub(super) fn intrinsic_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f32,
    #[cfg(any(test, feature = "testing"))] parent_data_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] parent_data_seeds: &(),
) -> crate::error::RenderResult<f32> {
    ensure_stack(|| intrinsic_query_impl(slots, id, dimension, extent, parent_data_seeds))
}

/// Body of [`intrinsic_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn intrinsic_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
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
        let mut child_err: Option<crate::error::RenderError> = None;
        let value = {
            let child_err = &mut child_err;
            let mut child_query =
                |index: usize, dim: crate::storage::IntrinsicDimension, ext: f32| -> f32 {
                    let Some(&child_id) = children.get(index) else {
                        child_err.get_or_insert(crate::error::RenderError::contract_violation(
                            "intrinsic child query",
                            "child index out of range for this node's children",
                        ));
                        return 0.0;
                    };
                    match intrinsic_query(slots, child_id, dim, ext, parent_data_seeds) {
                        Ok(v) => v,
                        Err(err) => {
                            child_err.get_or_insert(err);
                            0.0
                        }
                    }
                };
            let mut child_flex = |index: usize| -> i32 {
                child_flex_from_seeds(parent_data_seeds, &children, index)
            };
            entry.render_object().intrinsic_raw(
                dimension,
                extent,
                children.len(),
                &mut child_query,
                &mut child_flex,
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
pub(super) fn dry_layout_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
) -> crate::error::RenderResult<flui_types::Size> {
    ensure_stack(|| dry_layout_query_impl(slots, id, constraints))
}

/// Body of [`dry_layout_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn dry_layout_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
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
        let mut child_err: Option<crate::error::RenderError> = None;
        let value = {
            let child_err = &mut child_err;
            let mut child_dry =
                |index: usize, c: crate::constraints::BoxConstraints| -> flui_types::Size {
                    let Some(&child_id) = children.get(index) else {
                        child_err.get_or_insert(crate::error::RenderError::contract_violation(
                            "dry-layout child query",
                            "child index out of range for this node's children",
                        ));
                        return flui_types::Size::ZERO;
                    };
                    match dry_layout_query(slots, child_id, c) {
                        Ok(v) => v,
                        Err(err) => {
                            child_err.get_or_insert(err);
                            flui_types::Size::ZERO
                        }
                    }
                };
            entry
                .render_object()
                .dry_layout_raw(constraints, children.len(), &mut child_dry)
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
/// payloads.
pub(super) fn dry_baseline_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    baseline: crate::traits::TextBaseline,
) -> crate::error::RenderResult<Option<f32>> {
    ensure_stack(|| dry_baseline_query_impl(slots, id, constraints, baseline))
}

/// Body of [`dry_baseline_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn dry_baseline_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
    baseline: crate::traits::TextBaseline,
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
                    };
                };
                match request {
                    DryBaselineChildRequest::Baseline(c, b) => {
                        match dry_baseline_query(slots, child_id, c, b) {
                            Ok(v) => DryBaselineChildResponse::Baseline(v),
                            Err(err) => {
                                child_err.get_or_insert(err);
                                DryBaselineChildResponse::Baseline(None)
                            }
                        }
                    }
                    DryBaselineChildRequest::DryLayout(c) => {
                        match dry_layout_query(slots, child_id, c) {
                            Ok(v) => DryBaselineChildResponse::DryLayout(v),
                            Err(err) => {
                                child_err.get_or_insert(err);
                                DryBaselineChildResponse::DryLayout(flui_types::Size::ZERO)
                            }
                        }
                    }
                }
            };
            entry.render_object().dry_baseline_raw(
                constraints,
                baseline,
                children.len(),
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

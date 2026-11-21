//! Post-layout servicing of lazy-sliver child managers: building requested children and evicting by the retain bands.

use super::BuildOwner;
use crate::element::child_manager::ChildManager;
use crate::tree::ElementTree;
use flui_foundation::{RenderId, SlotSet};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// Which finalize step a lazy-sliver service pass ends with — see
/// [`BuildOwner::service_child_requests_between_passes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServiceFinalize {
    /// Unmount the evicted children and verify the frame's `GlobalKey`
    /// reservations (the post-`run_frame` call).
    Frame,
    /// Unmount the evicted children only; the ledger keeps accumulating
    /// until the frame's own finalize (a fixpoint pass).
    UnmountOnly,
}

/// What a lazy-sliver service pass does with the child-build requests the
/// last layout pass recorded — see
/// [`BuildOwner::service_child_requests_evict_only`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServiceRequests {
    /// Build the requested children (and evict by the retain bands).
    Build,
    /// Evict by the retain bands only; the requests stay queued on the
    /// pipeline for the frame's post-`run_frame` service to build.
    Leave,
}

impl BuildOwner {
    /// Drive lazy-sliver child managers post-layout.
    ///
    /// Called by the layout↔build fixpoint and `WidgetsBinding::service_child_requests`
    /// immediately **after** `PipelineOwner::run_frame` and the pipeline lock
    /// is released, so the render tree is quiescent and no node of it is
    /// entered.
    ///
    /// # Ordering
    ///
    /// 1. Drain `PipelineOwner`'s `pending_child_requests` and
    ///    `pending_retain_bands` accumulated during the most recent layout pass.
    /// 2. Group by `RenderId`.
    /// 3. Clone each affected manager `Arc` out of the registry (releases the
    ///    registry lock before the service calls).
    /// 4. For each manager, call `ChildManager::service` with an inline
    ///    `ElementOwner` split-borrow — mirrors the pattern in `build_scope`'s
    ///    `catch_unwind` closure.
    /// 5. A second `build_scope` expands the newly-built children's subtrees
    ///    (the items' own views — e.g. `Padding(Text)` — need their own build
    ///    pass since `SparseChildren::ensure` only mounts the top-level node).
    /// 6. Mark each affected sliver as needing layout so the next frame
    ///    re-measures with the newly-present render nodes.
    /// 7. `finalize_tree` cleans up any evicted children pushed to inactive by
    ///    `retain_band` or `on_unmount`.
    ///
    /// # Every frame path
    ///
    /// `UiRuntime::draw_frame` reaches it through
    /// `WidgetsBinding::service_child_requests` after each `run_frame` drops
    /// the pipeline write-lock, on a platform runner and a headless host alike.
    ///
    /// Returns `true` iff a manager built or evicted a child — the sliver was
    /// then marked `needs_layout`, and the caller (the layout↔build fixpoint
    /// in `run_frame_with_layout_builders`) must run another layout pass so
    /// the change is laid out and painted in this same frame.
    pub fn service_child_requests(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
    ) -> bool {
        self.service_child_requests_impl(
            tree,
            pipeline,
            ServiceFinalize::Frame,
            ServiceRequests::Build,
        )
    }

    /// [`Self::service_child_requests`] as run between two layout passes of
    /// the same frame's fixpoint.
    ///
    /// Identical servicing, but the finalize step only unmounts the evicted
    /// children — it does **not** verify (and so does not clear) the
    /// `GlobalKey` reservation ledger. Verification is a per-*frame* verdict
    /// (ADR-0050): two lazy slivers that both declare one key in the same
    /// frame must be seen by one verification, and the fixpoint may service
    /// them in different passes. The frame's post-`run_frame` call keeps
    /// the full finalize.
    pub(crate) fn service_child_requests_between_passes(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
    ) -> bool {
        self.service_child_requests_impl(
            tree,
            pipeline,
            ServiceFinalize::UnmountOnly,
            ServiceRequests::Build,
        )
    }

    /// Apply the retain bands the last layout pass recorded and leave its
    /// build requests queued: the evict-before-paint step of a frame whose
    /// lazy band did not settle within its pass budget.
    ///
    /// The fixpoint stops servicing when the budget trips, so the residents
    /// the last pass's band no longer covers would otherwise stay attached
    /// through the frame's final layout and paint — positioned nowhere by
    /// that layout (the walk positions in-band children only) yet painted,
    /// hit-tested, and assembled into semantics at whatever offset they
    /// last had. Evicting them here, and marking their sliver for the
    /// final layout, means nothing outside the committed band exists when
    /// the frame paints. The requests are not touched: they stay queued
    /// for the post-frame service, which builds them for the next frame —
    /// the deferral the budget asked for, without the stale residents.
    /// (Taking them here would cost that band a frame: a sliver the
    /// eviction left clean is not laid out again before the frame ends,
    /// so nothing would re-record them.)
    pub(crate) fn service_child_requests_evict_only(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
    ) -> bool {
        self.service_child_requests_impl(
            tree,
            pipeline,
            ServiceFinalize::UnmountOnly,
            ServiceRequests::Leave,
        )
    }

    fn service_child_requests_impl(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &flui_rendering::pipeline::PipelineCell,
        finalize: ServiceFinalize,
        requests: ServiceRequests,
    ) -> bool {
        let finalize = |owner: &mut Self, tree: &mut ElementTree| match finalize {
            ServiceFinalize::Frame => owner.finalize_tree(tree),
            ServiceFinalize::UnmountOnly => owner.unmount_inactive_elements(tree),
        };
        // 1. Drain pending buffers from the pipeline (a brief checkout).
        let (pending_requests, retain_bands) = pipeline.with_mut(|guard| {
            let requests = match requests {
                ServiceRequests::Build => guard.take_pending_child_requests(),
                ServiceRequests::Leave => Vec::new(),
            };
            let bands = guard.take_pending_retain_bands();
            (requests, bands)
        });

        // Finalize BEFORE the early-return: `build_scope` does not call
        // `finalize_tree`, so sparse children pushed to `inactive_elements` by
        // `SliverAdaptorBehavior::on_unmount` (F3) — during a reconcile that
        // removed their host — must be cleaned up here. Without this, those
        // elements and their render nodes are leaked until the next
        // `service_child_requests` call that has pending layout requests.
        if !self.inactive_elements.is_empty() {
            finalize(self, tree);
        }

        if pending_requests.is_empty() && retain_bands.is_empty() {
            return false;
        }

        tracing::debug!(
            requests = pending_requests.len(),
            bands = retain_bands.len(),
            "service_child_requests: draining lazy-sliver pending buffers"
        );

        // 2. Group requests and retain-bands by sliver RenderId.
        let mut requests_by_sliver: FxHashMap<RenderId, Vec<usize>> = FxHashMap::default();
        for (sliver_id, logical_index) in pending_requests {
            requests_by_sliver
                .entry(sliver_id)
                .or_default()
                .push(logical_index);
        }
        let mut bands_by_sliver: FxHashMap<RenderId, (usize, usize)> = FxHashMap::default();
        for (sliver_id, first, last) in retain_bands {
            bands_by_sliver.insert(sliver_id, (first, last));
        }

        // Collect all affected sliver ids (may have requests, bands, or both).
        let affected_ids: Vec<RenderId> = {
            let mut ids: SlotSet<RenderId> = SlotSet::new();
            for &id in requests_by_sliver.keys().chain(bands_by_sliver.keys()) {
                ids.insert(id);
            }
            ids.as_slice().to_vec()
        };

        // 3. Clone manager Rcs out of the registry before any service call.
        //    This releases the registry borrow so that service calls that call
        //    `register_child_manager` / `unregister_child_manager` through an
        //    `ElementOwner` can re-enter the registry without re-borrowing.
        let manager_rcs: Vec<(RenderId, Rc<RefCell<dyn ChildManager>>)> = {
            let registry = self.child_manager_registry.borrow();
            affected_ids
                .iter()
                .filter_map(|&id| registry.get(&id).map(|m| (id, Rc::clone(m))))
                .collect()
        };

        if manager_rcs.is_empty() {
            tracing::debug!("service_child_requests: no registered managers for affected slivers");
            return false;
        }

        // 4. Call service on each manager. Each iteration builds an inline
        //    ElementOwner split-borrow, calls service (which may call
        //    `ensure`/`evict` and mutate the tree + dirty heap), then drops
        //    the inline owner — the borrow ends before the next iteration.
        //
        //    Track whether any manager did real work (built or evicted a child).
        //    When all managers return `false` (settled state — band unchanged,
        //    no new requests in-band) we skip `mark_needs_layout` so the sliver
        //    stays clean and the frame quiesces rather than looping forever.
        let mut any_service_did_work = false;
        for (sliver_id, manager_rc) in &manager_rcs {
            let requested = requests_by_sliver
                .get(sliver_id)
                .map_or(&[][..], Vec::as_slice);
            let (retain_first, retain_last) = bands_by_sliver
                .get(sliver_id)
                .copied()
                .unwrap_or((0, usize::MAX));

            // Inline split-borrow (same pattern as `build_scope` catch_unwind).
            let partitioned_dirty_count = self.partitioned_dirty_count();
            let mut inline_owner = super::super::ElementOwner {
                global_keys: &mut self.global_keys,
                global_key_reservations: &mut self.global_key_reservations,
                dirty_elements: &mut self.dirty_elements,
                partitioned_dirty_count,
                dirty_reasons: &mut self.dirty_reasons,
                inactive_elements: &mut self.inactive_elements,
                pending_dependency_changes: &mut self.pending_dependency_changes,
                inherited_dependencies: &mut self.inherited_dependencies,
                keep_alive: self.keep_alive.clone(),
                unmount_hooks: self.unmount_hooks.clone(),
                frame_wake: super::super::element_owner::FrameWake::AfterPass,
                external_inbox: &self.external_inbox,
                external_request_frame: self.on_build_scheduled.as_ref(),
                build_view: None,
                child_manager_registry: &self.child_manager_registry,
                layout_builder_registry: &self.layout_builder_registry,
                focus_manager: &self.focus_manager,
                lifecycle_handle: &self.lifecycle_handle,
                window_handle: &self.window_handle,
                async_driver: &self.async_driver,
                post_frame_handle: &self.post_frame_handle,
                capabilities: &self.capabilities,
                flush_registry: &self.flush_registry,
                interaction_dispatch: &self.interaction_dispatch,
                collects_semantics: self.collects_semantics,
                hit_test_handle: &self.hit_test_handle,
                global_key_scope: &mut self.global_key_scope,
                owner_tag: self.owner_tag,
                tree_observer: &mut self.tree_observer,
                recovered_panics: &mut self.recovered_panics,
                build_recovered: None,
                reactive: &self.reactive,
                lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
            };

            let did_work = manager_rc.borrow_mut().service(
                requested,
                retain_first,
                retain_last,
                tree,
                &mut inline_owner,
                pipeline,
            );
            if did_work {
                any_service_did_work = true;
            }
        } // `inline_owner` drops here — all `&mut` borrows released.

        // 5. Second build_scope: expand newly-built children's subtrees.
        //    `SparseChildren::ensure` mounts the top-level lazy-child node and
        //    pushes it onto the dirty heap (F1), but the child's own sub-views
        //    (e.g. a Padding wrapping a Text) need a dedicated build pass.
        //    `build_scope_impl`, not `build_scope`: this can run more than
        //    once per frame (once per fixpoint pass), and must NOT reset the
        //    mid-drain absorb budget each time — that reset is exactly once
        //    per frame, in `build_scope` itself.
        self.build_scope_impl(tree);

        // 6. Mark each serviced sliver as needing re-layout ONLY if service
        //    did real work (built or evicted children). When all service calls
        //    returned `false`, the list has settled: the viewport, band, and
        //    built-child set are all unchanged. Skipping `mark_needs_layout`
        //    keeps the sliver clean so `flush_layout` skips it, `perform_layout`
        //    does not run, no new `emit_retain_band` fires, and
        //    `service_child_requests` early-returns on the next frame.
        //    This breaks the unconditional dirty-loop that prevented quiescence.
        if any_service_did_work {
            pipeline.with_mut(|guard| {
                for (sliver_id, _) in &manager_rcs {
                    guard.mark_needs_layout(*sliver_id);
                }
            });
        } else {
            tracing::debug!(
                "service_child_requests: no work done, skipping mark_needs_layout \
                 (frame quiesces)"
            );
        }

        // 7. Finalize: unmount evicted children (pushed to inactive by
        //    `retain_band` → `evict` → `tree.remove_subtree`) and the lazy
        //    children pushed by `on_unmount` (F3).
        finalize(self, tree);
        any_service_did_work
    }
}

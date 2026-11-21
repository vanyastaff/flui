//! The build pass: draining the dirty queue in depth order, absorbing the out-of-frame inbox mid-drain, and replacing an element whose lifecycle hook failed.

use super::super::dirty_queue::DirtyElement;
use super::super::external_build_inbox::{ExternalBuildInbox, PendingBuilds};
use super::{BuildOwner, BuildScopeTarget, raise};
use crate::owner::StagedRecoveredPanic;
use crate::tree::ElementTree;
use crate::view::ChildViews;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{ElementId, SlotSet};
use std::sync::Arc;

/// Upper bound on RE-ENTRY mid-drain absorbs per frame (issue #1180).
///
/// A "re-entry" is a `Vacant` landing for an id that already completed a
/// build in this `build_scope` call — most often that same element
/// rescheduling itself from inside its own `build`, but identically a child
/// notifying its already-built parent, or one half of an A↔B ping-pong. The
/// FIRST time any id lands in a frame is always free, however many
/// independent elements do it (a page whose N unrelated parents each
/// retarget an implicit animation in one frame performs N first-time
/// absorbs, all legitimate) — see [`BuildOwner::built_this_frame`]. Only an
/// id that keeps getting notified again after each of its own builds spends
/// this budget, so 16 is already a pathological tree, not a realistic one:
/// a downward notification (parent notifies a descendant below it) costs
/// nothing beyond its own first-time absorb.
pub(super) const MAX_MID_DRAIN_ABSORBS: usize = 16;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct BuildDrainResult {
    pub(crate) any_rebuilt: bool,
    pub(crate) target_rebuilt: bool,
}

/// Drain-scoped accumulator for ids [`BuildOwner::absorb_mid_drain_inbox`]
/// capped this drain (see that method's doc for why this exists instead of
/// putting each one straight back into the shared inbox). `Drop` — not a
/// flush call placed after `drain_build_scope`'s loop — is what actually
/// gets the accumulated ids back to `external_inbox`, because the loop's own
/// per-element build and reconcile panics are caught, patched up, and
/// RE-RAISED via `std::panic::resume_unwind` from several arms inside that
/// loop: each one unwinds `drain_build_scope`'s stack frame immediately,
/// skipping any code placed after the loop. Without this guard, an id
/// already marked dirty on the tree and removed from `external_inbox` (by
/// `absorb_mid_drain_inbox`'s `take_into`) but capped by the budget would
/// end up in none of `dirty_elements`, `dirty_reasons`, or `external_inbox`
/// the moment a LATER build in the same drain panics — a silently lost
/// rebuild request. A `Drop` impl runs on every exit path, unwind included,
/// so this is the house pattern for cleanup that must survive a panicking
/// callback (see the reentrancy-depth guard around ticker callbacks for the
/// same shape). Holds a cloned `Arc`, not a borrow of `BuildOwner`, so it can
/// coexist with `&mut self` calls elsewhere in the loop.
pub(super) struct CappedLeftoverGuard {
    inbox: Arc<ExternalBuildInbox>,
    leftover: PendingBuilds,
}

impl CappedLeftoverGuard {
    fn new(inbox: Arc<ExternalBuildInbox>) -> Self {
        Self {
            inbox,
            leftover: PendingBuilds::default(),
        }
    }
}

impl Drop for CappedLeftoverGuard {
    fn drop(&mut self) {
        if self.leftover.is_empty() {
            return;
        }
        let mut capped = Vec::new();
        self.leftover.take_into(&mut capped);
        let mut inbox = self.inbox.lock();
        for (id, reasons) in capped {
            inbox.merge(id, reasons);
        }
    }
}

impl BuildOwner {
    /// Process all dirty elements outside layout-builder scopes.
    ///
    /// Rebuilds elements in depth order (shallowest first). This ensures
    /// that when a parent rebuilds, any children that become dirty are
    /// processed after the parent.
    ///
    /// # Arguments
    ///
    /// * `tree` - The element tree to rebuild
    pub fn build_scope(&mut self, tree: &mut ElementTree) {
        // Per-frame mid-drain absorb accounting: re-arm the exhaustion streak
        // against how the PREVIOUS frame ended — BEFORE this frame's own
        // reset overwrites `mid_drain_absorbs_left` — then reset the budget
        // and the re-entry set for this frame. There is no frame-complete
        // hook on `BuildOwner`, so this retroactive check at the NEXT entry
        // is the only place "did the last frame end clean" can be answered.
        //
        // This reset must run exactly once per real frame, which is why it
        // lives here rather than in `build_scope_impl`: `build_scope` is the
        // frame-boundary entry point every production/headless binding calls
        // once per pump, but `service_child_requests_impl`'s lazy-sliver
        // service pass also needs to drain newly-built children's subtrees
        // — possibly several times in the SAME frame, once per fixpoint pass
        // — and calls `build_scope_impl` directly for that, so the budget it
        // shares stays a genuine per-frame bound instead of resetting on
        // every service pass.
        if self.mid_drain_absorbs_left > 0 {
            self.mid_drain_cap_streak = false;
        }
        self.mid_drain_absorbs_left = MAX_MID_DRAIN_ABSORBS;
        self.clear_frame_build_report();

        self.build_scope_impl(tree);
    }

    /// The reusable core of [`Self::build_scope`]: the build span, the
    /// early-return when a mounted-but-clean layout builder has no work,
    /// drain-target selection, the drain itself, and collapsing the scoped
    /// queues when they empty out. Deliberately excludes the per-frame
    /// mid-drain accounting reset that only [`Self::build_scope`] performs —
    /// see that method's doc for why a mid-frame re-entrant caller (the
    /// lazy-sliver service pass) must reach this directly instead.
    pub(super) fn build_scope_impl(&mut self, tree: &mut ElementTree) {
        // The build phase's span, matching the `layout`/`paint`/`compositing`
        // spans the pipeline already emits. Together the four are what a
        // profiler subscribes to: `flui-devtools` is layer 9 and nothing in
        // the framework may consume it, so tracing is the only seam by which
        // frame timings can reach it. `absorbed_mid_drain` is recorded by
        // `drain_build_scope` once the drain completes — declared empty here
        // so the field exists on the span from the start.
        let _span = tracing::debug_span!(
            target: "flui.frame",
            "build",
            dirty_elements = self.dirty_count(),
            absorbed_mid_drain = tracing::field::Empty,
        )
        .entered();

        let has_partitioned_work = self
            .build_scope_queues
            .as_ref()
            .is_some_and(|queues| !queues.root.is_empty() || !queues.isolated.is_empty());
        if !self.layout_builder_registry.is_empty()
            && !has_partitioned_work
            && self.dirty_elements.is_empty()
            && self.external_inbox.lock().is_empty()
        {
            // A mounted but clean layout builder must not turn the lazy scoped
            // queues into a per-frame allocation. There is no work to route.
            return;
        }
        let target = if self.layout_builder_registry.is_empty() && !has_partitioned_work {
            // Preserve the pre-scope hot path exactly when the feature is inert.
            BuildScopeTarget::All
        } else {
            BuildScopeTarget::Global
        };
        self.build_scope_target(tree, target);
        self.collapse_empty_build_scope_queues();
    }

    pub(crate) fn build_scope_target(&mut self, tree: &mut ElementTree, target: BuildScopeTarget) {
        let live_scopes = match target {
            BuildScopeTarget::All => None,
            BuildScopeTarget::Global | BuildScopeTarget::LayoutBuilder(_) => {
                let live_scopes = self.live_layout_scope_ids(tree);
                self.repartition_dirty(tree, &live_scopes);
                self.activate_build_target(target);
                Some(live_scopes)
            }
        };
        let _ = self.drain_prepared_build_target(tree, target, live_scopes.as_ref());
    }

    /// Finish a parented lifecycle recovery once the panicking element is back in its slab slot.
    ///
    /// The configurable view factory is the last fallible step before the
    /// destructive replacement transaction begins. A factory unwind retains
    /// the original element and its dirty entry; the owned staged diagnostic
    /// is then dropped without touching earlier committed records. Once
    /// `replace_child_with` starts, its fatal no-rollback contract applies.
    #[cold]
    fn replace_failed_lifecycle_element(
        &mut self,
        tree: &mut ElementTree,
        failed: ElementId,
        replacement_location: (ElementId, usize),
        dirty: DirtyElement,
        mut staged: StagedRecoveredPanic,
        caught: FirstPanic,
    ) {
        let (parent, slot) = replacement_location;
        // The failed element is replaced below, after which the drain could not find it: read its
        // path while it is a node.
        staged.locate_in(tree);
        let error = crate::view::FrameworkError::from_panic(
            caught
                .held()
                .expect("BUG: a failed lifecycle element's panic is held"),
            "running a lifecycle hook during rebuild",
        );
        // Classified, the hook's payload is destroyed under containment
        // before the error-view factory, which is user code, runs.
        drop(caught);
        // A factory panic must not escape recovery half-done: the element
        // keeps its dirty entry and the factory's panic is raised.
        let mut factory = FirstPanic::new();
        let Some(recovery_view) = factory.contain(|| {
            crate::view::recovery_view_for(&error, self.capabilities.error_view_builder)
        }) else {
            self.dirty_elements.requeue(dirty);
            raise(factory)
        };

        self.dirty_reasons.remove(&failed);
        self.pending_dependency_changes.remove(failed);
        let mut element_owner = self.element_owner_mut();
        let substitute = crate::element::sparse_children::replace_failed_child(
            parent,
            slot,
            failed,
            &recovery_view,
            tree,
            &mut element_owner,
        );
        staged.substituted_by(substitute, parent, slot);
        element_owner.commit_staged_lifecycle_panic(staged);
    }

    pub(crate) fn drain_prepared_build_target(
        &mut self,
        tree: &mut ElementTree,
        target: BuildScopeTarget,
        live_scopes: Option<&SlotSet<ElementId>>,
    ) -> BuildDrainResult {
        #[cfg(not(debug_assertions))]
        if matches!(target, BuildScopeTarget::All) {
            let result = self.drain_build_scope(tree, target, live_scopes);
            self.request_owed_frame();
            return result;
        }

        #[cfg(debug_assertions)]
        {
            assert!(!self.building, "build_scope called while already building");
            self.building = true;
            self.scope_depth += 1;
        }

        // The scoped drain restores build state before a panic propagates.
        let mut caught = FirstPanic::new();
        let outcome = caught.contain(|| self.drain_build_scope(tree, target, live_scopes));

        if outcome.is_none() && !matches!(target, BuildScopeTarget::All) {
            let live_scopes = live_scopes.expect("BUG: scoped drain owns a live-scope snapshot");
            self.repartition_dirty(tree, live_scopes);
        }

        #[cfg(debug_assertions)]
        {
            self.building = false;
            self.scope_depth -= 1;
        }

        match outcome {
            Some(result) => {
                self.request_owed_frame();
                result
            }
            // The pass's own failure comes first; the owed wake stays
            // pending for the next request to retry.
            None => raise(caught),
        }
    }

    /// Deliver the frame a build pass owes for the entries it scheduled
    /// (see [`FrameWake::AfterPass`](super::super::element_owner::FrameWake)), once
    /// every element is back in the tree. A failing wake reaches the caller
    /// and stays owed, so the next request retries it.
    fn request_owed_frame(&self) {
        self.external_inbox
            .request_frame_if_needed(false, self.on_build_scheduled.as_deref());
    }

    fn drain_build_scope(
        &mut self,
        tree: &mut ElementTree,
        target: BuildScopeTarget,
        live_scopes: Option<&SlotSet<ElementId>>,
    ) -> BuildDrainResult {
        let mut result = BuildDrainResult::default();
        let mut absorbed_mid_drain = 0usize;
        // Ids capped for the rest of THIS drain accumulate here instead of
        // being put straight back into `self.external_inbox` — see
        // `CappedLeftoverGuard`'s doc for why a guard, not a flush placed
        // after the loop, and `Self::absorb_mid_drain_inbox`'s doc for the
        // budget rules.
        let mut capped_leftover = CappedLeftoverGuard::new(Arc::clone(&self.external_inbox));

        // Re-key every element already on the heap to its current tree depth
        // before draining. `schedule_build_for` trusts the depth its caller
        // passes; a stale one (a subtree moved by a GlobalKey after the
        // schedule) would let a deeply-nested rebuild sort as if it were
        // shallow, so a child could build before its parent and violate
        // the shallowest-first build contract, which keys on the element's
        // real depth. Re-derive each id's depth from its node — the same authority
        // `absorb_mid_drain_inbox` uses for a freshly-absorbed id.
        self.rekey_dirty_depths(tree);

        // Process dirty elements in depth order, extract-then-apply
        // (E3 — atomic box→arena swap).
        //
        // The hard problem: the old loop held `&mut tree.get_mut(id).element`
        // while calling `perform_build`, so `perform_build` could not also
        // take `&mut ElementTree` to reconcile slab-resident children —
        // the exact double-borrow that cost the render-tree PRs. The fix
        // is the same extract-then-apply discipline E2.5 proves out, lifted
        // to the build seam:
        //
        //   1. Take a `&mut element` borrowed FROM the tree, run the
        //      behavior's build half (`build_into_views`), capture the
        //      OWNED child views, and DROP the element borrow.
        //   2. With a FRESH `&mut tree` borrow, feed those views to the
        //      id-reconciler, which inserts / updates / removes the
        //      slab-resident child nodes.
        //
        // No `&mut` into the slab is ever live across a second slab access.
        //
        // Absorb the out-of-frame inbox BEFORE every pop, not only once
        // before the first one (issue #1180): a build below can call another
        // element's `RebuildHandle::schedule` synchronously (the inbox is the
        // only route a cross-thread or listener-driven rebuild has), and that
        // schedule must join THIS drain rather than sit until the next
        // `build_scope`; the heap is re-sorted every iteration. See
        // `Self::absorb_mid_drain_inbox` for the Occupied/Vacant/budget rules.
        //
        // Each iteration pops one entry first so `pop()`'s mutation of
        // `self.dirty_elements` (a field the split-borrow handle aliases)
        // is released before the handle is reborrowed.
        //
        // `absorbed_mid_drain` only counts from the SECOND iteration onward:
        // the very first absorb of this call is ordinary intake — whatever
        // landed between the previous drain (or frame) and this one, before
        // a single element here has built — not something that arrived
        // "mid-drain" in the sense a profiler cares about. Only an absorb
        // that runs after this call has already popped and built at least
        // one entry observed something land while THIS drain was actively
        // in progress, which is what the field name promises.
        let mut first_pop = true;
        // One buffer of dependency records serves every element this drain
        // builds: each build fills it, the drain below empties it.
        let mut dependent_records: Vec<crate::context::DependentRecord> = Vec::new();
        // The providers a dependent read in its previous build, kept across
        // builds and inline for eight, so the drain copies them without
        // allocating.
        let mut previous_providers: smallvec::SmallVec<[ElementId; 8]> = smallvec::SmallVec::new();
        loop {
            let absorbed_this_pop =
                self.absorb_mid_drain_inbox(tree, &mut capped_leftover.leftover);
            if !first_pop {
                absorbed_mid_drain += absorbed_this_pop;
            }
            first_pop = false;
            let Some(dirty) = self.dirty_elements.pop() else {
                break;
            };
            let id = dirty.id();
            let nearest_scope = match target {
                BuildScopeTarget::All => None,
                BuildScopeTarget::Global | BuildScopeTarget::LayoutBuilder(_) => {
                    Self::nearest_layout_builder_scope(
                        tree,
                        id,
                        live_scopes.expect("BUG: scoped drain owns a live-scope snapshot"),
                    )
                }
            };
            let target_accepts = match target {
                BuildScopeTarget::All => true,
                BuildScopeTarget::Global => nearest_scope.is_none(),
                BuildScopeTarget::LayoutBuilder(scope) => nearest_scope == Some(scope),
            };
            if !target_accepts {
                self.defer_dirty_element(nearest_scope, dirty);
                continue;
            }

            let reasons_at_start = self
                .dirty_reasons
                .get(&id)
                .copied()
                .expect("BUG: every dirty heap entry must own rebuild reasons");

            // If this dependent received an inherited-dependency change since its
            // last build, fire `ViewState::did_change_dependencies` BEFORE
            // the build. Consumed here so the typed hook runs exactly once
            // per dependency-change-then-rebuild cycle.
            let needs_did_change = self.pending_dependency_changes.contains(id);

            // Guard (still holding only a brief `&mut node`): an
            // inherited-dependency change marks an otherwise-clean dependent
            // dirty so its build re-runs against the new value; then skip
            // unless the element is both buildable (lifecycle) AND dirty. A
            // clean element's build half returns an empty view list, and the
            // phase-2 reconcile would then wrongly REMOVE all its children —
            // so a clean entry must never reach reconcile.
            {
                let Some(node) = tree.get_mut(id) else {
                    // Stale / removed id — nothing to build.
                    self.dirty_reasons.remove(&id);
                    self.pending_dependency_changes.remove(id);
                    continue;
                };
                if needs_did_change {
                    node.element_mut().mark_needs_build();
                }
                if !node.element().lifecycle().can_build() || !node.element().is_dirty() {
                    self.dirty_reasons.remove(&id);
                    self.pending_dependency_changes.remove(id);
                    continue;
                }
            }

            // This element is about to re-state, from scratch, which keyed
            // children it declares — so everything the frame recorded about
            // it up to now is superseded. Clearing here rather than after
            // the reconcile is what makes the newest build authoritative:
            // the reconcile below immediately re-reserves whatever this
            // build still declares, and anything it has dropped simply does
            // not come back.
            self.global_key_reservations.note_parent_rebuild(id);

            // A per-node span: TRACE, primitives only (the element's packed id, the reasons as a
            // bitmask over the `RebuildReason` variants in declaration order).
            let rebuild_span = tracing::trace_span!(
                target: "flui.tree",
                "element_rebuild",
                element = id.as_u64(),
                reasons = u64::from(reasons_at_start.bits()),
            );
            let _rebuild_entered = rebuild_span.enter();

            // ── Phase 1: extract BY VALUE, build against a LIVE read view.
            // Taking the element out of its slot frees the tree for a shared
            // `&` borrow, so the element's `build()` can resolve InheritedView
            // / ancestor lookups against the REAL tree (via the `LiveBuildContext`
            // the behaviour builds from `ElementOwner::build_view`) — no empty
            // dummy: the build reads a shared reborrow of the tree the frame
            // driver already holds, never a second borrow of its cell.
            // Inherited dependents are buffered (the tree is read-only here)
            // and applied below once `&mut tree` is free again.
            let Some(mut element) = tree.take_element(id) else {
                continue;
            };
            let replacement_location = tree
                .get(id)
                .and_then(|node| node.parent().map(|parent| (parent, node.slot())));
            let dep_sink: std::cell::RefCell<Vec<crate::context::DependentRecord>> =
                std::cell::RefCell::new(std::mem::take(&mut dependent_records));

            // Run the build half under `catch_unwind` so the extracted element
            // is ALWAYS restored to its slot before this seam decides whether
            // to substitute or propagate. User `build()` is already recovered
            // one level down. The lifecycle calls reachable here (stateful hooks,
            // `update_render_object`) catch only their literal user-hook expressions,
            // stage only when this parented seam arms them, and rethrow into this
            // guard. Any other unwind remains unrecorded and propagates after restoration.
            // `AssertUnwindSafe` is sound because the sole cross-unwind
            // invariant — the slot is whole again — is re-established by the
            // unconditional `put_element` below.
            // Set by `build_or_recover` if this element's build panics and is
            // recovered (reset-on-build then keeps its masks).
            let build_recovered = std::cell::Cell::new(false);
            let partitioned_dirty_count = self.partitioned_dirty_count();
            let mut caught = FirstPanic::new();
            let build_outcome = caught.contain(|| {
                let mut element_owner = super::super::ElementOwner {
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
                    build_view: Some(super::super::BuildHandle {
                        tree: &*tree,
                        dep_sink: &dep_sink,
                    }),
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
                    build_recovered: Some(&build_recovered),
                    reactive: &self.reactive,
                    lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
                };
                if needs_did_change {
                    if replacement_location.is_some() {
                        element_owner.arm_lifecycle_panic_handoff();
                    }
                    element
                        .element_mut()
                        .notify_dependency_change(&mut element_owner);
                    if replacement_location.is_some() {
                        assert!(
                            element_owner.take_staged_lifecycle_panic().is_none(),
                            "a successful dependency hook must leave its own window unrecorded"
                        );
                    }
                }
                if replacement_location.is_some() {
                    element_owner.arm_lifecycle_panic_handoff();
                }
                let views = element.element_mut().build_into_views(&mut element_owner);
                if replacement_location.is_some() {
                    assert!(
                        element_owner.take_staged_lifecycle_panic().is_none(),
                        "a successful initial-state/build call must leave its own window unrecorded"
                    );
                }
                views
            }); // `element_owner` + its `&*tree` borrow drop here.

            // Restore the element BEFORE anything else — the slot must be whole
            // whether the build returned or unwound. With `&mut tree` free
            // again we then apply the dependents buffered during the read-only
            // build onto their provider nodes; recording in the SAME iteration
            // (before the next dirty pop) preserves the record-before-notify
            // ordering.
            tree.put_element(id, element);

            let Some(new_views): Option<ChildViews> = build_outcome else {
                let staged = self.lifecycle_panic_handoff.take().into_staged();
                let Some((parent, slot)) = replacement_location else {
                    self.dirty_elements.requeue(dirty);
                    raise(caught)
                };
                let Some(staged) = staged else {
                    self.dirty_elements.requeue(dirty);
                    raise(caught)
                };

                self.replace_failed_lifecycle_element(
                    tree,
                    id,
                    (parent, slot),
                    dirty,
                    staged,
                    caught,
                );
                result.any_rebuilt = true;

                if !matches!(target, BuildScopeTarget::All) {
                    let fresh_live_scopes = self.live_layout_scope_ids(tree);
                    self.repartition_dirty(tree, &fresh_live_scopes);
                    self.activate_build_target(target);
                }
                continue;
            };
            debug_assert!(
                self.lifecycle_panic_handoff.take().is_disarmed(),
                "successful lifecycle windows must disarm their handoff immediately"
            );

            let reasons = self
                .dirty_reasons
                .remove(&id)
                .expect("BUG: a completed rebuild must retain its rebuild reasons");
            result.any_rebuilt = true;
            result.target_rebuilt |=
                matches!(target, BuildScopeTarget::LayoutBuilder(scope) if scope == id);
            self.pending_dependency_changes.remove(id);
            // A LATER mid-drain absorb of this same id is a re-entry, not a
            // first-time absorb, whether it comes from this element
            // rescheduling itself, a child notifying it back, or one half of
            // an A↔B ping-pong — see `Self::absorb_mid_drain_inbox`.
            self.built_this_frame.insert(id);
            self.frame_builds.builds_run += 1;
            for reason in reasons.iter() {
                self.frame_builds.record(reason);
            }

            // ADR-0040: the build ran to completion (the resume_unwind branch
            // can no longer take it) and the slot is restored — safe window
            // for third-party observer code, and a parent's rebuilt event
            // precedes the child mutations its build produced (phase 2).
            if self.tree_observer.is_some() {
                let view_type_id = tree.get(id).map(|node| node.element().view_type_id());
                if let Some(view_type_id) = view_type_id {
                    super::super::emit_observation(&mut self.tree_observer, |o| {
                        o.element_rebuilt(&flui_foundation::observe::ElementRebuilt::new(
                            id,
                            view_type_id,
                            reasons,
                        ));
                    });
                }
            }
            // Reset-on-build (ADR-0074 §5.5; dependencies do not
            // accumulate until unmount): the fields this
            // element is recorded as reading at each of its providers go back
            // to NONE, the build's own reads are re-applied from the sink, and a
            // provider it no longer read drops the entry. A field read only in
            // an earlier build therefore stops rebuilding this element.
            //
            // A build that panicked and was recovered (ErrorView substituted;
            // `build_or_recover` sets the owner's `build_recovered` flag) may
            // have stopped before reading its providers: the empty sink says nothing
            // about what the element depends on, so its previous masks are kept
            // (the sink's records are still added) and it keeps receiving the
            // notifications that let a fixed condition rebuild it.
            previous_providers.clear();
            if !build_recovered.get() {
                self.inherited_dependencies
                    .copy_providers_into(id, &mut previous_providers);
            }
            for provider in &previous_providers {
                if let Some(accessor) = tree
                    .get_mut(*provider)
                    .and_then(|node| node.element_mut().as_inherited_mut())
                {
                    accessor.reset_dependent_mask(crate::context::CrateToken::new(), id);
                }
            }
            if !build_recovered.get() {
                self.inherited_dependencies.reset_build_misses(id);
            }
            let mut recorded: Vec<crate::context::DependentRecord> = dep_sink.into_inner();
            {
                // The distinct (dependent, provider) edges the loop below
                // registers, sorted so that each dependent's are adjacent: a
                // sort of the build's reads, not a scan per read. Sixteen
                // stay inline, so an ordinary build allocates nothing here.
                let tree = &*tree;
                let mut edges: smallvec::SmallVec<[(ElementId, ElementId); 16]> = recorded
                    .iter()
                    .filter_map(|record| {
                        record.provider.map(|provider| (record.dependent, provider))
                    })
                    .filter(|&(_, provider)| {
                        tree.get(provider)
                            .is_some_and(|node| node.element().as_inherited().is_some())
                    })
                    .collect();
                edges.sort_unstable();
                edges.dedup();
                for dependent_edges in edges.chunk_by(|a, b| a.0 == b.0) {
                    self.inherited_dependencies.reserve(
                        dependent_edges[0].0,
                        dependent_edges.iter().map(|&(_, provider)| provider),
                    );
                }
            }
            for record in recorded.drain(..) {
                let Some(provider) = record.provider else {
                    self.inherited_dependencies
                        .register_miss(record.dependent, record.lifecycle);
                    continue;
                };
                let Some(node) = tree.get_mut(provider) else {
                    continue;
                };
                let Some(accessor) = node.element_mut().as_inherited_mut() else {
                    continue;
                };
                if record.lifecycle {
                    accessor.record_lifecycle_dependent(
                        crate::context::CrateToken::new(),
                        record.dependent,
                        record.depth,
                        record.mask,
                    );
                } else {
                    accessor.record_dependent(
                        crate::context::CrateToken::new(),
                        record.dependent,
                        record.depth,
                        record.mask,
                    );
                }
                self.inherited_dependencies
                    .register(record.dependent, provider);
            }
            dependent_records = recorded;
            for &provider in &previous_providers {
                let pruned = tree
                    .get_mut(provider)
                    .and_then(|node| node.element_mut().as_inherited_mut())
                    .is_some_and(|accessor| {
                        accessor.prune_unread_dependent(crate::context::CrateToken::new(), id)
                    });
                if pruned {
                    self.inherited_dependencies.unregister(id, provider);
                }
            }

            self.refresh_provided_scope(tree, id);

            // ── Phase 2: reconcile the returned views against the node's
            // slab-resident children with a fresh `&mut tree`. Newly inserted
            // children are scheduled inside the reconciler so this same drain
            // loop reaches them. The outer catch below is unwind hygiene, not
            // containment: an unbounded framework panic still propagates, but
            // the rebuilding parent regains its consumed reason and queue
            // entry before the scoped-drain guard repartitions all work.
            let partitioned_dirty_count = self.partitioned_dirty_count();
            let mut caught = FirstPanic::new();
            let reconcile_outcome = caught.contain(|| {
                let mut element_owner = super::super::ElementOwner {
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
                crate::tree::id_reconcile::reconcile_children_by_id(
                    tree,
                    id,
                    &new_views,
                    &mut element_owner,
                );
            });
            if reconcile_outcome.is_none() {
                tree.mark_needs_build(id);
                match self.dirty_reasons.entry(id) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(reasons);
                        self.dirty_elements.requeue(dirty);
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().merge(reasons);
                    }
                }
                raise(caught);
            }
        }

        // `capped_leftover`'s `Drop` flushes whatever this drain capped back into the real
        // inbox here, on this normal-return path exactly like it would on an unwind through the
        // loop above — see `CappedLeftoverGuard`'s doc.

        // The build drained: every render child has attached. Settle each render parent's
        // children into element-slot order (no-op unless an insert flagged a possible drift), so
        // a render sibling attached before a component-deferred one does not invert their layout.
        tree.reorder_render_children_after_build(
            &self.child_manager_registry,
            &mut self.frame_arena,
        );

        // Record onto whichever span is current — `build_scope`'s own "build"
        // span and `service_layout_builders`'s `during_layout` "build" span
        // both declare this field now, so a fixpoint pass's re-entries are
        // captured exactly like a top-level drain's (a span that never
        // declares a field silently drops a `record` for it, which is why
        // both call sites must).
        tracing::Span::current().record("absorbed_mid_drain", absorbed_mid_drain);

        result
    }

    /// Absorb every id currently sitting in the out-of-frame inbox onto this
    /// drain's dirty work, routed to whichever bucket `target` accepts.
    /// Returns how many ids were absorbed (merged into existing work or
    /// freshly scheduled) — never how many merely landed, since a capped
    /// re-entry is held in `capped_leftover` (the caller's drain-scoped
    /// accumulator), not absorbed.
    ///
    /// Called at the top of every [`Self::drain_build_scope`] pop, not once
    /// at drain start (issue #1180): `BuildOwner`'s own
    /// synchronous `schedule` — a build calling another element's
    /// [`RebuildHandle`](super::super::RebuildHandle) on the owner thread — joins
    /// THIS SAME drain instead of waiting for the next `build_scope` call,
    /// collapsing the double-frame the issue describes into one. There is no
    /// debug-mode assert that a mid-build
    /// schedule targets a descendant of the element currently building (see
    /// the `## Mapping decisions` entry in `crates/flui-view/ARCHITECTURE.md`
    /// for why: the inbox is the only route a cross-thread or
    /// listener-driven rebuild has, and a synchronous `schedule` call cannot
    /// tell "during my own build" from "after it").
    ///
    /// # Occupied vs Vacant
    ///
    /// At the top of a pop nothing is "being served" as a distinct state:
    /// `dirty_reasons` Occupied means the id is already on the heap or
    /// sitting in a deferred layout-builder-scope bucket, and the landed
    /// reasons simply merge into whichever bucket already holds it (a
    /// deferred id stays in its bucket). Vacant means a fresh entry is
    /// needed, keyed by the AUTHORITATIVE tree depth (never the depth
    /// captured at `schedule` time — `ElementCore` does not know its own
    /// tree depth), and pushed straight onto `self.dirty_elements` —
    /// `Self::drain_build_scope`'s own pop already re-derives this id's
    /// nearest layout-builder scope and runs `Self::defer_dirty_element` on
    /// it if `target` doesn't accept it, so classifying scope again here
    /// would only duplicate that pop-time routing, never change its
    /// outcome (a Global id landing during a `LayoutBuilder(scope)` drain
    /// still ends up deferred to the root bucket for the next Global drain,
    /// and the mirror still lands a scope-local id in `isolated[scope]`
    /// during a Global drain).
    ///
    /// # The re-entry budget
    ///
    /// A RE-ENTRY is a Vacant landing for an id that already completed a
    /// build in this `build_scope` call — [`Self::built_this_frame`] is how
    /// that is recognized. The common shape is an element rescheduling
    /// itself from inside its own build, but a child notifying its
    /// already-built parent, or one half of an A↔B ping-pong, land here
    /// identically. Every other Vacant id is a first-time absorb this frame
    /// and is free, however many independent listeners fire (each id can
    /// only be a first-time absorb once, so the free case is finite). A
    /// re-entry spends one [`Self::mid_drain_absorbs_left`]; at zero the id
    /// is held in `capped_leftover` for the rest of this drain (the existing
    /// `has_dirty_elements` gate already schedules the next `build_scope`
    /// once it is flushed back — see `Self::drain_build_scope`) and a
    /// `tracing::warn!` fires once per streak
    /// ([`Self::mid_drain_cap_streak`]), naming every id this call turned
    /// away.
    ///
    /// # Why `capped_leftover` is the caller's, not the inbox's
    ///
    /// Once the budget hits zero, every further re-entry for the rest of the
    /// drain is capped too — a self-rescheduler that keeps re-entering re-hits
    /// this every pop. Putting each one straight back into `self.external_inbox`
    /// would mean every LATER pop's `drain()` pulls the same already-capped
    /// ids back out, re-locks, and re-inserts them again, for no new
    /// information. Accumulating them in `capped_leftover` — the
    /// [`CappedLeftoverGuard`] [`Self::drain_build_scope`] owns, passed in
    /// here by `&mut` to its `leftover` field — means a capped id is looked
    /// at once, and `self.external_inbox` can go and stay empty for the
    /// remainder of the drain (the common case once budget is exhausted), so
    /// later pops hit this function's early return instead of a full
    /// lock-and-move cycle.
    ///
    /// # Locking
    ///
    /// The inbox lock is held only for the `take_into` that moves its requests
    /// into the kept `landed_builds` buffer, released before anything else runs. It is never held
    /// across a build — `parking_lot::Mutex` is non-reentrant, and a build
    /// may call `schedule` synchronously. `capped_leftover` itself is a
    /// plain, unlocked map; `CappedLeftoverGuard::drop` re-locks the inbox
    /// exactly once, when the guard goes out of scope — on the drain's
    /// normal return OR on an unwind through the loop, so a capped id
    /// survives a later build's panic in the same drain instead of vanishing
    /// with the guard's stack frame.
    fn absorb_mid_drain_inbox(
        &mut self,
        tree: &mut ElementTree,
        capped_leftover: &mut PendingBuilds,
    ) -> usize {
        // The kept buffer leaves its field for the loop; a panic below drops
        // it, and the next absorb grows a new one.
        let mut landed = std::mem::take(&mut self.landed_builds);
        {
            let mut inbox = self.external_inbox.lock();
            if inbox.is_empty() {
                self.landed_builds = landed;
                return 0;
            }
            inbox.take_into(&mut landed);
        }

        let mut absorbed = 0usize;
        let mut newly_capped_ids: Vec<ElementId> = Vec::new();
        for (id, reasons) in landed.drain(..) {
            // Mark dirty unconditionally, Occupied or Vacant — matching the
            // pre-#1180 drain's own unconditional call. A `RebuildHandle`
            // carries no reference to the element's dirty flag (a plain
            // `(inbox, ElementId)` pair), so this is the one place that both
            // knows the id and holds `&mut tree`. A node since unmounted is a
            // no-op lookup.
            tree.mark_needs_build(id);
            if let Some(existing) = self.dirty_reasons.get_mut(&id) {
                existing.merge(reasons);
                absorbed += 1;
                continue;
            }
            if self.built_this_frame.contains(id) {
                let Some(remaining) = self.mid_drain_absorbs_left.checked_sub(1) else {
                    newly_capped_ids.push(id);
                    capped_leftover.merge(id, reasons);
                    continue;
                };
                self.mid_drain_absorbs_left = remaining;
            }
            self.dirty_reasons.insert(id, reasons);
            let depth = tree.get(id).map_or(0, crate::tree::ElementNode::depth);
            // Push straight onto the heap: `drain_build_scope`'s pop already
            // re-derives this id's scope and defers non-accepted ids, so
            // classifying scope here too would only duplicate that work.
            self.dirty_elements.push(id, depth);
            absorbed += 1;
        }

        if !newly_capped_ids.is_empty() && !self.mid_drain_cap_streak {
            tracing::warn!(
                ids = ?newly_capped_ids,
                budget = MAX_MID_DRAIN_ABSORBS,
                "mid-drain absorb budget exhausted this frame; deferring the \
                 remaining re-entered id(s) to the next build_scope"
            );
            self.mid_drain_cap_streak = true;
        }

        self.landed_builds = landed;
        absorbed
    }
}

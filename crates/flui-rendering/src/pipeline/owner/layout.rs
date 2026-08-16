//! Layout phase implementation for `PipelineOwner<Layout>`.

use flui_foundation::RenderId;
use flui_types::Size;
use rustc_hash::FxHashSet;

use crate::{
    constraints::BoxConstraints,
    pipeline::{
        deferred::{DeferredMutation, DeferredRenderObject},
        phase::{Compositing, Idle, Layout},
        scheduler::PhaseKind,
    },
};

use super::{PipelineOwner, poison::LayoutFailureKind, rebind_phase, subtree_arena::SubtreeArena};

// ============================================================================
// Layout phase: run_layout + helpers
// ============================================================================

impl PipelineOwner<Layout> {
    /// Transitions a layout-phase pipeline into the [`Compositing`] phase.
    #[must_use]
    pub fn into_compositing(self) -> PipelineOwner<Compositing> {
        rebind_phase(self)
    }

    /// Returns to [`Idle`] from the layout phase (e.g. on error abort).
    #[must_use]
    pub fn into_idle(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Updates layout for all dirty render objects.
    ///
    /// This is phase 1 of the rendering pipeline. During layout:
    /// - Sizes and positions are calculated
    /// - Objects may dirty paint or compositing
    ///
    /// Nodes are sorted by depth (shallow first) so parents are laid out
    /// before their children. This matches Flutter's `flushLayout` behavior.
    ///
    /// # Synchronous Child Layout
    ///
    /// With interior mutability (RwLock on RenderNode), parent's
    /// `perform_layout` can call `layout_child()` which triggers
    /// synchronous child layout through the RenderTree. The child is laid
    /// out immediately and returns its size.
    pub fn run_layout(&mut self) -> crate::error::RenderResult<()> {
        let _span =
            tracing::debug_span!("layout", dirty_nodes = self.scheduler.layout_queue_len(),)
                .entered();

        // Process own dirty nodes if any
        // Flutter pattern: while loop to handle nodes added during layout
        while self.scheduler.has_layout_work() {
            self.scheduler.enter_phase(PhaseKind::Layout);

            // Take the dirty nodes and replace with empty set
            // This allows new nodes to be added during layout (routed
            // to mid_layout_marks; drained back at end of
            // iteration below).
            let dirty_nodes = self.scheduler.take_layout_batch_shallow_first();

            tracing::debug!(
                "run_layout: sorted order (shallow-first) = {:?}",
                dirty_nodes
                    .iter()
                    .map(|n| (n.id, n.depth))
                    .collect::<Vec<_>>()
            );

            // Process each dirty node.
            //
            // layout_dirty_root replaces the legacy
            // layout_node_with_children no-op recursion. Each dirty
            // entry is laid out via the pre-acquired-subtree walk
            // protected by LayoutCycleGuard. Constraints come from
            // cached state (post-frame-1) OR the binding-set
            // root_constraints (frame-1 root).
            for dirty_node in dirty_nodes {
                // Layout-poison backstop: a poisoned node that still
                // landed in the dirty queue (queue pushes do not lift
                // poison; only the mark_needs_layout walk does) is
                // skipped.  Clearing NEEDS_LAYOUT here too keeps paint
                // from treating the node as perpetually mid-layout.
                if self.layout_poison.is_poisoned(dirty_node.id) {
                    if let Some(node) = self.render_tree.get(dirty_node.id) {
                        node.clear_needs_layout();
                    }
                    tracing::trace!(
                        id = ?dirty_node.id,
                        "run_layout: skipping layout-poisoned dirty entry",
                    );
                    continue;
                }

                // Skip entries whose NEEDS_LAYOUT flag was already
                // cleared earlier in this iteration. Common case: a
                // parent's layout_child callback recursively lays out
                // a child whose dirty-queue entry was queued
                // separately (e.g., insert_child_render_object
                // enqueues both). Re-laying out the child would be
                // redundant + potentially side-effectful.
                let already_clean = self
                    .render_tree
                    .get(dirty_node.id)
                    .is_some_and(|n| !n.needs_layout());
                if already_clean {
                    tracing::trace!(
                        id = ?dirty_node.id,
                        "run_layout: skipping dirty-queue entry whose NEEDS_LAYOUT \
                         was already cleared this iteration",
                    );
                    continue;
                }

                let Some(constraints) = self.cached_or_root_constraints(dirty_node.id) else {
                    // Dropping the dirty entry here without recovery
                    // strands the work. The two real cases this hits:
                    //   1. Root id with root_constraints unset — the
                    //      binding should have called
                    //      set_root_constraints BEFORE run_frame.
                    //      set_root_constraints auto-marks root dirty
                    //      when constraints land, so the next
                    //      run_layout picks up the deferred work
                    //      automatically.
                    //   2. Non-root id with no cached constraints
                    //      AND no parent-driven layout yet — the
                    //      shallow-first dirty queue sort means the
                    //      parent should have processed first; if
                    //      it didn't (parent's perform_layout didn't
                    //      call layout_child for this id), the entry
                    //      is correctly dropped because the parent
                    //      is the authority on child constraints.
                    // Logged at warn so the diagnostic surfaces but
                    // doesn't halt the pipeline.
                    tracing::warn!(
                        id = ?dirty_node.id,
                        is_root = ?(self.root_id == Some(dirty_node.id)),
                        "run_layout: no cached state.constraints() AND no \
                         root_constraints (or id != root_id); skipping dirty entry. \
                         Recovery: for root → call set_root_constraints (which \
                         auto-marks the root dirty); for non-root → parent's \
                         perform_layout must call ctx.layout_child(idx, c) for \
                         this id first."
                    );
                    continue;
                };
                match self.layout_dirty_root(dirty_node.id, constraints) {
                    Ok(_) => {
                        self.layout_poison.note_success(dirty_node.id);
                    }
                    Err(e) => {
                        let kind = LayoutFailureKind::of(&e);
                        match self.layout_poison.note_failure(
                            dirty_node.id,
                            kind,
                            Some(crate::storage::ErasedConstraints::from(constraints)),
                        ) {
                            // Re-poison after a fresh invalidation lifted
                            // the poison: the caller was already signaled
                            // on the first transition, so skip the node
                            // quietly and let the frame complete with the
                            // healthy rest of the tree.
                            Some(false) => {
                                if let Some(node) = self.render_tree.get(dirty_node.id) {
                                    node.clear_needs_layout();
                                }
                                tracing::debug!(
                                    id = ?dirty_node.id,
                                    ?kind,
                                    "layout poison re-engaged after a fresh \
                                     invalidation; the node still fails layout \
                                     and stays skipped.",
                                );
                                continue;
                            }
                            // First poison transition (Some(true)) or budget
                            // not yet exhausted (None): surface the failure —
                            // a structural break (or an exhausted retry
                            // budget) is a bug the embedder must see once.
                            // This cannot storm: once poisoned, later
                            // run_layout passes skip the node via the
                            // backstop above, so the Err is a one-frame
                            // signal rather than a perpetual frame drop.
                            first @ (Some(true) | None) => {
                                if first.is_some() {
                                    if let Some(node) = self.render_tree.get(dirty_node.id) {
                                        node.clear_needs_layout();
                                    }
                                    tracing::error!(
                                        id = ?dirty_node.id,
                                        ?kind,
                                        error = ?e,
                                        "layout poison engaged: dirty-root layout \
                                         failed with a structural error (or \
                                         exhausted its retry budget); the node is \
                                         skipped in later layout passes until \
                                         freshly invalidated (mark_needs_layout).",
                                    );
                                }
                                // Drain mid-phase marks back into `dirty`
                                // even on the error path so they survive
                                // across phase invocations.
                                let _ = self.scheduler.exit_phase(PhaseKind::Layout);
                                return Err(e);
                            }
                        }
                    }
                }

                // Flutter parity (object.dart: `RenderObject.layout`
                // unconditionally ends with `markNeedsPaint()`): a
                // subtree that re-laid out must repaint, otherwise a
                // pure-layout invalidation (setState moving a child)
                // leaves stale pixels on screen.
                //
                // Flutter marks per OBJECT, and the difference is not
                // cosmetic. `mark_needs_paint` walks UP to the nearest
                // established boundary, so marking only the relayout root
                // reaches the boundary ABOVE the subtree and never the
                // boundaries INSIDE it — which also re-laid out and whose
                // content is therefore stale. A full-tree paint descent
                // hides that; a paint pass that lets a clean boundary reuse
                // its previous layers would show the stale pixels.
                self.mark_needs_paint(dirty_node.id);
                self.queue_boundaries_under(dirty_node.id);
            }

            // exit_phase clears debug_doing_layout AND drains
            // mid_layout_marks back into `dirty` so the outer while
            // condition picks up marks routed to the side queue
            // during this iteration's `debug_doing_layout = true`
            // window.
            let _ = self.scheduler.exit_phase(PhaseKind::Layout);
        }

        // Drain deferred mutations: render objects may have enqueued
        // child insertions, removals, or updates during layout. Apply
        // them now, outside the `&mut` borrow scope of the layout walk.
        // This is the Rust-native alternative to Flutter's
        // `invokeLayoutCallback`.
        //
        // True Remove → Insert → Update ordering. Previously the
        // apply loop was strict FIFO with only a remove-vs-update skip,
        // which let a frame that enqueues both Remove and Insert apply them
        // in arbitrary order. Stable-partition into three buckets and apply
        // each bucket in order. This makes the ordering comment above
        // factually true rather than aspirational.
        if !self.deferred_mutations.is_empty() {
            let mutations = self.deferred_mutations.drain();

            let (mut removes, rest): (Vec<_>, Vec<_>) = mutations
                .into_iter()
                .partition(|m| matches!(m, DeferredMutation::Remove { .. }));
            let (inserts, updates): (Vec<_>, Vec<_>) = rest
                .into_iter()
                .partition(|m| matches!(m, DeferredMutation::Insert { .. }));

            // Collect removed IDs for conflict detection (Update on same target).
            let removed_ids: FxHashSet<RenderId> = removes
                .iter()
                .filter_map(|m| match m {
                    DeferredMutation::Remove { child_id, .. } => Some(*child_id),
                    _ => None,
                })
                .collect();

            tracing::trace!(
                "run_layout: applying {} deferred mutations ({} removes, {} inserts, {} updates)",
                removes.len() + inserts.len() + updates.len(),
                removes.len(),
                inserts.len(),
                updates.len(),
            );

            let mut applied = 0usize;
            let mut skipped = 0usize;

            // Phase 1: Removes.
            for mutation in removes.drain(..) {
                self.apply_deferred_mutation(mutation);
                applied += 1;
            }

            // Phase 2: Inserts.
            for mutation in inserts {
                self.apply_deferred_mutation(mutation);
                applied += 1;
            }

            // Phase 3: Updates — skip any whose target was removed this batch.
            for mutation in updates {
                match &mutation {
                    DeferredMutation::Update { target_id, .. }
                        if removed_ids.contains(target_id) =>
                    {
                        tracing::warn!(
                            ?target_id,
                            "apply_deferred_mutation: skipping update — target was \
                             removed in the same layout pass"
                        );
                        skipped += 1;
                    }
                    _ => {
                        self.apply_deferred_mutation(mutation);
                        applied += 1;
                    }
                }
            }

            if skipped > 0 {
                tracing::warn!(
                    "run_layout: {skipped} deferred mutations skipped due to \
                     remove-update conflicts ({applied} applied)"
                );
            }
        }

        Ok(())
    }

    /// Applies a single deferred mutation.
    ///
    /// Called after the layout pass completes. The mutation queue was
    /// populated during layout by render objects that needed to modify
    /// the tree structure (e.g., `LayoutBuilder`, `OverlayPortal`).
    fn apply_deferred_mutation(&mut self, mutation: DeferredMutation) {
        match mutation {
            DeferredMutation::Insert {
                parent_id,
                render_object,
                index,
                logical_index,
                initial_parent_data,
            } => {
                // A deferred insert must schedule the new child and apply the
                // canonical membership impact to its parent. Without this the fresh node
                // carries NEEDS_LAYOUT but is absent from every dirty queue,
                // so it is laid out never and painted never (invisible child).
                let Some(parent_depth) = self.render_tree.depth(parent_id) else {
                    tracing::warn!(
                        ?parent_id,
                        "apply_deferred_mutation: Insert parent does not exist; mutation dropped"
                    );
                    return;
                };
                let child_depth = (parent_depth + 1) as usize;
                let inserted = match render_object {
                    DeferredRenderObject::Box(obj) => {
                        self.render_tree.insert_box_child(parent_id, obj)
                    }
                    DeferredRenderObject::Sliver(obj) => {
                        self.render_tree.insert_sliver_child(parent_id, obj)
                    }
                };
                let Some(child_id) = inserted else { return };

                // `insert_*_child` appends; honor an explicit position by
                // moving the freshly appended child into place. Clamp so an
                // out-of-range index lands at the end rather than panicking.
                if let Some(i) = index
                    && let Some(parent) = self.render_tree.get_mut(parent_id)
                {
                    parent.remove_child(child_id);
                    let clamped = i.min(parent.child_count());
                    parent.insert_child(clamped, child_id);
                }

                // Part of the lazy-sliver re-entrant build contract: install the
                // logical index on the fresh child's parent-data so the lazy
                // sliver consumer can reconcile it on the next pass.
                //
                // Fresh `RenderNode`s start with `parent_data = None`; the
                // build backend seeds `initial_parent_data` with a pre-built
                // `SliverMultiBoxAdaptorParentData { index: logical_index }`
                // for exactly this case.  If parent-data is already present
                // (e.g. a re-inserted node whose data survived) we stamp the
                // index field directly instead so we never overwrite unrelated
                // fields the existing box carries.
                if (logical_index.is_some() || initial_parent_data.is_some())
                    && let Some(child_node) = self.render_tree.get_mut(child_id)
                {
                    match child_node.parent_data_mut() {
                        None => {
                            // Node has no parent-data yet: install the
                            // pre-built box wholesale.
                            if let Some(pd) = initial_parent_data {
                                child_node.set_parent_data(pd);
                                tracing::trace!(
                                    ?child_id,
                                    logical_index,
                                    "apply_deferred_mutation: installed \
                                     initial parent-data on fresh child",
                                );
                            }
                        }
                        Some(pd) => {
                            // Parent-data already present: stamp only the
                            // logical-index field so other fields are
                            // preserved.
                            if let Some(li) = logical_index
                                && let Some(lip) = pd.as_logical_index_mut()
                            {
                                lip.set_logical_index(li);
                                tracing::trace!(
                                    ?child_id,
                                    logical_index = li,
                                    "apply_deferred_mutation: stamped \
                                     logical index into existing parent-data",
                                );
                            }
                        }
                    }
                }

                self.bootstrap_repaint_boundary_flag(child_id);
                // ADR-0013: hand the freshly-inserted child its self-dirty
                // handle. `attach_inserted_node` is protocol-generic (keys
                // off `RenderId`, not the protocol tag), so this one call
                // covers both `DeferredRenderObject::Box` and `::Sliver` —
                // before this, a lazily-built list/grid child (of either
                // protocol) never received `attach`, silently starving any
                // render object that subscribes to a `Listenable` there
                // (e.g. a snap-animation controller) of its handle.
                self.attach_inserted_node(child_id);
                self.add_node_needing_layout(child_id, child_depth);
                self.note_render_child_membership_changed(parent_id);
                tracing::trace!(
                    ?parent_id,
                    ?child_id,
                    "apply_deferred_mutation: inserted child and scheduled membership work"
                );
            }
            DeferredMutation::Remove {
                parent_id,
                child_id,
            } => {
                // Cascade-dispose: `remove_render_object` evicts the whole
                // subtree from every dirty queue and frees it recursively
                // (the old `remove_shallow` orphaned + leaked descendants and
                // left stale dirty entries). Then re-dirty the parent so it
                // reflows without the removed child.
                let removed = self.remove_render_object(child_id);
                if removed > 0 {
                    // Re-dirty the parent (flag-setting walk, not a flagless
                    // enqueue) so it reflows without the removed child.
                    self.mark_needs_layout(parent_id);
                }
                tracing::trace!(
                    ?parent_id,
                    ?child_id,
                    removed,
                    "apply_deferred_mutation: removed subtree and re-dirtied parent"
                );
            }
            DeferredMutation::Update { target_id, updater } => {
                if let Some(node) = self.render_tree.get_mut(target_id) {
                    match node {
                        crate::storage::RenderNode::Box(entry) => {
                            updater(entry.render_object_mut() as &mut dyn std::any::Any);
                        }
                        crate::storage::RenderNode::Sliver(entry) => {
                            updater(entry.render_object_mut() as &mut dyn std::any::Any);
                        }
                    }
                    tracing::trace!(?target_id, "apply_deferred_mutation: updated render object");
                }
            }
        }
    }

    /// Returns the constraints to apply when laying out `id` as a
    /// dirty root.
    ///
    /// Sourced from (in priority order):
    ///
    /// 1. The node's cached `state.constraints()` — set on the
    ///    previous frame's successful layout. This is the common
    ///    case for re-layout (constraints unchanged → cache hit
    ///    fast path inside `layout_dirty_root`).
    /// 2. The binding-set [`PipelineOwner::root_constraints`] if `id` is the
    ///    tree root (`root_id`). Used on the very first frame
    ///    before any layout has cached its own constraints.
    /// 3. Otherwise `None` — caller skips this dirty entry with a
    ///    warning (no constraints available means the parent's
    ///    perform_layout must propagate constraints first; the
    ///    dirty queue's shallow-first ordering ensures parents are
    ///    processed before children).
    fn cached_or_root_constraints(&self, id: RenderId) -> Option<BoxConstraints> {
        // The binding-set root constraints are AUTHORITATIVE for the
        // root: on resize, set_root_constraints updates them and marks
        // the root dirty — if the stale cached constraints won here,
        // the resize relayout would run at the OLD window size and the
        // newly exposed area would stay unpainted.
        if self.root_id == Some(id)
            && let Some(root) = self.root_constraints
        {
            return Some(root);
        }
        if let Some(node) = self.render_tree.get(id)
            && let Some(entry) = node.as_box()
            && let Some(cached) = entry.state().constraints()
        {
            return Some(*cached);
        }
        if self.root_id == Some(id) {
            return self.root_constraints;
        }
        None
    }

    /// Production disjoint-borrow layout walk.
    ///
    /// Lays out the subtree rooted at `id` with the supplied
    /// `constraints`, running `RenderObject::perform_layout_raw` against a
    /// typed [`crate::protocol::BoxLayoutCtx`] populated with the parent's
    /// direct children. Returns the parent's computed `Size` on success.
    ///
    /// Replaces the recursion shape of `layout_node_with_children`
    /// (which only walks the dirty tree without invoking per-node
    /// layout — see the audit comment in that method). The
    /// pipeline-side `run_layout` outer loop is wired to this method.
    ///
    /// # Mechanism (pre-acquired subtree borrows)
    ///
    /// 1. **Collect ids**: `RenderTree::collect_subtree_ids(id)` walks
    ///    the subtree in DFS pre-order producing
    ///    `Vec<RenderId>` covering root + all descendants.
    /// 2. **Pre-acquire borrows in ONE scope**:
    ///    `RenderTree::get_subtree_mut(&ids)` materialises N disjoint
    ///    `&mut RenderNode` references in a single function body via
    ///    the proven `*mut Slab` reborrow pattern that already powers
    ///    `RenderTree::get_two_mut` and
    ///    `RenderTree::get_parent_and_children_mut`.
    /// 3. **Index by id**: the N borrows are wrapped in a private
    ///    `SubtreeArena` as a `HashMap<RenderId, (NodePtr, AtomicBool)>`
    ///    (raw pointer alias of the still-live `&mut RenderNode` borrows,
    ///    paired with a per-slot in-flight flag). Lookup is O(1) by id.
    /// 4. **Recursive walk**: a private `layout_subtree_borrowed`
    ///    helper indexes into `SubtreeArena` to acquire one node's
    ///    reborrow at each call level. The leaf path delegates to
    ///    [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only).
    ///    The non-leaf path constructs a Direct-storage `BoxLayoutCtx`
    ///    via the erased driver context ([`crate::protocol::ErasedBoxLayoutCtx`]) with a closure
    ///    that captures `&SubtreeArena` (Sync via `NodePtr`'s
    ///    `unsafe impl`) and re-enters `layout_subtree_borrowed` for
    ///    each child. The bridge in `traits/render_box.rs` reconstructs
    ///    a typed `BoxLayoutCtx<T::Arity, T::ParentData>` (Proxy
    ///    variant) and forwards to `RenderBox::perform_layout`.
    ///    Synchronous `ctx.layout_child(i, c)` calls dispatch through
    ///    the callback, recursing into the child's subtree via its
    ///    pre-acquired `NodePtr`.
    /// 5. **Per-level cleanup**: on success **AND when no descendant
    ///    errored**, updates `state.set_geometry` /
    ///    `state.set_constraints`, bootstraps `IS_RELAYOUT_BOUNDARY`,
    ///    clears `NEEDS_LAYOUT`. When a descendant errored
    ///    mid-callback, geometry + constraints are still recorded but
    ///    `NEEDS_LAYOUT` stays set on the parent so the next dirty
    ///    walk re-runs the subtree — with one bound: a descendant that
    ///    exhausts the layout-poison retry budget is skipped in place
    ///    from then on (its last committed geometry stands in), and
    ///    the flags it left set are cleared by the poison bookkeeping
    ///    instead of retrying forever.
    ///
    /// # Soundness (Miri-clean)
    ///
    /// The prior design used a recursive raw-pointer
    /// re-entry into `RenderTree` from inside the layout-child callback —
    /// outer `&mut RenderEntry` was held LIVE across `perform_layout_raw`
    /// while the inner call synthesised a fresh `&mut RenderTree` from
    /// the same `*mut`. Under Stacked / Tree Borrows that invalidates
    /// the outer tag (latent UB; Miri flagged the 2-level and 3-level
    /// happy paths).
    ///
    /// This redesign eliminates the inner reborrow entirely. All
    /// N disjoint `&mut RenderNode` borrows are acquired in ONE call to
    /// `get_subtree_mut` (single `&mut Slab` reborrow scope, mirroring
    /// `get_parent_and_children_mut`). The callback then acquires one
    /// node's reborrow per call level by dereferencing the pre-acquired
    /// `NodePtr` for that slot — no `&mut RenderTree` ever appears
    /// inside the callback chain.
    ///
    /// At any given moment during the walk, the per-slot reborrows in
    /// scope are: one for the current call's parent + one for each
    /// active recursive child call below it. All on **distinct slab
    /// slots** (parent ≠ child in a well-formed tree). The Unique tag
    /// for each slot lives independently because `NodePtr` is a raw
    /// pointer (SharedReadWrite permission on the allocation, distinct
    /// derived Unique tags per slot reborrow). Miri verifies this on
    /// the existing integration tests.
    ///
    /// # Error handling
    ///
    /// - **Leaf-path panics** in user `perform_layout` → caught by
    ///   `layout_leaf_only`'s `catch_unwind`, returned as
    ///   [`crate::error::RenderError::Poisoned`].
    /// - **Non-leaf-path panics**: wrapped in
    ///   `catch_unwind` at the non-leaf `perform_layout_raw` call site,
    ///   returned as the same [`crate::error::RenderError::Poisoned`].
    ///   Symmetric with the leaf path.
    /// - **Descendant `Err` returned through the callback** → tracking
    ///   flag (`AtomicBool`) set; outer `perform_layout` still completes
    ///   with `Size::ZERO` for that child; outer `Ok` is returned to the
    ///   caller, BUT parent's `NEEDS_LAYOUT` is **not cleared** — unless
    ///   the failure tips the child over its layout-poison budget, in
    ///   which case the failed child (and its direct layout parent) have
    ///   the flag cleared by the poison bookkeeping below and the child
    ///   is skipped in later walks.  The `LayoutChildCallback`
    ///   signature is `Fn(_) -> Size`, not `Fn(_) -> Result<Size, _>`,
    ///   so callback failures can't propagate as typed `Err` through
    ///   the parent's `perform_layout` body; instead they reach this
    ///   method through the arena's failure sink as
    ///   `(parent, failed, kind)` triples, which is what the poison
    ///   counters consume.  Surfacing the typed error
    ///   to the outer caller requires widening `LayoutChildCallback`
    ///   to `Result`; deferred to Core.1.
    /// - **Stale tree state** (id not in tree → `NodeNotFound`; id is
    ///   present but wrong protocol → `ProtocolMismatch`).
    ///
    /// # ParentData scope (current limitation)
    ///
    /// The pipeline-side Direct `BoxLayoutCtx` is parameterised over
    /// [`crate::parent_data::BoxParentData`], so widgets whose `T::ParentData` is the default
    /// (`RenderPadding`, `RenderCenter`, `RenderColoredBox`,
    /// `RenderOpacity`, `RenderTransform`, `RenderSizedBox`) drive
    /// through correctly. Non-default parent-data types (e.g.,
    /// `FlexParentData` on `RenderFlex`) trigger a
    /// `BoxLayoutCtx::from_erased` debug-assert mismatch in debug builds
    /// and silently fail to downcast in release builds. Pipeline-driven
    /// flex layout is out of scope for this method; per-render-object
    /// `T::ParentData` dispatch lands as a Core.1 follow-up alongside the
    /// real `RenderFlex` slice integration.
    ///
    /// # Cycle / depth safety
    ///
    /// Three-layer cycle protection:
    ///
    /// 1. `collect_subtree_ids` terminates safely on cycles via its
    ///    `visited` `HashSet<RenderId>` short-circuit —
    ///    the cyclic id is visited at most once, the cycle edge is
    ///    silently dropped from the collected subtree, deduplicated
    ///    `Vec<RenderId>` returns. No hang / OOM at the collect
    ///    phase.
    /// 2. `get_subtree_mut` receives the deduplicated id list →
    ///    uniqueness precondition satisfied → returns `Some(refs)`.
    ///    No double-borrow attempt at acquisition.
    /// 3. `layout_subtree_borrowed` marks each `id`'s in-flight flag in
    ///    `SubtreeArena::by_id` via the
    ///    `LayoutCycleGuard` RAII on entry. A `perform_layout`
    ///    body that calls `layout_child` for an ancestor id already
    ///    in flight hits the guard's `enter` collision check →
    ///    returns [`crate::error::RenderError::LayoutCycle`]
    ///    immediately instead of attempting a second `NodePtr`
    ///    reborrow (which would be UB).
    ///
    /// The cycle error collapses through the layout-child callback
    /// (Size::ZERO + `descendant_error_flag`) so the parent stays
    /// `NEEDS_LAYOUT` for next-frame retry. A layout cycle is a
    /// structural failure, so the first occurrence already engages the
    /// layout poison on the re-entered node: the retry is attempted at
    /// most once per fresh external invalidation rather than every
    /// frame — predictably, never as panic/UB/hang. The user can fix the
    /// tree (remove the cyclic `add_child`) and the next invalidation
    /// lifts the poison, so the retry succeeds.
    ///
    /// Frame-cross panic safety: `LayoutCycleGuard::Drop` runs on
    /// every exit path including unwind from a panicking
    /// `perform_layout`. Combined with the non-leaf path's
    /// `catch_unwind` wrapper, the cycle set stays consistent across
    /// frames — a panic does not leak an in-flight id.
    ///
    /// `run_layout`'s dirty-queue iteration calls this method directly.
    pub fn layout_dirty_root(
        &mut self,
        id: RenderId,
        constraints: BoxConstraints,
    ) -> crate::error::RenderResult<Size> {
        // Steps 1–3: collect subtree ids, pre-acquire disjoint &mut borrows,
        // and wrap them in a SubtreeArena for O(1) by-id lookup during the
        // recursive walk.  The unsafe aliasing machinery lives entirely inside
        // `subtree_arena::SubtreeArena`; this call site is safe.  The arena
        // also borrows the poison table read-only for the walk so poisoned
        // nodes are skipped in place.
        let arena = SubtreeArena::from_tree(
            &mut self.render_tree,
            id,
            &self.layout_poison,
            #[cfg(any(test, feature = "testing"))]
            &self.parent_data_seeds,
        )?;

        // Step 4: recursive walk via the safe arena API.  Internally the arena
        // reborrows one NodePtr per call level (distinct slots; parent ≠ child
        // enforced by LayoutCycleGuard), but no `unsafe` appears here.
        let result = arena.layout_child(id, constraints);

        // Step 5: drain all three arena sinks (re-entrant build contract v1,
        // per ADR-0003 Decision 2).
        // Take all three (owned), then DROP `arena` to release the &mut RenderTree
        // subtree borrow before touching `&mut self`.
        //
        // Ordering: Remove → Insert → Request.  Removes first so that an
        // off-band child evicted this pass does not collide with the Insert that
        // replaces it in the same batch.  Requests last because they carry no
        // pre-built object and do not mutate the tree.
        let pending_removes = arena.take_pending_removes();
        let pending_builds = arena.take_pending_builds();
        let pending_child_requests = arena.take_pending_child_requests();
        let pending_retain_bands = arena.take_pending_retain_bands();
        let layout_failures = arena.take_layout_failures();
        let layout_successes = arena.take_layout_successes();
        drop(arena);

        // Poison bookkeeping for the walk's descendant failures/successes.
        // A success clears the node's failure record; failures are deduped
        // to one count per node per walk (a node failing both layout and
        // an intrinsic query in one pass burns its budget once), and on
        // the 0 → 1 poison transition the failed node AND its direct
        // layout parent have their NEEDS_LAYOUT flags cleared: the
        // parent's geometry with the poisoned child's stand-in size is the
        // same value any further retry would produce, so keeping the flag
        // would only hide the subtree from paint without changing it.
        for succeeded in layout_successes {
            self.layout_poison.note_success(succeeded);
        }
        for (parent, failed, kind, first_report) in
            self.layout_poison.note_failures(layout_failures)
        {
            if let Some(node) = self.render_tree.get(failed) {
                node.clear_needs_layout();
            }
            if parent != failed
                && let Some(node) = self.render_tree.get(parent)
            {
                node.clear_needs_layout();
            }
            if first_report {
                tracing::error!(
                    ?failed,
                    ?parent,
                    ?kind,
                    "layout poison engaged: node failed layout with a structural \
                     error (or exhausted its retry budget); skipping it in layout \
                     until it is freshly invalidated (mark_needs_layout). Its last \
                     committed geometry (or ZERO) stands in.",
                );
            } else {
                tracing::debug!(
                    ?failed,
                    ?parent,
                    ?kind,
                    "layout poison re-engaged after a fresh invalidation; the \
                     node still fails layout and stays skipped.",
                );
            }
        }

        // Apply removes first.  Each entry is `(parent, child)`:
        // the parent is the sliver's own node_id (tagged at push time in
        // ErasedSliverLayoutCtx::dispose_box_child), NOT the walk root `id`.
        // Using `id` here would misdirect `mark_needs_layout` to the viewport
        // root instead of the lazy sliver, preventing it from reflowing.
        for (parent, child_id) in pending_removes {
            self.defer_remove(parent, child_id);
        }

        for pending in pending_builds {
            self.defer_insert_box(
                pending.parent,
                pending.object,
                Some(pending.index),
                Some(pending.logical_index),
                pending.initial_parent_data,
            );
        }

        // Move child-build requests into the owner's observable buffer so the
        // binding layer can consume them after the frame.  No tree
        // mutation here — the requests are inert until a manager wires them up.
        self.pending_child_requests.extend(pending_child_requests);

        // Move retain-band signals from element-owned slivers into the
        // owner's observable buffer.  The binding layer drains
        // these via `take_pending_retain_bands` to drive `SparseChildren::
        // retain_band` on the element side, skipping `dispose_box_child` to
        // avoid the ABA double-remove.
        self.pending_retain_bands.extend(pending_retain_bands);

        result
    }
}

impl PipelineOwner<Layout> {
    /// Queue every repaint boundary inside `root`'s subtree for paint.
    ///
    /// [`Self::mark_needs_paint`] walks UP to the nearest established
    /// boundary, which is the right shape for an invalidation originating at a
    /// leaf. It is the wrong shape for "this whole subtree just re-laid out":
    /// the boundaries INSIDE the subtree also ran layout, their content is
    /// stale, and nothing walking upward from the subtree's root will ever
    /// reach them.
    ///
    /// Flutter has no equivalent sweep because it marks per object —
    /// `RenderObject.layout` ends with `markNeedsPaint()` for every object it
    /// lays out. This is that behaviour expressed once per relayout root
    /// instead of once per object, which costs one downward walk over a
    /// subtree layout has just traversed anyway.
    ///
    /// Descends through boundaries rather than stopping at them: a boundary
    /// nested inside another one laid out just the same.
    ///
    /// # Known imprecision
    ///
    /// This queues every boundary under `root`, including ones whose layout
    /// the walk skipped because their constraints were unchanged. That is the
    /// safe direction — an extra repaint, never a stale one — but it is
    /// blunter than Flutter, which marks exactly the objects it laid out
    /// because the mark happens inside `RenderObject.layout`. Reaching the
    /// same precision here means recording ids from inside the layout walk
    /// (`subtree_arena`), where the node's slot is exclusively borrowed and
    /// the scheduler is not in scope. Worth doing before a retaining paint
    /// pass relies on the queue to decide what to skip, since over-queueing
    /// costs exactly the repaints retention is meant to avoid.
    fn queue_boundaries_under(&mut self, root: RenderId) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.render_tree.get(id) else {
                continue;
            };
            stack.extend_from_slice(node.children());
            // The root itself is already handled by the `mark_needs_paint`
            // above.
            //
            // Route through `mark_needs_paint` rather than enqueueing
            // directly. It is the one place that decides WHICH node owns the
            // affected retained layer, and the answer is not always this node:
            // a boundary introduced this frame
            // (`is_repaint_boundary_flag() && !was_repaint_boundary()`) owns
            // no layer yet, so invalidation has to walk past it to an
            // established ancestor — the contract
            // `mark_needs_paint_walks_past_a_new_repaint_boundary` pins.
            // Enqueueing here would also leave `NEEDS_PAINT` unset on the
            // node, putting the queue and the per-node flags out of step.
            //
            // It stays cheap: the walk returns at the first already-dirty
            // node, so an established boundary costs one step.
            let is_boundary = node.is_repaint_boundary_flag();
            if id != root && is_boundary {
                self.mark_needs_paint(id);
            }
        }
    }
}

#[cfg(test)]
mod deferred_insert_tests {
    use flui_tree::Leaf;
    use flui_types::Size;

    use super::*;
    use crate::{
        context::BoxLayoutContext, parent_data::BoxParentData,
        pipeline::deferred::DeferredRenderObject, traits::RenderBox,
    };

    #[derive(Debug)]
    struct LeafBox;

    impl flui_foundation::Diagnosticable for LeafBox {}

    impl RenderBox for LeafBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            Size::ZERO
        }
    }

    #[test]
    fn deferred_insert_applies_exact_parent_membership_impact() {
        let mut idle_owner = PipelineOwner::new();
        let parent = idle_owner.set_root_render_object(Box::new(LeafBox));
        idle_owner.set_semantics_enabled(true);
        idle_owner.clear_all_dirty_nodes();
        let parent_node = idle_owner.render_tree().get(parent).unwrap();
        parent_node.clear_needs_layout();
        parent_node.clear_needs_paint();
        parent_node.clear_needs_compositing_bits_update();

        let mut owner = idle_owner.into_layout();
        owner.apply_deferred_mutation(DeferredMutation::Insert {
            parent_id: parent,
            render_object: DeferredRenderObject::Box(Box::new(LeafBox)),
            index: None,
            logical_index: None,
            initial_parent_data: None,
        });

        let child = owner.render_tree().get(parent).unwrap().children()[0];
        let layout_ids = owner
            .nodes_needing_layout()
            .iter()
            .map(|dirty| dirty.id)
            .collect::<Vec<_>>();
        assert!(layout_ids.contains(&parent));
        assert!(layout_ids.contains(&child));
        assert!(owner.render_tree().get(parent).unwrap().needs_layout());
        assert!(
            owner
                .render_tree()
                .get(parent)
                .unwrap()
                .needs_compositing_bits_update()
        );
        assert_eq!(owner.nodes_needing_compositing_bits_update()[0].id, parent);
        assert_eq!(owner.nodes_needing_semantics()[0].id, parent);
        assert!(owner.nodes_needing_paint().is_empty());
    }
}

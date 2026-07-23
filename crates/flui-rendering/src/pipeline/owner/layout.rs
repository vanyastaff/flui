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

use super::{PipelineOwner, rebind_phase, subtree_arena::SubtreeArena};

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
                if let Err(e) = self.layout_dirty_root(dirty_node.id, constraints) {
                    // Drain mid-phase marks back into `dirty` even on
                    // the error path so they survive across phase
                    // invocations.
                    let _ = self.scheduler.exit_phase(PhaseKind::Layout);
                    return Err(e);
                }

                // Flutter parity (object.dart: `RenderObject.layout`
                // unconditionally ends with `markNeedsPaint()`): a
                // subtree that re-laid out must repaint, otherwise a
                // pure-layout invalidation (setState moving a child)
                // leaves stale pixels on screen. One entry per dirty
                // root suffices — run_paint walks the whole tree from
                // the root, so triggering the phase is what matters.
                self.mark_needs_paint(dirty_node.id);
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
                // A deferred insert must schedule the new child (and re-dirty
                // its parent) for layout and paint — mirroring
                // `insert_child_render_object`. Without this the fresh node
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
                self.mark_needs_paint(parent_id);
                // `mark_needs_layout` (not `add_node_needing_layout`) so the
                // parent's NEEDS_LAYOUT flag is actually set and its relayout
                // boundary is enqueued — the dirty-root walk skips queued
                // entries whose flag is clear, so a flagless enqueue would
                // leave the new child unpositioned at the origin.
                self.mark_needs_layout(parent_id);
                tracing::trace!(
                    ?parent_id,
                    ?child_id,
                    "apply_deferred_mutation: inserted child and scheduled layout + paint"
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
    ///    walk re-runs the subtree.
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
    ///   caller, BUT parent's `NEEDS_LAYOUT` is **not cleared**.
    ///   Next-frame dirty walk re-runs the parent. The `LayoutChildCallback`
    ///   signature is `Fn(_) -> Size`, not `Fn(_) -> Result<Size, _>`,
    ///   so callback failures can't propagate as typed `Err` through
    ///   the parent's `perform_layout` body. Surfacing the typed error
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
    /// `NEEDS_LAYOUT` for next-frame retry. The cycle persists
    /// structurally so retry will re-surface `LayoutCycle` — but
    /// predictably, never as panic/UB/hang. The user can fix the
    /// tree (remove the cyclic `add_child`) and the next retry
    /// succeeds.
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
        // `subtree_arena::SubtreeArena`; this call site is safe.
        let arena = SubtreeArena::from_tree(
            &mut self.render_tree,
            id,
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
        drop(arena);

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

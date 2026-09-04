//! Layout phase implementation for `PipelineOwner<Layout>`.

use flui_foundation::RenderId;
use flui_types::Size;

use crate::{
    constraints::BoxConstraints,
    pipeline::{
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
            }

            // exit_phase clears debug_doing_layout AND drains
            // mid_layout_marks back into `dirty` so the outer while
            // condition picks up marks routed to the side queue
            // during this iteration's `debug_doing_layout = true`
            // window.
            let _ = self.scheduler.exit_phase(PhaseKind::Layout);
        }

        Ok(())
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

        // Step 5: drain the arena's request-strategy sinks (the
        // request-strategy seam's producer/removal halves).
        // Take both (owned), then DROP `arena` to release the &mut RenderTree
        // subtree borrow before touching `&mut self`.
        let pending_child_requests = arena.take_pending_child_requests();
        let pending_retain_bands = arena.take_pending_retain_bands();
        let layout_failures = arena.take_layout_failures();
        let layout_successes = arena.take_layout_successes();
        let laid_out = arena.take_laid_out();
        drop(arena);

        // Flutter marks needs-paint per object at the end of
        // `RenderObject.layout`; this is the same rule, applied once per walk
        // to exactly the boundaries that reached layout. Anything else is
        // covered by `mark_needs_paint` finding its enclosing boundary.
        for id in laid_out {
            self.mark_needs_paint(id);
        }

        // Flutter pairs `performLayout()` with `markNeedsSemanticsUpdate()` in
        // both of `RenderObject`'s layout entry points, so a node that lays out
        // re-publishes its semantics geometry. Without it a frame that only
        // re-laid-out publishes nothing, and a scroll — which is exactly that
        // frame, since the viewport's offset listener requests layout and
        // nothing else — leaves the accessibility tree describing where the
        // content used to be.
        //
        // ONE mark, on the dirty root, not one per laid-out node. The arena
        // walks the subtree of `id`, so every node that laid out is under it,
        // and `try_graft_pass` re-assembles a marked node's whole subtree —
        // marking each descendant as well would be strictly redundant while
        // costing a `fire_need_visual_update` per node (each of which asks the
        // platform to redraw) and a per-node ancestor walk in the graft, i.e.
        // O(N·depth) for a deep relayout. `mark_needs_semantics` is itself a
        // no-op while semantics is disabled, so a session with no
        // accessibility client attached pays one predictable branch.
        self.mark_needs_semantics(id);

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

        // Move child-build requests into the owner's observable buffer so the
        // binding layer can consume them after the frame. No tree mutation
        // here — the request sits inert until the binding layer's child
        // manager services it between layout passes of this frame's fixpoint.
        self.pending_child_requests.extend(pending_child_requests);

        // Move retain-band signals from element-owned slivers into the
        // owner's observable buffer.  The binding layer drains these via
        // `take_pending_retain_bands` to drive `SparseChildren::retain_band`
        // on the element side.  The render side never disposes a child
        // itself, which is what avoids an ABA double-remove.
        self.pending_retain_bands.extend(pending_retain_bands);

        result
    }
}

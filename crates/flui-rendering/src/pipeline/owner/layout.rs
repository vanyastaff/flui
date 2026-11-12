//! Layout phase implementation for `PipelineOwner<Layout>`.

use flui_foundation::RenderId;
use flui_foundation::geometry::Size;

use crate::{
    constraints::BoxConstraints,
    pipeline::{
        phase::{Compositing, Idle, Layout},
        scheduler::PhaseKind,
    },
};

use super::{PipelineOwner, poison::LayoutFailureKind, rebind_phase, subtree_arena::SubtreeArena};
use crate::pipeline::dirty::DirtyNode;

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
    /// before their children.
    ///
    /// # Synchronous Child Layout
    ///
    /// The pre-acquired subtree lends each render object to its parent-driven
    /// walk. `layout_child()` lays out the child synchronously and returns its
    /// result; a rejected measurement propagates before the parent can publish
    /// dependent geometry.
    pub fn run_layout(&mut self) -> crate::error::RenderResult<()> {
        let _span =
            tracing::debug_span!(target: "flui.frame", "layout", dirty_nodes = self.scheduler.layout_queue_len(),)
                .entered();

        // Paint owed by the relayout roots is committed as each root finishes
        // and announced by one wake once the batches are back in the scheduler:
        // a wake is user code, so it runs only after the state it reports is
        // committed (ADR-0287).
        //
        // The wake goes out on every exit, an error or a panic included. When
        // the pass already failed, that failure is the first one: a wake that
        // panics after it is discarded (ADR-0281 §2).
        let mut paint_queued = false;
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the layout batches' panic still lets the paint wake go out"
        )]
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_layout_batches(&mut paint_queued)
        }));
        if paint_queued {
            if matches!(outcome, Ok(Ok(()))) {
                self.scheduler.notify_visual_update();
            } else {
                flui_foundation::panic::FirstPanic::new()
                    .contain(|| self.scheduler.notify_visual_update());
            }
        }
        match outcome {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Runs layout batches until no layout work is left, handing each batch
    /// back to the scheduler however its pass ends.
    fn run_layout_batches(&mut self, paint_queued: &mut bool) -> crate::error::RenderResult<()> {
        // While loop to handle nodes added during layout
        while self.scheduler.has_layout_work() {
            self.scheduler.enter_phase(PhaseKind::Layout);

            // Take the dirty nodes and replace with empty set
            // This allows new nodes to be added during layout (routed
            // to mid_layout_marks; drained back at end of
            // iteration below).
            let dirty_nodes = self
                .scheduler
                .take_layout_batch_shallow_first(&self.render_tree);
            if !dirty_nodes.is_empty() {
                self.counters.layout_passes += 1;
            }

            // The batch is out of the scheduler from here on. A pass that
            // stops early, with an error or with a panic out of user code (a
            // `tracing` subscriber, a render object, a wake), leaves the phase
            // and puts its unfinished entries back before the failure goes on,
            // so the retry lays out what this pass did not.
            let mut next = 0;
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a layout pass's panic puts unfinished entries back first"
            )]
            let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.layout_batch(&dirty_nodes, &mut next, paint_queued)
            }));

            // exit_phase clears debug_doing_layout AND drains
            // mid_layout_marks back into `dirty` so the outer while
            // condition picks up marks routed to the side queue
            // during this iteration's `debug_doing_layout = true`
            // window.
            let _ = self.scheduler.exit_phase(PhaseKind::Layout);
            match pass {
                Ok(Ok(())) => self.scheduler.recycle_layout_batch(dirty_nodes),
                Ok(Err(stop)) => {
                    // `resume_at` is `next` or the entry after it: in bounds.
                    self.scheduler
                        .retain_layout_batch(&dirty_nodes[stop.resume_at..], stop.wait_for_input);
                    self.scheduler.recycle_layout_batch(dirty_nodes);
                    return Err(stop.error);
                }
                Err(payload) => {
                    // The entry the panic interrupted may have laid out before
                    // it: its layout flag is clear, so the retry skips it, and
                    // the paint its new geometry owes is committed here. An
                    // entry that never laid out (a skipped one) over-queues one
                    // repaint, the safe direction.
                    if let Some(entry) = dirty_nodes.get(next)
                        && self
                            .render_tree
                            .get(entry.id)
                            .is_some_and(|node| !node.needs_layout())
                    {
                        *paint_queued |= self
                            .scheduler
                            .commit_needs_paint(&self.render_tree, entry.id);
                    }
                    self.scheduler
                        .retain_layout_batch(&dirty_nodes[next..], false);
                    self.scheduler.recycle_layout_batch(dirty_nodes);
                    std::panic::resume_unwind(payload)
                }
            }
        }

        Ok(())
    }

    /// Lays out one batch of relayout roots, shallow-first.
    ///
    /// `next` stays at the first entry not yet finished, so a pass that stops
    /// early leaves the rest of the batch for [`Self::run_layout_batches`] to
    /// retain. Every new paint queue entry sets `paint_queued`.
    fn layout_batch(
        &mut self,
        dirty_nodes: &[DirtyNode],
        next: &mut usize,
        paint_queued: &mut bool,
    ) -> Result<(), BatchStop> {
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
        for (index, dirty_node) in dirty_nodes.iter().enumerate() {
            *next = index;

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
                    id = dirty_node.id.as_u64(),
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
                    id = dirty_node.id.as_u64(),
                    "run_layout: skipping dirty-queue entry whose NEEDS_LAYOUT \
                     was already cleared this iteration",
                );
                continue;
            }

            let Some(constraints) = self.cached_or_root_constraints(dirty_node.id) else {
                // No constraints to lay out against. The two cases
                // this hits, one a bug and one routine:
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
                //      This is ORDINARY for a child mounted during
                //      layout (ADR-0017: `LayoutBuilder`, the
                //      sliver builders) that its parent chose not
                //      to lay out this pass — a list item built
                //      and found off-screen, say — and the child
                //      keeps NEEDS_LAYOUT until the parent does.
                //      A Scaffold body under a LayoutBuilder hit
                //      it once per launch and a scrolling
                //      ListView::builder once per frame, so it is
                //      a debug line, not a warning (measured
                //      2026-09-22 in the macOS lifecycle run).
                // Case 1 stays at warn: the root having no
                // constraints is a binding bug, and a launch that
                // draws nothing is what it looks like.
                if self.root_id == Some(dirty_node.id) {
                    tracing::warn!(
                        id = dirty_node.id.as_u64(),
                        "run_layout: the root has no root_constraints; skipping its \
                         dirty entry. The binding must call set_root_constraints \
                         before run_frame (it auto-marks the root dirty)."
                    );
                } else {
                    tracing::debug!(
                        id = dirty_node.id.as_u64(),
                        "run_layout: non-root dirty entry has no cached constraints \
                         and its parent did not lay it out this pass; it keeps \
                         NEEDS_LAYOUT until the parent's perform_layout calls \
                         ctx.layout_child for it"
                    );
                }
                continue;
            };
            // One span per relayout root, not per node: the walk below it is the root's cost.
            let root_span = tracing::trace_span!(
                target: "flui.frame",
                "layout_root",
                node = dirty_node.id.as_u64(),
            )
            .entered();
            let (laid_out, reports) =
                self.layout_relayout_root(dirty_node.id, constraints, paint_queued);
            match laid_out {
                Ok(_) => {
                    self.layout_poison.note_success(dirty_node.id);
                }
                Err(e) => {
                    // The root's error is the walk's first failure; a poison
                    // report that panicked after it is discarded.
                    drop(reports);
                    if e.is_recoverable_layout_error() {
                        let wait_for_input = matches!(
                            e,
                            crate::RenderError::TextLayout(_) | crate::RenderError::TextContextBusy
                        );
                        return Err(BatchStop {
                            error: e,
                            resume_at: index,
                            wait_for_input,
                        });
                    }
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
                                id = dirty_node.id.as_u64(),
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
                        //
                        // The rest of the batch goes back to the queue
                        // either way: the failed frame's retry lays out the
                        // healthy roots after this one. A root with budget
                        // left goes back with them and is attempted again; a
                        // poisoned one does not.
                        first @ (Some(true) | None) => {
                            let poisoned = first.is_some();
                            if poisoned {
                                if let Some(node) = self.render_tree.get(dirty_node.id) {
                                    node.clear_needs_layout();
                                }
                                tracing::error!(
                                    id = dirty_node.id.as_u64(),
                                    ?kind,
                                    error = ?e,
                                    "layout poison engaged: dirty-root layout \
                                     failed with a structural error (or \
                                     exhausted its retry budget); the node is \
                                     skipped in later layout passes until \
                                     freshly invalidated (mark_needs_layout).",
                                );
                            }
                            return Err(BatchStop {
                                error: e,
                                resume_at: if poisoned { index + 1 } else { index },
                                wait_for_input: false,
                            });
                        }
                    }
                }
            }

            // A subtree that re-laid out must repaint, otherwise a
            // pure-layout invalidation (setState moving a child)
            // leaves stale pixels on screen.
            //
            // The mark must be per OBJECT, and the difference is not
            // cosmetic. `mark_needs_paint` walks UP to the nearest
            // established boundary, so marking only the relayout root
            // reaches the boundary ABOVE the subtree and never the
            // boundaries INSIDE it — which also re-laid out and whose
            // content is therefore stale. A full-tree paint descent
            // hides that; a paint pass that lets a clean boundary reuse
            // its previous layers would show the stale pixels.
            *paint_queued |= self
                .scheduler
                .commit_needs_paint(&self.render_tree, dirty_node.id);

            // A poison report that panicked goes on only now, with the root's
            // own success recorded and its paint committed. The span closes
            // after that too. Either panic leaves this entry queued, and the
            // retry skips it because its layout flag is clear.
            reports.resume();
            drop(root_span);
        }
        *next = dirty_nodes.len();
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
    /// typed [`crate::protocol::RawBoxLayoutContext`] populated with the parent's
    /// direct children. Returns the parent's computed `Size` on success.
    ///
    /// Replaces the recursion shape of `layout_node_with_children`
    /// (which only walks the dirty tree without invoking per-node
    /// layout — see the audit comment in that method). The
    /// pipeline-side `run_layout` outer loop is wired to this method.
    ///
    /// # Mechanism (a lease, nodes acquired on demand)
    ///
    /// 1. **Lease**: `RenderTree::lease` takes the exclusive borrow of the
    ///    tree once, with the slab's base pointer (ADR-0252). Nothing is
    ///    collected or borrowed up front, so the cost of preparing a walk is
    ///    constant.
    /// 2. **Wrap**: the lease goes into a private `SubtreeArena`, which
    ///    resolves an id with the tree's generation check and hands out a
    ///    raw `NodePtr` for it. The in-flight bit and the parent link live in
    ///    the slot headers, never behind a `NodePtr`.
    /// 3. **Recursive walk**: a private `layout_subtree_borrowed`
    ///    helper acquires one node's reborrow at each call level, after
    ///    `LayoutCycleGuard::enter` has refused a cycle or an id that is not a
    ///    child of the frame asking. The leaf path delegates to
    ///    [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only).
    ///    The non-leaf path constructs a Direct-storage `RawBoxLayoutContext`
    ///    via the erased driver context ([`crate::protocol::DriverBoxLayoutContext`]) with a closure
    ///    that captures `&SubtreeArena` (Sync via `NodePtr`'s
    ///    `unsafe impl`) and re-enters `layout_subtree_borrowed` for
    ///    each child. The bridge in `traits/render_box.rs` reconstructs
    ///    a typed `RawBoxLayoutContext<T::Arity, T::ParentData>` (Proxy
    ///    variant) and forwards to `RenderBox::perform_layout`.
    ///    Synchronous `ctx.layout_child(i, c)` calls dispatch through
    ///    the callback, recursing into the child's subtree via its
    ///    `NodePtr`.
    /// 4. **Per-level cleanup**: on success **AND when no descendant
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
    /// This design eliminates the inner reborrow entirely. The lease holds the
    /// one `&mut RenderTree` for the walk, and the callback acquires one
    /// node's reborrow per call level by dereferencing a `NodePtr` derived
    /// from the slab's base pointer — no `&mut RenderTree` ever appears
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
    /// - **Rejected text input** propagates through typed child layout and
    ///   query results. It never supplies stand-in geometry or engages layout
    ///   poison. `run_layout` retains the failed root and unprocessed batch.
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
    /// The pipeline-side Direct `RawBoxLayoutContext` is parameterised over
    /// [`crate::parent_data::BoxParentData`], so widgets whose `T::ParentData` is the default
    /// (`RenderPadding`, `RenderCenter`, `RenderColoredBox`,
    /// `RenderOpacity`, `RenderTransform`, `RenderSizedBox`) drive
    /// through correctly. Non-default parent-data types (e.g.,
    /// `FlexParentData` on `RenderFlex`) trigger a
    /// `RawBoxLayoutContext::from_erased` debug-assert mismatch in debug builds
    /// and silently fail to downcast in release builds. Pipeline-driven
    /// flex layout is out of scope for this method; per-render-object
    /// `T::ParentData` dispatch lands as a Core.1 follow-up alongside the
    /// real `RenderFlex` slice integration.
    ///
    /// # Cycle / depth safety
    ///
    /// Cycle protection:
    ///
    /// 1. Nothing is collected up front, so a cyclic `children()` edge
    ///    cannot hang or exhaust memory in a preparation phase.
    /// 2. `layout_subtree_borrowed` marks each `id`'s in-flight bit in its
    ///    slot header via the `LayoutCycleGuard` RAII on entry. A `perform_layout`
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
    /// `run_layout`'s dirty-queue iteration runs the same walk, announcing the
    /// work of a whole batch with one wake instead of one per root.
    pub fn layout_dirty_root(
        &mut self,
        id: RenderId,
        constraints: BoxConstraints,
    ) -> crate::error::RenderResult<Size> {
        // The wake goes out on every exit. The first failure is the one that
        // goes on: the walk's error or panic, then a poison report's panic,
        // then the wake's; a later one is discarded (ADR-0281 §2).
        let mut queued = false;
        let mut first = flui_foundation::panic::FirstPanic::new();
        let laid_out = first
            .contain(|| self.layout_relayout_root(id, constraints, &mut queued))
            .map(|(laid_out, reports)| {
                if let Some(payload) = reports.into_payload() {
                    first.record(payload);
                }
                laid_out
            });
        if queued {
            first.contain(|| self.scheduler.notify_visual_update());
        }
        match laid_out {
            Some(Ok(size)) => {
                first.resume();
                Ok(size)
            }
            // Dropping `first` discards a panic that came after the error.
            Some(Err(error)) => Err(error),
            None => std::panic::resume_unwind(
                first
                    .into_payload()
                    .expect("BUG: a walk that unwound holds its panic"),
            ),
        }
    }

    /// [`Self::layout_dirty_root`] without its wake: a new paint or semantics
    /// queue entry sets `queued`, and the caller announces the work once.
    ///
    /// The walk's poison reports have run when this returns; a panic out of
    /// one is held in the returned [`FirstPanic`], for the caller to resume
    /// once it has recorded the dirty root's own result. A panic out of the
    /// walk itself goes on from here, after everything the walk finished is
    /// committed and the reports have run.
    ///
    /// [`FirstPanic`]: flui_foundation::panic::FirstPanic
    fn layout_relayout_root(
        &mut self,
        id: RenderId,
        constraints: BoxConstraints,
        queued: &mut bool,
    ) -> (
        crate::error::RenderResult<Size>,
        flui_foundation::panic::FirstPanic,
    ) {
        // Layout clears flags and fills caches under any stamped walk path.
        self.scheduler.forget_layout_walks();
        // Steps 1–2: lease the tree and wrap the lease in a SubtreeArena, which
        // acquires nodes by id as the recursive walk reaches them.  The unsafe aliasing machinery lives entirely inside
        // `subtree_arena::SubtreeArena`; this call site is safe.  The arena
        // also borrows the poison table read-only for the walk so poisoned
        // nodes are skipped in place.
        // The ui_runtime's text context, lent to every node the walk measures.
        // One `Rc` clone per dirty root; the walk borrows the cell once per
        // measurement, never across a child's layout.
        let text = self.text_handle();
        let arena = match SubtreeArena::from_tree(
            &mut self.render_tree,
            id,
            &self.layout_poison,
            &self.layout_buffers,
            crate::pipeline::TextLender::new(text.cell(), &self.text_measurers, &self.text_sizing),
            #[cfg(any(test, feature = "testing"))]
            &self.parent_data_seeds,
        ) {
            Ok(arena) => arena,
            Err(error) => return (Err(error), flui_foundation::panic::FirstPanic::new()),
        };

        // Step 4: recursive walk via the safe arena API.  Internally the arena
        // reborrows one NodePtr per call level (distinct slots; parent ≠ child
        // enforced by LayoutCycleGuard), but no `unsafe` appears here.
        //
        // The walk contains each render object's own hooks, but a panic can
        // still leave it from user code outside them, a `tracing` subscriber
        // reporting on the dirty root, say. It is caught here: the work the
        // walk finished before the panic is committed below exactly as a
        // completed walk's is, and only then does the panic go on.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the layout walk's panic is held until its committed work lands"
        )]
        let walked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            arena.layout_child(id, constraints)
        }));

        // Step 5: drain the arena's request-strategy sinks (the
        // request-strategy seam's producer/removal halves).
        // Take both (owned), then DROP `arena` to release the &mut RenderTree
        // subtree borrow before touching `&mut self`.
        let pending_child_requests = arena.take_pending_child_requests();
        let pending_retain_bands = arena.take_pending_retain_bands();
        let layout_failures = arena.take_layout_failures();
        let layout_successes = arena.take_layout_successes();
        let laid_out = arena.take_laid_out();
        let laid_out_count = arena.take_laid_out_count();
        let acquired_count = arena.take_acquired_count();
        drop(arena);
        self.counters.nodes_laid_out += laid_out_count;
        self.counters.nodes_acquired += acquired_count;

        // Everything the walk produced is committed before the first callout
        // (ADR-0287): the child requests and retain bands, the poison records,
        // then the paint and semantics queue entries. The poison reports run
        // last.

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
        // A panic below the dirty root is caught by its parent's containment,
        // so a walk that unwound did so from the root's own frame, which may
        // not have finished. The root keeps its flag then, and the retry lays
        // it out again; every other parent finished its frame.
        let unwound = walked.is_err();
        let poison_reports = self.layout_poison.note_failures(layout_failures);
        for &(parent, failed, _, _) in &poison_reports {
            if let Some(node) = self.render_tree.get(failed) {
                node.clear_needs_layout();
            }
            if parent != failed
                && !(unwound && parent == id)
                && let Some(node) = self.render_tree.get(parent)
            {
                node.clear_needs_layout();
            }
        }

        // Needs-paint is marked per object that laid out, applied once per
        // walk to exactly the boundaries that reached layout. Anything else is
        // covered by `mark_needs_paint` finding its enclosing boundary.
        for id in laid_out {
            *queued |= self.scheduler.commit_needs_paint(&self.render_tree, id);
        }

        // A node that lays out re-publishes its semantics geometry. Without
        // that, a frame that only
        // re-laid-out publishes nothing, and a scroll — which is exactly that
        // frame, since the viewport's offset listener requests layout and
        // nothing else — leaves the accessibility tree describing where the
        // content used to be.
        //
        // ONE entry, on the dirty root, not one per laid-out node. The arena
        // walks the subtree of `id`, so every node that laid out is under it,
        // and `plan_graft_pass` re-assembles a marked node's whole subtree —
        // marking each descendant as well would be strictly redundant while
        // costing a per-node ancestor walk in the graft, i.e. O(N·depth) for a
        // deep relayout. A session with no accessibility client attached pays
        // one predictable branch.
        if self.semantics_enabled()
            && let Some(node) = self.render_tree.get(id)
        {
            *queued |= self
                .scheduler
                .commit_node_needing_semantics(id, usize::from(node.depth()));
        }

        // A poison report reaches a `tracing` subscriber, which is user code.
        // Each report runs even when an earlier one panics, so a first
        // transition is never left unreported, and the first panic goes on
        // once they all have.
        let mut reports = flui_foundation::panic::FirstPanic::new();
        for (parent, failed, kind, first_report) in poison_reports {
            reports.contain(|| report_layout_poison(parent, failed, kind, first_report));
        }
        match walked {
            Ok(result) => (result, reports),
            // The walk's panic came first; dropping `reports` discards a
            // later one under containment (ADR-0281 §2).
            Err(payload) => {
                drop(reports);
                std::panic::resume_unwind(payload)
            }
        }
    }
}

/// Why a layout batch stopped before its last entry, and where the rest of it
/// resumes.
///
/// A stop always hands the unfinished entries back to the queue: there is no
/// stop that ends the batch. An entry left out would keep `NEEDS_LAYOUT` with
/// nothing queued to clear it, so neither the retry the runtime arms after a
/// failed frame nor a later frame would lay it out until an unrelated mark
/// walked over it.
struct BatchStop {
    error: crate::error::RenderError,
    /// Index of the first entry put back. It is the failed root itself while
    /// that root is to be attempted again, and the entry after it once the
    /// failure poisoned it (the poison skip would only discard it).
    resume_at: usize,
    /// Whether the entries put back wait for changed input (an authored
    /// rejection) or are runnable as they are.
    wait_for_input: bool,
}

/// Reports a node's layout poison to the `tracing` subscriber: an error on
/// the first transition, a debug line when a fresh invalidation re-engages it.
fn report_layout_poison(
    parent: RenderId,
    failed: RenderId,
    kind: LayoutFailureKind,
    first_report: bool,
) {
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

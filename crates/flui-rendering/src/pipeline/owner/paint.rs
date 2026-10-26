//! Paint phase implementation for `PipelineOwner<PaintPhase>`.

use std::sync::Arc;

use flui_foundation::geometry::{Offset, Size};
use flui_foundation::{LayerId, RenderId};
use flui_layer::{
    AnnotatedRegionLayer, BackdropFilterLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
    ClipSuperellipseLayer, ContentToken, FollowerLayer, Layer, LayerNode, LayerTree, LeaderLayer,
    OffsetLayer, OpacityLayer, PictureLayer, ShaderMaskLayer, TransformLayer,
};
use flui_painting::DisplayList;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{
    context::{FragmentOp, FragmentRecorder, FragmentScope, PaintAnchor},
    error::PoisonPhase,
    pipeline::{
        phase::{Idle, PaintPhase, Semantics},
        scheduler::PhaseKind,
    },
    traits::{PaintClip, PaintEffects, resolve_path_clip},
};

use super::{PipelineOwner, counters::PaintCounts, rebind_phase, subtree_arena::ensure_stack};

// ============================================================================
// Paint phase: run_paint + helpers
// ============================================================================

impl PipelineOwner<PaintPhase> {
    /// Transitions a paint-phase pipeline into the [`Semantics`] phase.
    #[must_use]
    pub fn into_semantics(self) -> PipelineOwner<Semantics> {
        rebind_phase(self)
    }

    /// Returns to [`Idle`] from the paint phase.
    #[must_use]
    pub fn into_idle(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Paints all dirty render objects.
    ///
    /// Phase 3 of the rendering pipeline, as a **fragment composition**
    /// (sans-IO paint model): each node's `paint_raw` records a
    /// node-local fragment — draw runs, child markers, clip scopes —
    /// which is immediately replayed into the frame's [`LayerTree`].
    /// Adjacent inline draw runs merge into shared `PictureLayer`s;
    /// repaint-boundary children are rebased to `Offset::ZERO` under
    /// their own `OffsetLayer`; clip scopes become real clip layers.
    ///
    /// A fresh `LayerTree` is produced every paint pass, but not a fresh
    /// full one: a repaint boundary absent from the dirty set is grafted
    /// from `retained_boundaries` rather than repainted, and the layers a
    /// graft re-inserts are clones that share their `DisplayList` behind an
    /// `Arc`. A boundary queued only for a composited-layer update is
    /// grafted too, with just the effect layers of the node that asked for
    /// the update rebuilt — see `layer_patches_for`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::RenderError::PaintBeforeLayout`] when the
    /// paint phase begins with layout work still pending — the scheduler's
    /// layout queue is non-empty. This is the one misuse the type system
    /// cannot rule out: a caller may legally drive the phases directly
    /// (`into_layout().into_compositing().into_paint()`, the bench
    /// pattern) and skip
    /// [`PipelineOwner::run_layout`](super::PipelineOwner::run_layout), so
    /// completeness stays a runtime gate. The empty-queue variant of that
    /// chain is designed behavior and keeps returning `Ok`.
    pub fn run_paint(&mut self) -> crate::error::RenderResult<()> {
        // Frame-completeness gate, BEFORE the empty-paint-queue early
        // return: the paint phase began with layout work still pending. A
        // direct phase chain that skipped `run_layout` with a serviced
        // queue must be refused, not silently paint stale geometry; the
        // same chain with an EMPTY layout queue is the legitimate bench
        // pattern (`benches/semantics_assembly.rs`) and passes through.
        if self.scheduler.has_layout_work() {
            return Err(crate::error::RenderError::PaintBeforeLayout);
        }

        if !self.scheduler.has_paint_work() {
            return Ok(());
        }

        let _span = tracing::debug_span!(
            "paint",
            dirty_nodes = self.scheduler.paint_queue_len(),
            // How many of those are queued only to have some node's own effect
            // layers rebuilt. A frame whose two counts are equal repainted
            // nothing — which is what distinguishes a layer-only frame from a
            // paint in a trace.
            layer_updates = self
                .scheduler
                .nodes_needing_paint()
                .iter()
                .filter(|e| matches!(e.kind, crate::pipeline::PaintKind::LayerUpdate(_)))
                .count(),
        )
        .entered();

        self.scheduler.enter_phase(PhaseKind::Paint);
        self.local_dirty.begin_paint();

        // Deepest-first ordering retained: the
        // full-tree descent below repaints everything, but per-boundary
        // dirty-driven repaints will rely on this order once retention
        // lands, and keeping it now means the dirty-list semantics
        // don't shift under that change.
        self.scheduler.sort_paint_deep_first();

        // Which queued nodes the descent actually reached — the residue scan's
        // oracle. Empty when there is no root to descend from, which correctly
        // makes every queued entry unreached.
        let mut reached: FxHashSet<RenderId> = FxHashSet::default();
        if let Some(root_id) = self.root_id
            && self.render_tree.get(root_id).is_some()
        {
            // Build a set of dirty node IDs for O(1) lookup during the
            // paint walk.
            let dirty_ids: FxHashSet<RenderId> = self
                .scheduler
                .nodes_needing_paint()
                .iter()
                .map(|d| d.id)
                .collect();

            let root_boundary = self
                .render_tree
                .get(root_id)
                .is_some_and(crate::storage::RenderNode::is_repaint_boundary)
                .then_some(root_id);
            // Boundaries queued only to have some node's own effect layers
            // rebuilt. Intersected with "does not need paint", which is where
            // paint precedence is enforced: a boundary marked for a real
            // repaint after its layer-update mark drops out here: a node that
            // needs paint is repainted, otherwise only its layer properties
            // are updated.
            //
            // `dirty_ids` above deliberately stays the FULL queue, so a
            // boundary becomes graft-eligible only by appearing in this set —
            // the pre-existing retention path cannot change behaviour when
            // nothing requested an update.
            // The queue carries its own reason, so there is no second record
            // to reconcile and nothing here reads a node flag. That matters
            // because `run_paint`'s error arm returns before
            // `clear_paint_queue`: after a pass that failed partway the queue
            // is what the retry runs on, while every flag the walk touched is
            // already clear. A classification kept anywhere else has to be
            // proven consistent with this one at four write sites — and the
            // orderings that broke it (update-then-paint, paint-then-update,
            // and a mark arriving after the failure) are all just joins on one
            // entry now.
            let layer_updates: FxHashMap<RenderId, SmallVec<[RenderId; 2]>> = self
                .scheduler
                .nodes_needing_paint()
                .iter()
                .filter_map(|e| match &e.kind {
                    crate::pipeline::PaintKind::LayerUpdate(targets) => {
                        Some((e.id, targets.clone()))
                    }
                    crate::pipeline::PaintKind::Repaint => None,
                })
                .collect();

            // The root's content version: kept while the root is not queued
            // (every paint change under it that no nested boundary absorbs
            // queues the root, since `mark_needs_paint` stops at the nearest
            // boundary), minted otherwise. Committed with the frame below; an
            // errored frame drops a minted token, and the retry, finding the
            // root still queued, mints again.
            let root_stamp = root_boundary.map(|id| {
                let kept = (!dirty_ids.contains(&id))
                    .then_some(self.root_content.as_ref())
                    .flatten()
                    .filter(|(committed, _)| *committed == id)
                    .map(|(_, token)| token.clone());
                (id, kept.unwrap_or_else(ContentToken::mint))
            });
            let mut composer = FragmentComposer::new(self.device_pixel_ratio, root_stamp.clone());
            let inputs = PaintInputs {
                dirty_set: &dirty_ids,
                layer_updates: &layer_updates,
            };
            match self.paint_subtree(&mut composer, root_id, Offset::ZERO, None, &inputs) {
                Ok(()) => {
                    let (
                        layer_tree,
                        follower_correlations,
                        retained_captures,
                        layer_patches,
                        consumed_updates,
                        visited,
                        counts,
                    ) = composer.finish();
                    reached = visited;
                    self.root_content = root_stamp;
                    self.counters.nodes_painted += counts.nodes_painted;
                    self.counters.layers_produced += counts.layers_produced;
                    self.counters.layers_reused += counts.layers_reused;
                    tracing::debug!("run_paint: layer tree has {} layers", layer_tree.len());

                    // Commit the walk's retention decisions now that its
                    // `&self` borrow has ended.
                    let mut committed_captures: Vec<RenderId> = Vec::new();
                    for (id, captured) in retained_captures {
                        committed_captures.push(id);
                        match captured {
                            Some(subtree) => {
                                self.retained_boundaries.insert(id, subtree);
                            }
                            None => {
                                self.retained_boundaries.remove(&id);
                            }
                        }
                    }

                    // Write the patches into the stored captures, so the next
                    // frame that grafts one replays the NEW property rather
                    // than the value it was captured with. Skipped for a
                    // boundary the loop above just replaced or evicted: that
                    // capture is fresh output, already carrying the current
                    // properties.
                    // Now that the frame has committed, the requests those
                    // patches serve are satisfied. An error above returned
                    // before this point, leaving the flags set so the retry
                    // re-derives them.
                    //
                    // Hygiene rather than correctness for the OUTPUT: patches
                    // are idempotent over live properties, so a flag left set
                    // is re-applied harmlessly by the next graft (a mutation
                    // removing this loop leaves every test green). It matters
                    // because a mark refuses itself while the flag is set, so
                    // never clearing would stop the boundary being re-queued —
                    // and the queued targets are what let the graft detect a
                    // node whose effect layers appeared and refuse.
                    for render_id in consumed_updates {
                        if let Some(node) = self.render_tree.get(render_id) {
                            node.clear_needs_composited_layer_update();
                        }
                    }

                    let mut served: FxHashSet<RenderId> =
                        layer_patches.iter().map(|(id, _, _)| *id).collect();
                    for (boundary_id, content, patches) in layer_patches {
                        let Some(subtree) = self.retained_boundaries.get_mut(&boundary_id) else {
                            continue;
                        };
                        // The token the frame stamped: a grafted boundary that
                        // was queued (for a layer update) minted one, and the
                        // capture must carry it, or the next frame that grafts
                        // it would report the pre-patch version as changed
                        // again.
                        subtree.content = content;
                        for (index, layer) in patches {
                            if let Some(Entry::Layer(node)) = subtree.entries.get_mut(index) {
                                node.layer = layer;
                            }
                        }
                    }
                    served.extend(committed_captures);

                    // A boundary queued for an update that the walk never
                    // reached — unplaced by its parent's latest layout, or in a
                    // detached subtree — keeps a capture the request never got
                    // to touch, and the residue scan below cannot see it
                    // because a layer update leaves no `needs_paint` to test.
                    // Its own flag also survives, and a mark refuses itself
                    // while that flag is set, so nothing would re-queue it.
                    //
                    // Evicting is the bounded answer, and the same one the
                    // residue scan gives for the paint case: the next frame
                    // that reaches this boundary repaints it, which serves any
                    // request correctly whatever shape it changed.
                    let unserved: SmallVec<[RenderId; 2]> = layer_updates
                        .keys()
                        .copied()
                        .filter(|id| !served.contains(id))
                        .collect();
                    if !unserved.is_empty() {
                        self.retained_boundaries
                            .retain(|id, _| !unserved.contains(id));
                    }

                    // Follower hit-testing: resolve each paint-phase-correlated
                    // follower's composite-resolved offset against the SAME
                    // fully-built layer_tree the GPU path (flui-engine's
                    // `render_layer_recursive`) resolves against, reusing the
                    // identical `resolve_follower_offset` — one algorithm, two
                    // consumers (pixels + hit-test), not two copies of the
                    // logic. The tree carries the leader index itself.
                    let mut follower_offsets = FxHashMap::default();
                    let mut hidden_follower_ids = FxHashSet::default();
                    for (render_id, follower_layer_id) in follower_correlations {
                        debug_assert!(
                            layer_tree
                                .get_layer(follower_layer_id)
                                .is_some_and(|layer| layer.as_follower().is_some()),
                            "BUG: a follower correlation must name a Layer::Follower node"
                        );
                        match flui_layer::resolve_follower_offset(&layer_tree, follower_layer_id) {
                            Some(offset) => {
                                follower_offsets.insert(render_id, offset);
                            }
                            None => {
                                hidden_follower_ids.insert(render_id);
                            }
                        }
                    }
                    self.last_follower_offsets = follower_offsets;
                    self.last_hidden_follower_ids = hidden_follower_ids;

                    // A u64 of frames outlasts any process; reusing the last number instead
                    // would let a kept layer id read the next scene's layer.
                    self.scenes_painted = self
                        .scenes_painted
                        .checked_add(1)
                        .expect("BUG: u64 scene numbers exhausted");
                    let scene = std::num::NonZeroU64::new(self.scenes_painted)
                        .expect("BUG: incremented from at least 0 just above");
                    self.last_layer_tree = Some(layer_tree.with_scene(scene));
                }
                Err(e) => {
                    // Restore the debug invariant before propagating so
                    // the owner stays consistent on the error path.
                    let _ = self.local_dirty.end_paint();
                    let _ = self.scheduler.exit_phase(PhaseKind::Paint);
                    return Err(e);
                }
            }
        }

        // Dirty-list residue scan: any queued node the root descent did not
        // REACH (multi-root, detached subtree, or a child its parent stopped
        // laying out). Warn + clear so the bug is visible AND the dirty list
        // doesn't accumulate across frames.
        //
        // Keyed on what the walk recorded, not on the node still being flagged
        // needs-paint. That flag is cleared by the walk itself while
        // `run_paint`'s error arm keeps the queue for a retry, so after a pass
        // that failed partway the flag test skips exactly the entries this scan
        // exists to catch — and since the scan is also the only diagnostic on
        // that path, it fails silently. A boundary left with a stale capture
        // that way is grafted forever.
        for dirty_node in self.scheduler.nodes_needing_paint() {
            if !reached.contains(&dirty_node.id)
                && let Some(render_node) = self.render_tree.get(dirty_node.id)
            {
                tracing::warn!(
                    id = ?dirty_node.id,
                    depth = dirty_node.depth,
                    "run_paint: dirty node not reached by root descent (multi-root, \
                     detached subtree, or a child its parent stopped laying out); \
                     paint dropped, flag cleared, retained capture evicted"
                );
                render_node.clear_needs_paint();
                // Dropping the invalidation must drop the cached output with
                // it. `mark_needs_paint` stops at the nearest established
                // boundary, so a node in this list IS one, and leaving its
                // capture behind means the next frame that reaches it grafts
                // output that was already known to be stale — the flag that
                // would have forced a repaint has just been cleared here.
                //
                // Reachable through the placed-generation gate above: a child
                // its parent stopped laying out is skipped, so a paint-only
                // update it received while unplaced lands in this scan. If the
                // parent later places it again with unchanged constraints its
                // layout short-circuits and requeues nothing, and without this
                // eviction the stale capture would be grafted.
                self.retained_boundaries.remove(&dirty_node.id);
            }
        }
        // `clear()` retains capacity (preserve Vec backing across frames).
        self.scheduler.clear_paint_queue();

        // exit_phase clears debug_doing_paint AND drains mid-paint marks back
        // into dirty so paint marks made during this pass become next-frame
        // work rather than being stranded.
        //
        // Finding 2 (intentional improvement over pre-refactor behavior):
        // exit_phase also drains mid-marks on the ERROR path (the early-return
        // above calls exit_phase before returning Err). Pre-refactor, the error
        // path only cleared debug_doing_paint and did NOT drain mid-marks, so
        // any mark made between enter_phase and the error was silently lost.
        // The always-drain contract of exit_phase is the correct behavior:
        // mid-paint marks scheduled before the error survive into the next
        // frame's retry rather than being dropped.
        let layout_during_paint = self.local_dirty.end_paint();
        let _ = self.scheduler.exit_phase(PhaseKind::Paint);
        debug_assert!(
            layout_during_paint.is_none(),
            "BUG: render object {layout_during_paint:?} requested layout during paint; \
             paint must not change geometry"
        );

        Ok(())
    }

    /// Records one node's paint fragment and replays it into the
    /// composer, recursing at child markers.
    ///
    /// Per-node order: `WAS_REPAINT_BOUNDARY` is written and `NEEDS_PAINT` cleared
    /// **before** the node paints, so a paint body that re-marks its own
    /// node is caught by the debug check below instead of silently
    /// erasing the evidence.
    fn paint_subtree(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
        anchor: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        ensure_stack(|| self.paint_subtree_impl(composer, node_id, origin, anchor, inputs))
    }

    /// The effect-layer replacements a grafted `subtree` needs this frame, or
    /// `Ok(None)` when one of them cannot be expressed as a patch.
    ///
    /// Walks the nodes whose own effect layers this capture holds and rebuilds
    /// those of them that asked for a composited-layer update, through the
    /// same [`own_effect_layers`] the paint walk uses. Everything else in the
    /// capture is replayed untouched.
    ///
    /// Returns `Ok(None)` — meaning "repaint instead" — when a node's effect
    /// layers no longer have the same SHAPE they were captured with: a
    /// different count, or a different layer kind at the same position. Both
    /// are structural changes a patch cannot express (an alpha crossing out of
    /// the layered range drops its layer entirely), and both are correctly
    /// served by a repaint. Comparing the discriminant per position rather
    /// than just the count is what catches the case where one effect appears
    /// as another disappears and the count is unchanged.
    ///
    /// # Errors
    ///
    /// `Poisoned` with phase [`PoisonPhase::LayerUpdate`] when a target's
    /// `paint_effects` — or the resolution of a `PaintClip::PathTarget` it
    /// reports — panics while the patch is built; the frame is discarded and
    /// the queued update survives for the retry.
    fn layer_patches_for(
        &self,
        subtree: &RetainedSubtree,
        targets: &[RenderId],
    ) -> crate::error::RenderResult<Option<LayerPatch>> {
        // A node that asked for an update this frame but owns no slot in this
        // capture cannot be served by a patch: its effect layers did not exist
        // when the capture was taken, so there is nothing to replace and the
        // loop below would never even look at it. That is a shape change, and
        // a repaint is the only thing that can express it.
        //
        // A childless `RenderRotatedBox` is a real, reachable caller of this
        // branch: its setter reports `COMPOSITED_LAYER_UPDATE` for a
        // same-parity turn change whether or not it has a child (no
        // `has_child` gate), `paint_effects` returns a `None` transform
        // without a child, so it never owns an effect slot to begin with — and
        // `RotatedBox::new(n)` seeds an empty child by default, so a bare
        // rebuild hits exactly this. Without this guard the loop below walks
        // EXISTING slots, silently skips a target that has none, and reports
        // `Some` — an empty, no-op patch that grafts the boundary's stale
        // capture instead of refusing. `run_paint`'s `consumed_updates` bookkeeping
        // then never clears the node's own `NEEDS_COMPOSITED_LAYER_UPDATE`
        // flag either (that list is populated from what this function
        // consumes, or from a REAL paint walk — neither runs on the silent
        // no-op path), stranding the flag set and self-refusing every later
        // mark to the same node. A test for it must observe the flag staying
        // set, not the emitted frame, which looks the same either way for a
        // node that paints nothing regardless.
        if targets.iter().any(|target| {
            !subtree
                .effect_slots
                .iter()
                .any(|(owner, _)| owner == target)
        }) {
            return Ok(None);
        }
        let mut patches = Vec::new();
        // Nodes whose request this patch serves. Returned rather than cleared
        // here, and cleared by `run_paint` only once the frame commits: the
        // walk can still fail after this point (a later sibling's `paint_raw`
        // can poison), and that path discards the composer with its patches
        // while the dirty queue survives for the retry. Clearing during the
        // walk would leave the retry seeing no pending request, grafting the
        // old layer, and going visibly stale until something else mutates.
        let mut consumed: SmallVec<[RenderId; 2]> = SmallVec::new();
        // Iterated in capture order (the `Vec`'s order) rather than hash
        // order: a target this loop rebuilds resolves in the same sequence a
        // full repaint's paint walk would visit it in — see the field doc on
        // `RetainedSubtree::effect_slots`.
        for (render_id, slots) in &subtree.effect_slots {
            let render_id = *render_id;
            let Some(node) = self.render_tree.get(render_id) else {
                // The node is gone but its layers are still in the capture.
                // Nothing asked for an update, so replaying them is what a
                // clean frame would do anyway.
                continue;
            };
            if !node.needs_composited_layer_update() {
                continue;
            }
            // `AssertUnwindSafe` is sound here for the same reason the paint
            // walk's own `paint_raw` wrapper is: the closure captures `node`
            // (a view over the `dyn RenderObject` state) and `slots.origin`
            // by shared reference. Two different things are at stake if
            // `paint_effects` panics, and only one of them is protected
            // here: the node's OWN state (interior-mutable fields, if it has
            // any) may be left torn, and nothing below repairs that — it is
            // the render object's problem, not this function's. What IS
            // protected is this function's pipeline bookkeeping: the `?`
            // below returns out of this function before `consumed` (see its
            // doc above) is ever handed back to the caller, so the
            // composited-layer-update request this loop would have consumed
            // stays on the queue, untouched, for the retry.
            // Refusing here and falling back to a repaint was rejected: the
            // repaint calls this same `paint_effects` on the same node and
            // would panic a second time, so the poison must surface once,
            // not be laundered into an infinite retry loop.
            let fresh = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                own_effect_layers(node.paint_effects(), slots.origin)
            }))
            .map_err(|_| {
                crate::error::RenderError::poisoned(node.debug_name(), PoisonPhase::LayerUpdate)
            })?;
            if fresh.len() != slots.indices.len() {
                return Ok(None);
            }
            for (&index, layer) in slots.indices.iter().zip(fresh) {
                // Indices come from this same capture, so they are in range by
                // construction — but this runs inside the paint walk, where an
                // out-of-range index would panic a frame rather than lose one
                // reuse. Refusing degrades to a repaint, which is always
                // correct.
                let Some(Entry::Layer(captured)) = subtree.entries.get(index) else {
                    debug_assert!(false, "BUG: effect slot index outside its own capture");
                    return Ok(None);
                };
                // Under the FIXED order `own_effect_layers` emits (opacity,
                // clip, transform), the count check above plus this
                // per-position discriminant compare is a COMPLETE shape
                // check — no stored shape is needed. It is conservative
                // rather than load-bearing: the effect chain is positional
                // (`capture` records it in push order, `replay` swaps only
                // `layer` at those indices), so a same-count kind change
                // patched positionally would already yield the repaint's
                // tree; refusing it only trades a correct patch for a
                // repaint, and that refusal is observable as one. No
                // production producer changes its own clip kind at runtime,
                // and no test pins the fallback.
                if std::mem::discriminant(&layer) != std::mem::discriminant(&captured.layer) {
                    return Ok(None);
                }
                patches.push((index, layer));
            }
            consumed.push(render_id);
        }
        Ok(Some(LayerPatch { patches, consumed }))
    }

    /// Body of [`Self::paint_subtree`]; split out so every recursion
    /// level enters through the [`ensure_stack`] probe.
    fn paint_subtree_impl(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
        anchor: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        let Some(render_node) = self.render_tree.get(node_id) else {
            return Ok(());
        };
        // Reached, whatever happens below: the early returns for skip-paint,
        // pending layout and invisible slivers are all decisions the descent
        // MADE about this node, not evidence it never got here.
        //
        // Recorded only for QUEUED nodes. The residue scan never asks about
        // anything else, and paint is a full-tree descent every frame — an
        // unconditional insert would cost a hash write per render object per
        // frame to answer a question about a handful of ids.
        if inputs.dirty_set.contains(&node_id) {
            composer.visited.insert(node_id);
        }

        let is_repaint_boundary = render_node.is_repaint_boundary();

        // Boundary status has TWO readers and they must not disagree. Paint (here)
        // asks the trait live; the scheduler's `owns_retained_layer`, the
        // compositing-bits walk, and `laid_out`'s paint marking all read the
        // cached FLAG, which is bootstrapped once at insert and never re-synced.
        //
        // While every production `is_repaint_boundary()` returns a literal
        // constant they agree trivially. A dynamic one — say an opacity object
        // whose boundary status follows whether it always needs compositing —
        // desyncs them, and fails in two directions at once: inert on the fast
        // path (a fade mounting at an endpoint bootstraps `false` and stays
        // there), and CORRUPTING on relayout (paint pushes a boundary layer and
        // stores a capture, but flag-gated `laid_out` never marks it needs-paint,
        // so the next frame grafts a stale capture over new geometry).
        //
        // Avoiding this needs the flag re-synced at the same moment
        // `mark_needs_compositing_bits_update` fires; this assert is what
        // makes walking into the trap loud instead of silent. Issue #995.
        debug_assert_eq!(
            is_repaint_boundary,
            render_node.is_repaint_boundary_flag(),
            "BUG: repaint-boundary flag and trait answer disagree for {node_id:?} -- \
             the flag is insert-time configuration and is not re-synced, so a \
             render object whose is_repaint_boundary() varies at runtime silently \
             corrupts retained layers (issue #995). Re-sync the flag where the \
             status changes, beside mark_needs_compositing_bits_update."
        );

        // Borrowed, not copied: `&self` keeps the tree immutable for the walk.
        let child_ids = render_node.children();
        // The generation this node's most recent layout stamped onto the
        // children it laid out; a child carrying anything else was not part of
        // that pass and is skipped at the splice below.
        let parent_generation = render_node.layout_generation();

        // Written unconditionally PRE-paint:
        // a node flipping boundary→non-boundary leaves exactly one
        // `WAS_REPAINT_BOUNDARY=true` trail for the next compositing
        // walk's lost-boundary branch.
        render_node.set_was_repaint_boundary(is_repaint_boundary);

        // Clear BEFORE paint so the post-paint check catches a paint
        // body that marks its own node dirty (paint-must-not-redirty).
        render_node.clear_needs_paint();
        // A repaint rebuilds this node's effect layers from current properties
        // anyway, so it subsumes any pending layer-property update. RECORDED
        // rather than cleared, and cleared by `run_paint` only once the frame
        // commits: a later sibling can still poison the pass, and that path
        // keeps both the dirty queue and the recorded update targets for the
        // retry. Clearing here would leave the retry with a boundary that no
        // longer needs paint — reclassified as update-only — whose target is no
        // longer flagged, so it would graft the pre-error capture unpatched.
        //
        // Recorded before the early returns below: a node skipped for layout
        // must not strand the request either.
        if render_node.needs_composited_layer_update() {
            composer.consumed_updates.push(node_id);
        }

        // Fully transparent subtree: skip recording entirely. Children
        // keep whatever dirty flags they carry; the residue scan in
        // run_paint clears them with a warning.
        // Uses `skip_paint()` rather than `effects.opacity`'s alpha being
        // `Some(0)` so that `paint_effects()`'s opacity field only controls
        // layer-emission; the skip-paint decision is a separate, explicit
        // contract (a fully transparent opacity paints nothing).
        if render_node.skip_paint() {
            return Ok(());
        }

        // A node that still needs layout must not paint stale geometry.
        // Layout runs before paint in the pipeline, so this guards descendant-error and partial-frame
        // paths where a poisoned layout left the flag set.
        if render_node.needs_layout() {
            return Ok(());
        }

        // Sliver visibility cull: a sliver with zero paint extent
        // (`!visible`) paints nothing and splices no children. The gate lives here, in
        // the driver — next to the sliver hit-test extent gate — so sliver
        // objects no longer cache `geometry` just to short-circuit their
        // own `paint`. Box nodes (`geometry_sliver() == None`) are never
        // culled here. Same dirty-residue handling as the `alpha == 0`
        // skip above.
        if render_node.geometry_sliver().is_some_and(|g| !g.visible) {
            return Ok(());
        }

        // Record the node's fragment. paint_raw sees ONLY the recorder
        // (sans-IO): no tree access, no layer access, no recursion.
        //
        // `paint_effects()` is read HERE, inside this same `catch_unwind`,
        // behind the same three gates as `paint_raw` itself:
        //
        // (a) a gated-out node (fully transparent, still needing layout, or
        //     a culled sliver) never builds a descriptor it will not use.
        //     That matters because a node reached with `needs_layout` still
        //     set carries stale geometry -- its last committed size, or
        //     `Size::ZERO` if it has never been laid out -- so building its
        //     descriptor unconditionally would build it against a size this
        //     pass never computed;
        // (b) a panic in `paint_effects` -- or in the walk's
        //     `PaintClip::PathTarget` resolution inside `own_effect_layers`'s
        //     clip arm -- surfaces as `RenderError::poisoned(name,
        //     PoisonPhase::Paint)` instead of an unwind through `run_paint`
        //     that leaves the phase never exited. Building the own-effect
        //     layers in the SAME closure as `paint_raw` keeps a single
        //     poison point per node rather than two.
        //
        // `AssertUnwindSafe` is sound here because the closure only borrows
        // `render_node` (a view over the `dyn RenderObject` state) and
        // writes into the local `recorder`; a panic mid-closure can leave
        // only the node's own interior-mutable state (if it has any) torn,
        // which this poisoned-frame design accepts rather than repairs --
        // see `layer_patches_for`'s identical wrapper, above, for why that
        // is fine.
        // The anchor this node and its subtree measure from: itself when it is
        // one, otherwise the one its parent passed down.
        let anchor = if render_node.is_paint_anchor() {
            Some(AnchorFrame {
                base: -origin,
                size: render_node.geometry_box().unwrap_or(Size::ZERO),
                local: true,
            })
        } else {
            anchor
        };
        let debug_name = render_node.debug_name();
        let mut recorder = FragmentRecorder::new(origin, self.device_pixel_ratio)
            .with_paint_anchor(anchor.map(|frame| frame.at(origin)));
        let own_effects = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_node.paint_raw(&mut recorder, child_ids.len());
            own_effect_layers(render_node.paint_effects(), origin)
        }))
        .map_err(|_| crate::error::RenderError::poisoned(debug_name, PoisonPhase::Paint))?;
        composer.note_painted();
        let fragment = recorder.finish();

        debug_assert!(
            !render_node.needs_paint(),
            "paint-must-not-redirty: a render object marked ITSELF \
             needs-paint during its own paint; derive visual changes \
             from state read at paint time instead of re-marking",
        );

        // A node's own paint effects wrap the ENTIRE node fragment (self
        // draws AND children). The pre-fragment walk wrapped children only;
        // `paint_effects()` implementors draw nothing themselves, so the
        // visible result is identical (an opacity wraps its child's whole
        // paint).
        let effect_layers = own_effects.len();
        for layer in own_effects {
            let layer_id = composer.push_layer(layer);
            // Record where this node's own effect layers landed, so a later
            // frame can rebuild just these and replay everything else.
            composer.record_effect_layer(node_id, layer_id, origin);
        }

        for op in fragment.ops {
            match op {
                FragmentOp::Run(list) => composer.append_run(list),
                FragmentOp::Push(scope) => {
                    // Follower hit-testing: capture the RenderId -> LayerId
                    // correlation as a near-free byproduct of pushing a
                    // Layer::Follower — `node_id` is already in scope from
                    // this fragment-op replay loop, and `push_layer` hands
                    // back the freshly-minted LayerId.
                    let is_follower = matches!(*scope, FragmentScope::Follower { .. });
                    let layer_id = composer.push_layer(scope_layer(*scope, origin));
                    if is_follower {
                        composer.record_follower_correlation(node_id, layer_id);
                    }
                }
                FragmentOp::PushTransform(matrix) => {
                    composer.push_layer(Layer::Transform(TransformLayer::new(conjugate(
                        *matrix, origin,
                    ))));
                }
                FragmentOp::Pop => composer.pop_layer(),
                FragmentOp::Child {
                    index,
                    offset_override,
                } => {
                    let Some(&child_id) = child_ids.get(index) else {
                        debug_assert!(
                            false,
                            "fragment child marker {index} out of range ({} children) — \
                             PaintCx bounds-checks markers, so a mismatch means the \
                             tree changed during paint",
                            child_ids.len(),
                        );
                        continue;
                    };
                    let Some(child_node) = self.render_tree.get(child_id) else {
                        continue;
                    };
                    // Skip a child this pass did not lay out. Its committed
                    // offset describes a pass that no longer holds — a lazy
                    // sliver's out-of-band resident is the case that makes it
                    // visible — and painting it there puts content where
                    // nothing is. `parent_generation` is the parent's current
                    // layout generation; a child it laid out was stamped with
                    // that value at the layout commit.
                    //
                    // Hit-test and semantics read the same stamp — see
                    // `RenderState::placed_generation`.
                    if !child_node.was_placed_by(node_id, parent_generation) {
                        continue;
                    }
                    if child_node
                        .as_sliver()
                        .and_then(|entry| entry.state().geometry())
                        .is_some_and(|geometry| !geometry.visible)
                    {
                        continue;
                    }
                    // Authoritative child position: RenderState.offset,
                    // committed by the layout walk; paint_child_at
                    // overrides it explicitly.
                    let child_offset = offset_override.unwrap_or_else(|| child_node.offset());
                    if child_node.is_repaint_boundary() {
                        self.place_boundary(
                            composer,
                            child_id,
                            origin,
                            offset_override,
                            anchor,
                            inputs,
                        )?;
                    } else {
                        // Inline children bake into the shared picture
                        // space — runs merge, no extra layer.
                        self.paint_subtree(
                            composer,
                            child_id,
                            origin + child_offset,
                            anchor,
                            inputs,
                        )?;
                    }
                }
            }
        }

        for _ in 0..effect_layers {
            composer.pop_layer();
        }

        Ok(())
    }

    /// Places the repaint boundary `child_id` under the layer the composer is
    /// building: stamps its `OffsetLayer`, then either replays the boundary's
    /// retained capture or paints it afresh and captures the result.
    ///
    /// Reached where the walk meets a boundary child and, through
    /// [`Self::replay`], at each [`NestedSlot`] of a capture being reused, so a
    /// nested boundary is decided on its own whatever its enclosing boundary
    /// does. `origin` is the parent's accumulated origin in the current layer
    /// space and `anchor` the paint-anchor frame in force at the parent.
    fn place_boundary(
        &self,
        composer: &mut FragmentComposer,
        child_id: RenderId,
        origin: Offset,
        offset_override: Option<Offset>,
        anchor: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        ensure_stack(|| {
            self.place_boundary_impl(composer, child_id, origin, offset_override, anchor, inputs)
        })
    }

    /// Body of [`Self::place_boundary`]; split out so every recursion level
    /// enters through the [`ensure_stack`] probe.
    fn place_boundary_impl(
        &self,
        composer: &mut FragmentComposer,
        child_id: RenderId,
        origin: Offset,
        offset_override: Option<Offset>,
        anchor: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        let Some(child_node) = self.render_tree.get(child_id) else {
            return Ok(());
        };
        // Authoritative child position: RenderState.offset, committed by the
        // layout walk; paint_child_at overrides it explicitly.
        let position = offset_override.unwrap_or_else(|| child_node.offset());
        let dirty_set = inputs.dirty_set;
        let capture = self.retained_boundaries.get(&child_id);

        // The boundary's content version, certified here
        // (`flui-rendering`'s ARCHITECTURE.md, "Paint certifies a boundary's
        // content token"): kept only while the boundary is absent from the FULL
        // queue — so a layer update counts as a change — and still has a
        // retained capture to vouch for what it painted last. Everything else
        // mints. Decided before the push because the stamp sits on the layer
        // pushed next, and it holds whichever way the boundary is served
        // below: a replay carries exactly the certified content.
        let content = (!dirty_set.contains(&child_id))
            .then_some(capture)
            .flatten()
            .map_or_else(ContentToken::mint, |subtree| subtree.content.clone());
        // Boundary children rebase to ZERO under their own OffsetLayer so a
        // future offset-only move is a layer-property update, not a repaint.
        let boundary_root = composer.push_boundary_layer(
            Layer::Offset(OffsetLayer::new(origin + position)),
            child_id,
            content.clone(),
        );
        composer.slot_info.insert(
            boundary_root,
            SlotPlacement {
                origin,
                offset_override,
                anchor: anchor.filter(|frame| frame.local),
            },
        );

        // Three dispositions, read straight off the queue: queued for a
        // repaint, queued only for a composited-layer update, or not queued at
        // all. The last two both reuse the retained output; only the first
        // descends. Sound only because the queue is exact: `run_layout` marks
        // every boundary layout touched and `mark_needs_paint` marks the rest,
        // so absence from `dirty_set` genuinely means unchanged content.
        let update_targets = inputs.layer_updates.get(&child_id);
        let queued_for_repaint = update_targets.is_none() && dirty_set.contains(&child_id);
        let retained = (!queued_for_repaint).then_some(capture).flatten();
        // `None` here means the structure changed under a layer-update request
        // (a node gained or lost an effect layer), which a patch cannot express
        // — fall back to a full repaint, which rebuilds it correctly. An `Err`
        // here means a target's own effect layers panicked while being
        // rebuilt; it propagates as a poisoned frame rather than falling back.
        let patch = retained
            .map(|subtree| {
                self.layer_patches_for(subtree, update_targets.map_or(&[][..], |targets| targets))
            })
            .transpose()
            .inspect_err(|_| composer.pop_layer())?
            .flatten();
        // The boundary's layer space starts at its own origin, so the anchor's
        // base moves with it.
        let region_anchor = anchor.map(|frame| frame.rebased(origin + position));
        let placed = if let (Some(subtree), Some(patch)) = (retained, patch) {
            let LayerPatch { patches, consumed } = patch;
            if !patches.is_empty() {
                tracing::trace!(
                    boundary = ?child_id,
                    layers = patches.len(),
                    "paint: composited-layer update, subtree replayed"
                );
            }
            // Reused rather than descended into, but reached. Same
            // queue-scoped rule as the walk's own record.
            if dirty_set.contains(&child_id) {
                composer.visited.insert(child_id);
            }
            let replayed = self.replay(composer, subtree, &patches, region_anchor, inputs);
            composer.consumed_updates.extend(consumed);
            // Pushed even when empty: this doubles as the record that the frame
            // SERVED this boundary, which `run_paint` uses to tell a boundary
            // it reused from one the walk never reached. The write-back loop
            // treats an empty patch list as a no-op.
            //
            // The stored capture has to move with the frame. Patching only the
            // emitted tree would leave the cache at the old value, and a later
            // frame that replays it for an unrelated reason would silently
            // revert the property for good.
            composer.layer_patches.push((child_id, content, patches));
            replayed
        } else {
            let painted =
                self.paint_subtree(composer, child_id, Offset::ZERO, region_anchor, inputs);
            // Seal before capturing: the boundary's trailing run is part of its
            // output, and `pop_layer` would otherwise flush it after the
            // snapshot was taken.
            composer.seal_picture();
            painted.map(|()| {
                // `None` evicts: a subtree that GAINED a Leader/Follower must
                // not be served its pre-link form. See `capture`.
                let captured = composer.capture(boundary_root, content);
                composer.retained_captures.push((child_id, captured));
            })
        };
        // Popped on the error path too, so the stack stays balanced.
        composer.pop_layer();
        placed
    }

    /// Re-inserts a capture's entries, in paint order, under the layer the
    /// composer is building, and places each nested boundary at its slot.
    ///
    /// Mints fresh ids and maps the flattened parent indices through them, so
    /// the result is structurally identical to what painting would have
    /// produced — the layers themselves are clones, cheap because a
    /// `PictureLayer` shares its `DisplayList` behind an `Arc`. A slot is
    /// decided by [`Self::place_boundary`], from the boundary's current
    /// position and queue state, with `frame` rebased to it.
    fn replay(
        &self,
        composer: &mut FragmentComposer,
        retained: &RetainedSubtree,
        patches: &[(usize, Layer)],
        frame: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        // Indexed once rather than scanned per entry. A linear `find` inside the
        // clone loop is O(retained entries x patched layers), and both operands
        // grow together on exactly the tree this feature targets — many
        // animated opacities under one boundary, all updating in one frame.
        let patch_by_index: FxHashMap<usize, &Layer> = patches
            .iter()
            .map(|(index, layer)| (*index, layer))
            .collect();
        // Seal first: an open picture run belongs BEFORE the replayed content
        // in draw order, exactly as it would if the boundary had painted.
        composer.seal_picture();
        let root = composer.current_parent();

        // `None` for a slot, which owns no layer of this capture.
        let mut minted: Vec<Option<LayerId>> = Vec::with_capacity(retained.entries.len());
        for (index, entry) in retained.entries.iter().enumerate() {
            let parent_of = |parent: Option<usize>, minted: &[Option<LayerId>]| {
                parent.map_or(root, |i| {
                    minted[i].expect("BUG: an entry's parent is a layer, never a slot")
                })
            };
            match entry {
                Entry::Layer(node) => {
                    // A patched index re-emits this node's layer built from the
                    // render object's CURRENT properties instead of the captured
                    // one; everything else is the cheap `Arc`-sharing clone
                    // retention is built on.
                    //
                    // The two are counted apart: a patched layer is new output
                    // built this pass, not a reuse, and folding it into
                    // `layers_reused` would hide exactly the per-layer cost an
                    // animated opacity pays.
                    let layer = if let Some(layer) = patch_by_index.get(&index) {
                        composer.counts.layers_produced += 1;
                        (*layer).clone()
                    } else {
                        composer.counts.layers_reused += 1;
                        node.layer.clone()
                    };
                    let parent = parent_of(node.parent, &minted);
                    let id = composer.tree.push_child(parent, LayerNode::new(layer));
                    minted.push(Some(id));
                }
                Entry::Slot(slot) => {
                    minted.push(None);
                    let parent = parent_of(slot.parent, &minted);
                    composer.adopt_parent(parent);
                    let nested = self.place_boundary(
                        composer,
                        slot.boundary,
                        slot.origin,
                        slot.offset_override,
                        slot.anchor.or(frame),
                        inputs,
                    );
                    composer.pop_layer();
                    nested?;
                }
            }
        }
        Ok(())
    }
}

// ============================================================================
// Fragment composition (paint phase plumbing)
// ============================================================================

/// A repaint boundary's painted output, kept for reuse on a later frame.
///
/// The boundary's own region only: the layers it painted itself, flattened in
/// preorder with local indices, and a [`NestedSlot`] where each repaint
/// boundary nested in it sits (ADR-0253). `parent` is `None` for an entry that
/// hung directly off the boundary's own `OffsetLayer`, and `Some(i)` for a
/// child of `entries[i]`. Local indices rather than `LayerId`s because every
/// frame builds a fresh `LayerTree` whose slab indices differ — replaying mints
/// new ids and maps them through this vector.
///
/// This is offset-independent, which is what makes it reusable at all: the
/// paint walk rebases a boundary's subtree to `Offset::ZERO` under its own
/// `OffsetLayer`, so a boundary that merely MOVED needs a different offset on
/// the layer above and nothing else.
#[derive(Clone, Debug)]
pub(super) struct RetainedSubtree {
    /// Paint order, one stream: an inline run between two nested boundaries
    /// stays between them, so blend and occlusion order are the painted order
    /// whatever the capture state.
    entries: Vec<Entry>,
    /// The content version the boundary was stamped with when this capture
    /// was taken or last served — what a later frame keeps while the
    /// boundary stays clean.
    content: ContentToken,
    /// Where each render node's OWN effect layers landed in `entries`.
    ///
    /// This is what makes an update-only commit addressable: a node whose
    /// alpha changed but whose painted content did not can have exactly these
    /// entries rebuilt while the rest of the capture is replayed untouched.
    ///
    /// Keyed by the node that pushed them, so it covers every node inside the
    /// boundary's region, not just the boundary itself — an `Opacity` nested
    /// three levels down is addressable without being a boundary of its own,
    /// which is the whole reason this design does not need to promote one. A
    /// node inside a NESTED boundary is addressed through that boundary's own
    /// capture.
    ///
    /// A `Vec`, not a map, and appended at each render id's FIRST appearance
    /// during `capture`'s pre-order walk: capture order equals paint order,
    /// so the patch arm in `layer_patches_for` resolves and rebuilds targets
    /// in the order a full repaint would — a stateful path clipper (a `Fn`
    /// that observes and mutates on every call) sees the same call sequence
    /// either way. A hash map's iteration order does not carry that
    /// guarantee. Looked up by linear scan rather than a side index: the
    /// number of effect-owning nodes per boundary is typically small (1-2
    /// patch targets per frame), and the `paint` benchmark's `layered`
    /// update arm at 1,000 nodes — the one that walks this list per patch
    /// attempt — is unchanged within measurement noise against a hash-map
    /// version of this same field. Revisit with a side `RenderId -> usize`
    /// index if a real tree grows this list long enough to change that.
    effect_slots: Vec<(RenderId, EffectSlots)>,
}

/// One entry of a capture's paint-order stream.
#[derive(Clone, Debug)]
enum Entry {
    /// A layer the boundary painted itself.
    Layer(RetainedNode),
    /// The place a nested repaint boundary sits.
    Slot(NestedSlot),
}

/// An ordered hole in a capture's entry stream, at the position the painting
/// walk reached a nested repaint boundary.
///
/// The slot owns the nested boundary's `OffsetLayer`: replay mints it from the
/// boundary's CURRENT position and content token, so a repainted nested
/// boundary always carries a new stamp for the damage differ and a boundary
/// that moved needs only the new offset. Everything here is relative to the
/// enclosing region, never an absolute anchor.
#[derive(Clone, Copy, Debug)]
struct NestedSlot {
    boundary: RenderId,
    /// The entry whose layer the boundary hung under, `None` for the region's
    /// own `OffsetLayer`.
    parent: Option<usize>,
    /// The enclosing node's accumulated origin in the region's layer space.
    origin: Offset,
    /// Where the parent painted the child through `paint_child_at` (a
    /// `FractionalTranslation`-style wrapper whose committed child offset stays
    /// zero), else `None` and the child's committed layout offset is read at
    /// replay. The override is part of the clean parent's own paint, so it is
    /// stored, not re-derived.
    offset_override: Option<Offset>,
    /// The anchor frame in force at the enclosing node when a paint anchor
    /// INSIDE this region governs it: that frame depends only on this
    /// region's own paint. `None` when the anchor came from outside, which
    /// replay takes from where the region is placed now.
    anchor: Option<AnchorFrame>,
}

/// One boundary's worth of composited-layer update work, decided during the
/// paint walk and applied by `run_paint` once the frame commits.
struct LayerPatch {
    /// `(index into RetainedSubtree::entries, replacement layer)`.
    patches: Vec<(usize, Layer)>,
    /// The nodes whose pending-update flag this patch serves.
    consumed: SmallVec<[RenderId; 2]>,
}

/// Where one render node's own effect layers live inside a [`RetainedSubtree`],
/// plus what is needed to rebuild them.
#[derive(Clone, Debug)]
struct EffectSlots {
    /// Indices into [`RetainedSubtree::entries`], in push order (outermost
    /// first) — the same order [`own_effect_layers`] returns.
    indices: SmallVec<[usize; 3]>,
    /// The accumulated origin this node painted at.
    ///
    /// Kept because a transform layer is conjugated by it. A property-only
    /// update cannot have moved the node — a move is a layout change, which
    /// repaints the boundary and discards this capture — so replaying the
    /// captured origin is correct by construction.
    origin: Offset,
}

#[derive(Clone, Debug)]
struct RetainedNode {
    layer: Layer,
    parent: Option<usize>,
}

/// The layers a node pushes for its OWN effects, in push order (outermost
/// first: opacity, clip, transform), built from the single
/// [`PaintEffects`] value the paint walk reads off the node.
///
/// One value in, two callers, deliberately: the paint walk pushes these
/// while painting, and the composited-layer-update patch arm rebuilds them
/// without painting. Two copies of this construction would drift, and the
/// drift would be a wrong-looking layer on a frame no test paints.
///
/// `origin` matters to the clip and the transform, not the opacity: both a
/// clip shape and a transform matrix are reported in the node's LOCAL
/// coordinates while every run inside the layer space carries the
/// accumulated origin, so both are conjugated by it ([`clip_layer`] for the
/// clip, [`conjugate`] for the transform). The opacity layer is
/// origin-independent (always `Offset::ZERO`), which is why an alpha-only
/// update is correct no matter where the node sits.
fn own_effect_layers(effects: PaintEffects, origin: Offset) -> SmallVec<[Layer; 3]> {
    let mut layers = SmallVec::new();
    if let Some(opacity) = effects.opacity {
        layers.push(Layer::Opacity(OpacityLayer::with_offset(
            opacity.alpha(),
            Offset::ZERO,
        )));
    }
    if let Some(clip) = effects.clip {
        layers.push(clip_layer(clip, origin));
    }
    if let Some(matrix) = effects.transform {
        // Same math and same reason as the per-child `PushTransform` fragment
        // op (RenderFlow and friends).
        layers.push(Layer::Transform(TransformLayer::new(conjugate(
            matrix, origin,
        ))));
    }
    layers
}

/// The nearest paint anchor as the walk carries it down: where the current
/// layer space's origin sits in the anchor's coordinates, and the anchor's
/// size.
///
/// A node at `origin` in the layer space is at `base + origin` from the
/// anchor. Inline children share the layer space and the frame; a repaint
/// boundary starts a layer space at its own origin, so its subtree gets the
/// frame [`rebased`](Self::rebased) there.
#[derive(Clone, Copy, Debug)]
struct AnchorFrame {
    base: Offset,
    size: Size,
    /// `true` while the anchor is a node of the repaint-boundary region being
    /// painted, `false` when the region inherited it from outside. A capture
    /// stores the frame only in the first case — see [`NestedSlot::anchor`].
    local: bool,
}

impl AnchorFrame {
    /// What a node painted at `origin` in the current layer space reads.
    fn at(self, origin: Offset) -> PaintAnchor {
        PaintAnchor {
            offset: self.base + origin,
            size: self.size,
        }
    }

    /// The frame for a layer space whose origin is at `origin` in this one.
    /// It starts a new region, so the anchor is no longer local to it.
    fn rebased(self, origin: Offset) -> Self {
        Self {
            base: self.base + origin,
            size: self.size,
            local: false,
        }
    }
}

/// How a nested boundary's `OffsetLayer` was placed this pass: the part of its
/// [`NestedSlot`] that only the painting walk knows. See
/// [`FragmentComposer::slot_info`].
#[derive(Clone, Copy, Debug)]
struct SlotPlacement {
    origin: Offset,
    offset_override: Option<Offset>,
    anchor: Option<AnchorFrame>,
}

/// The paint queue as one pass reads it.
struct PaintInputs<'a> {
    /// Every queued node: a boundary absent from it is clean.
    dirty_set: &'a FxHashSet<RenderId>,
    /// Boundaries queued only for composited-layer updates, with the nodes that
    /// asked.
    layer_updates: &'a FxHashMap<RenderId, SmallVec<[RenderId; 2]>>,
}

/// Builds the frame's [`LayerTree`] from replayed paint fragments,
/// merging adjacent inline draw runs into shared `PictureLayer`s.
///
/// Sealing discipline mirrors the recorder's: the open run is flushed
/// into a `PictureLayer` whenever a layer boundary needs ordering
/// (push/pop) and at [`Self::finish`]. The stack always holds at least
/// the root `OffsetLayer`.
#[derive(Debug)]
struct FragmentComposer {
    tree: LayerTree,
    stack: Vec<LayerId>,
    open: DisplayList,
    /// `(RenderId, LayerId)` correlation for each `Layer::Follower` pushed
    /// this paint pass (flui-rendering ARCHITECTURE.md, follower hit-testing) — the general `RenderId -> LayerId`
    /// primitive independently wanted by the snapshot/harness subtree-scoping
    /// TODOs (`testing/snapshot.rs`, `testing/harness.rs`), shipped narrowed
    /// to followers for now. `run_paint` resolves each entry post-paint via
    /// `flui_layer::resolve_follower_offset` into `PipelineOwner
    /// ::last_follower_offsets` / `::last_hidden_follower_ids`.
    follower_correlations: Vec<(RenderId, LayerId)>,
    /// Retention decisions taken during the walk, applied by `run_paint` once
    /// the walk's `&self` borrow ends.
    ///
    /// `Some` replaces the boundary's stored output, `None` evicts it. The
    /// walk itself only has `&self`, so this is the same side-collection shape
    /// the layout walk uses for the sinks it cannot commit in place.
    retained_captures: Vec<(RenderId, Option<RetainedSubtree>)>,
    /// What each nested boundary's `OffsetLayer` was placed with, so `capture`
    /// can turn the layer back into a [`NestedSlot`] of the enclosing capture.
    slot_info: FxHashMap<LayerId, SlotPlacement>,
    /// Which render node owns each effect layer pushed this pass, and the
    /// origin it painted at.
    ///
    /// Maintained incrementally rather than as a log the captures re-index:
    /// `capture` runs once per repainted boundary, so rebuilding this map
    /// inside it would make the paint pass O(boundaries x effect layers) even
    /// though each capture only looks up the layers it actually visits.
    effect_owner: FxHashMap<LayerId, (RenderId, Offset)>,
    /// Patches applied to a grafted capture this pass, to be written back into
    /// the stored capture by `run_paint` once the walk's `&self` borrow ends.
    ///
    /// Without the write-back the cache would keep the value it was captured
    /// with, and a later frame that grafts it for an unrelated reason would
    /// revert the property permanently — the failure a two-frame test cannot
    /// see.
    layer_patches: Vec<(RenderId, ContentToken, Vec<(usize, Layer)>)>,
    /// Nodes whose pending-update flag this pass's patches serve, cleared by
    /// `run_paint` on the commit path only — see [`LayerPatch::consumed`].
    consumed_updates: Vec<RenderId>,
    /// Every render node this pass's descent actually reached.
    ///
    /// The residue scan needs "was this queued node reached", and the only
    /// honest answer is one the walk records. Its previous test — is the node
    /// STILL flagged needs-paint — is blind after a pass that failed partway,
    /// because the walk clears that flag as it goes while `run_paint`'s error
    /// arm keeps the queue for the retry. The scan would then skip exactly the
    /// entries it exists to catch, silently, since it is also the only
    /// diagnostic on that path.
    visited: FxHashSet<RenderId>,
    /// Nodes painted and layers created or grafted this pass, folded into the
    /// owner's `PipelineCounters` by `run_paint` on the commit path only.
    counts: PaintCounts,
}

impl FragmentComposer {
    /// `device_pixel_ratio` becomes the root layer's scale: the
    /// framework paints in LOGICAL pixels, the engine rasterizes in
    /// physical surface pixels — the root transform is the single
    /// place the two meet.
    ///
    /// `root_boundary` stamps the root layer when the root render object
    /// declares itself a repaint boundary (`RenderView` does). It has to be
    /// threaded in here rather than applied later, because the root is never
    /// reached by `push_boundary_layer`: that fires from the PARENT's child
    /// loop, and the root has no parent. Without it the tree carries a
    /// boundary nothing can identify -- the worst shape for anything pairing
    /// boundaries across frames, which is what the stamp exists for. It
    /// carries the root's content token, certified by `run_paint`.
    fn new(device_pixel_ratio: f64, root_boundary: Option<(RenderId, ContentToken)>) -> Self {
        let root_layer = if (device_pixel_ratio - 1.0).abs() < f64::EPSILON {
            Layer::Offset(OffsetLayer::zero())
        } else {
            Layer::Transform(TransformLayer::new(
                flui_foundation::geometry::Matrix4::scaling(
                    device_pixel_ratio,
                    device_pixel_ratio,
                    1.0,
                ),
            ))
        };
        let root_node = LayerNode::new(root_layer);
        let tree = LayerTree::new(match root_boundary {
            Some((id, content)) => root_node.with_boundary(id, content),
            None => root_node,
        });
        Self {
            stack: vec![tree.root()],
            tree,
            open: DisplayList::new(),
            follower_correlations: Vec::new(),
            retained_captures: Vec::new(),
            slot_info: FxHashMap::default(),
            effect_owner: FxHashMap::default(),
            layer_patches: Vec::new(),
            consumed_updates: Vec::new(),
            visited: FxHashSet::default(),
            counts: PaintCounts::default(),
        }
    }

    /// Counts one render node whose `paint_raw` ran this pass.
    fn note_painted(&mut self) {
        self.counts.nodes_painted += 1;
    }

    /// Merges a sealed run while keeping its canvas clips local to that run.
    fn append_run(&mut self, run: DisplayList) {
        self.open.append_isolated(run);
    }

    /// Flushes the open picture into a `PictureLayer` under the
    /// current stack top (no-op when empty).
    /// The layer everything recorded right now hangs off.
    ///
    /// One accessor rather than the same `stack.last().expect(..)` repeated at
    /// every insertion point: the invariant has one owner, and the message
    /// naming it is written once.
    fn current_parent(&self) -> LayerId {
        *self.stack.last().expect(
            "BUG: the composer stack always holds the root layer — pop_layer refuses to remove it",
        )
    }

    fn seal_picture(&mut self) {
        if self.open.is_empty() {
            return;
        }
        let list = std::mem::take(&mut self.open);
        let parent = self.current_parent();
        let _ = self
            .tree
            .push_child(parent, Layer::from(PictureLayer::new(list)));
        self.counts.layers_produced += 1;
    }

    /// Inserts `layer` under the current stack top, returning its freshly
    /// minted `LayerId` — the caller (`paint_subtree_impl`'s fragment-op
    /// replay loop) uses this to record the `RenderId -> LayerId`
    /// correlation for `Layer::Follower` pushes (flui-rendering ARCHITECTURE.md, follower hit-testing).
    fn push_layer(&mut self, layer: Layer) -> LayerId {
        self.push_layer_node(LayerNode::new(layer))
    }

    /// Push the layer a repaint boundary hangs under, stamped with that
    /// boundary's id.
    ///
    /// The stamp is what lets two consecutive frames be compared: every frame
    /// builds a fresh `LayerTree` with fresh slab indices, so `LayerId` pairs
    /// nothing, and damage has to come from comparing layer trees rather than
    /// from which render objects repainted (ADR-0061 — the ones that always
    /// repaint cover the screen). The other half of that comparison — "is
    /// this content unchanged" — is `content`, the token the paint walk
    /// certified for this boundary. Picture `Arc`s cannot answer it: an
    /// enclosing boundary re-records its inline pictures whenever something
    /// nested in it is dirty.
    ///
    /// **Exactly one layer per boundary carries the stamp**, and it is this
    /// one: the `OffsetLayer` a boundary child rebases under, which is also
    /// the node a retained subtree is grafted beneath. Stamping a node's
    /// effect layers or a fragment's structural pushes as well would put
    /// several layers under one id and make pairing ambiguous — so those go
    /// through [`Self::push_layer`] and stay unstamped.
    fn push_boundary_layer(
        &mut self,
        layer: Layer,
        boundary_id: RenderId,
        content: ContentToken,
    ) -> LayerId {
        self.push_layer_node(LayerNode::new(layer).with_boundary(boundary_id, content))
    }

    fn push_layer_node(&mut self, node: LayerNode) -> LayerId {
        self.seal_picture();
        let parent = self.current_parent();
        let id = self.tree.push_child(parent, node);
        self.stack.push(id);
        self.counts.layers_produced += 1;
        id
    }

    /// Records that `layer_id` is one of `render_id`'s own effect layers.
    ///
    /// Flat and unconditional: which capture (if any) ends up owning this
    /// layer is not known until [`Self::capture`] runs, and a node's effect
    /// layers can be flattened into SEVERAL captures at once when boundaries
    /// nest, so the resolution has to happen per capture rather than here.
    fn record_effect_layer(&mut self, render_id: RenderId, layer_id: LayerId, origin: Offset) {
        self.effect_owner.insert(layer_id, (render_id, origin));
    }

    /// Records a `(RenderId, LayerId)` correlation for a pushed
    /// `Layer::Follower` node (flui-rendering ARCHITECTURE.md, follower hit-testing).
    fn record_follower_correlation(&mut self, render_id: RenderId, follower_layer_id: LayerId) {
        self.follower_correlations
            .push((render_id, follower_layer_id));
    }

    /// Flatten everything painted under `root` into a reusable
    /// [`RetainedSubtree`].
    ///
    /// Called after the boundary's own paint finished and its trailing picture
    /// run was sealed, so the tree under `root` is complete.
    ///
    /// Returns `None` when the subtree contains a `Leader` or `Follower`.
    /// The tree re-indexes a grafted leader by itself, but a follower also
    /// needs the `RenderId` correlation (`follower_correlations`) that the
    /// flattened form does not carry, so its hit-test offset would go stale
    /// on every reused frame. Refusing to retain either is the bounded
    /// answer; carrying the correlation through the capture is the complete
    /// one.
    ///
    /// A nested repaint boundary's `OffsetLayer` becomes a [`NestedSlot`] and its
    /// content is not copied: the nested boundary keeps its own capture.
    fn capture(&self, root: LayerId, content: ContentToken) -> Option<RetainedSubtree> {
        let mut entries: Vec<Entry> = Vec::new();
        // `effect_owner` is maintained incrementally by `record_effect_layer`,
        // so this walk answers "is this layer somebody's effect layer" in O(1)
        // without re-indexing anything per capture.
        //
        // Appended at a render id's FIRST appearance below, so this list ends
        // up in the same order the pre-order walk below visits render ids —
        // see the field doc on `RetainedSubtree::effect_slots`.
        let mut effect_slots: Vec<(RenderId, EffectSlots)> = Vec::new();

        // (tree id, parent index in `entries`)
        let mut stack: Vec<(LayerId, Option<usize>)> = self
            .tree
            .children(root)?
            .iter()
            .rev()
            .map(|&id| (id, None))
            .collect();

        while let Some((id, parent)) = stack.pop() {
            let node = self.tree.get(id)?;
            let index = entries.len();
            if let Some(stamp) = node.boundary() {
                // Only `push_boundary_layer` stamps a layer, and it records the
                // placement in the same call.
                let placement = self.slot_info.get(&id)?;
                entries.push(Entry::Slot(NestedSlot {
                    boundary: stamp.render_id(),
                    parent,
                    origin: placement.origin,
                    offset_override: placement.offset_override,
                    anchor: placement.anchor,
                }));
                continue;
            }
            let layer = node.layer();
            if layer.as_leader().is_some() || layer.as_follower().is_some() {
                return None;
            }
            if let Some(&(render_id, origin)) = self.effect_owner.get(&id) {
                // Find-or-push, not `entry().or_insert_with()`: this is a
                // `Vec` so the first appearance of `render_id` fixes its
                // position, which is what keeps `effect_slots` capture-
                // ordered. A short linear scan — see the field doc.
                if let Some((_, slots)) = effect_slots
                    .iter_mut()
                    .find(|(owner, _)| *owner == render_id)
                {
                    slots.indices.push(index);
                } else {
                    let mut indices = SmallVec::new();
                    indices.push(index);
                    effect_slots.push((render_id, EffectSlots { indices, origin }));
                }
            }
            entries.push(Entry::Layer(RetainedNode {
                layer: layer.clone(),
                parent,
            }));
            for &child in node.children().iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        Some(RetainedSubtree {
            entries,
            content,
            effect_slots,
        })
    }

    /// Makes `id` the layer everything recorded next hangs under, until
    /// [`Self::pop_layer`]. Replay uses it to place a nested boundary under the
    /// layer its slot named.
    fn adopt_parent(&mut self, id: LayerId) {
        self.seal_picture();
        self.stack.push(id);
    }

    fn pop_layer(&mut self) {
        self.seal_picture();
        debug_assert!(
            self.stack.len() > 1,
            "composer pop without matching push — fragment scope ops are \
             balanced by the recorder, so an underflow means the replay \
             loop pushed/popped asymmetrically",
        );
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    fn finish(
        mut self,
    ) -> (
        LayerTree,
        Vec<(RenderId, LayerId)>,
        Vec<(RenderId, Option<RetainedSubtree>)>,
        Vec<(RenderId, ContentToken, Vec<(usize, Layer)>)>,
        Vec<RenderId>,
        FxHashSet<RenderId>,
        PaintCounts,
    ) {
        self.seal_picture();
        debug_assert_eq!(
            self.stack.len(),
            1,
            "composer finished with unbalanced layer stack — every \
             push_layer in the replay loop must have a matching pop_layer",
        );
        (
            self.tree,
            self.follower_correlations,
            self.retained_captures,
            self.layer_patches,
            self.consumed_updates,
            self.visited,
            self.counts,
        )
    }
}

/// Conjugates `matrix` so it pivots around this layer's local `origin`
/// rather than the layer tree's own (0, 0).
///
/// Both callers report a transform in LOCAL coordinates while every run
/// they bracket carries the accumulated `origin` baked into its canvas
/// transform: the per-node [`RenderObject::paint_effects`](crate::traits::RenderObject::paint_effects)
/// value's `transform` field (one transform for the whole node, applied
/// here) and the per-child [`FragmentOp::PushTransform`] op (`RenderFlow`
/// and any other Variable-arity node giving each child its own paint-time
/// transform). The result is `T(offset)·M·T(−offset)`.
fn conjugate(
    matrix: flui_foundation::geometry::Matrix4,
    origin: Offset,
) -> flui_foundation::geometry::Matrix4 {
    if origin == Offset::ZERO {
        matrix
    } else {
        let (dx, dy) = (origin.dx, origin.dy);
        flui_foundation::geometry::Matrix4::translation(dx, dy, 0.0)
            * matrix
            * flui_foundation::geometry::Matrix4::translation(-dx, -dy, 0.0)
    }
}

/// Maps a recorded effect-layer scope onto its `flui-layer` layer.
///
/// Scope shapes/bounds are recorded in the node's LOCAL coordinates, while
/// the runs they bracket carry the accumulated `origin` baked into their
/// canvas transforms — so every variant is shifted by `origin` here
/// (a clip rect is `clipRect.shift(offset)`; a shader mask's rect and a
/// backdrop filter's bounds follow the same `offset & size` convention),
/// or a scope away from the
/// parent origin would apply at the layer's (0,0) instead of the node's
/// position.
///
/// Always a real layer today; lowering non-composited clips back into
/// canvas clips inside the merged picture is a composer-side optimization
/// gated on the `needs_compositing` bits — correctness is identical
/// either way, so the recording API does not expose the choice.
fn scope_layer(scope: FragmentScope, origin: Offset) -> Layer {
    match scope {
        FragmentScope::Opacity(opacity) => {
            Layer::Opacity(OpacityLayer::with_offset(opacity.alpha(), Offset::ZERO))
        }
        FragmentScope::Clip(clip) => clip_layer(clip, origin),
        FragmentScope::ShaderMask {
            shader,
            blend_mode,
            bounds,
        } => Layer::ShaderMask(ShaderMaskLayer::new(
            shader,
            blend_mode,
            bounds.translate_offset(origin),
        )),
        FragmentScope::BackdropFilter {
            filter,
            blend_mode,
            bounds,
            group,
        } => Layer::BackdropFilter(
            BackdropFilterLayer::new(filter, blend_mode, bounds.translate_offset(origin))
                .with_group(group),
        ),
        FragmentScope::AnnotatedRegion { value, bounds } => Layer::AnnotatedRegion(
            AnnotatedRegionLayer::from_value(bounds.translate_offset(origin), value),
        ),
        // Oracle `LeaderLayer(link: link, offset: offset)` — `offset` is
        // this node's own accumulated position, exactly what `origin`
        // already is at this call site (`:270`). Unlike the clip/mask
        // variants above, `size` is NOT shifted by `origin` — it is a
        // pure dimension, not a position.
        FragmentScope::Leader { link, size } => {
            Layer::Leader(LeaderLayer::with_offset(link, size, origin))
        }
        // `Layer::Follower` carries no resolved position at all — the
        // linked/unlinked offsets are inputs to a LATER resolution pass, never
        // stored as the final on-screen transform. `target_offset` is
        // recorded as-authored (not origin-shifted): resolving it against
        // the leader's position is deliberately deferred past this pass
        // (design research plan — a `flui-engine`/`flui-layer`
        // follow-up, not performed here).
        FragmentScope::Follower {
            link,
            size,
            target_offset,
            show_when_unlinked,
            leader_anchor,
            follower_anchor,
        } => Layer::Follower(
            FollowerLayer::new(link)
                .with_size(size)
                .with_target_offset(target_offset)
                .with_show_when_unlinked(show_when_unlinked)
                .with_leader_anchor(leader_anchor)
                .with_follower_anchor(follower_anchor),
        ),
    }
}

/// Maps a recorded [`PaintClip`] onto its `flui-layer` clip layer.
///
/// Shifted by `origin` for the same reason every [`scope_layer`] variant
/// is (see its doc): `Rect`/`RRect` translate their shape directly.
/// `Path` is the one case worth spelling out — at `origin == Offset::ZERO`
/// (the node sits at the layer-space origin, directly under its boundary)
/// the already-owned `Arc<Path>` moves through untouched, no clone and no
/// copy of the command buffer; only a non-zero origin pays for a
/// translated path in a freshly allocated `Arc`. `PathTarget` resolves
/// through [`resolve_path_clip`] first (never by the producer, and inside
/// the walk that calls this function), then follows the same
/// translate-then-box shape as `Path` — the one difference is that its
/// `Arc::new` is unavoidable either way, since the resolver always hands
/// back a freshly built owned `Path`, not a pre-existing `Arc`.
fn clip_layer(clip: PaintClip, origin: Offset) -> Layer {
    match clip {
        PaintClip::Rect { rect, behavior } => {
            Layer::ClipRect(ClipRectLayer::new(rect.translate_offset(origin), behavior))
        }
        PaintClip::RRect { rrect, behavior } => Layer::ClipRRect(ClipRRectLayer::new(
            rrect.translate_offset(origin),
            behavior,
        )),
        PaintClip::RSuperellipse {
            rsuperellipse,
            behavior,
        } => Layer::ClipSuperellipse(ClipSuperellipseLayer::new(
            rsuperellipse.translate_offset(origin),
            behavior,
        )),
        PaintClip::Path { path, behavior } => {
            let path = if origin == Offset::ZERO {
                path
            } else {
                Arc::new(path.translate(origin))
            };
            Layer::ClipPath(ClipPathLayer::new(path, behavior))
        }
        PaintClip::PathTarget {
            target,
            size,
            behavior,
        } => {
            // The path arm above shares its already-owned `Arc<Path>` for
            // free at zero origin; there is no pre-existing `Arc` to share
            // here — `resolve_path_clip` hands back a freshly built owned
            // `Path` every time, so the single `Arc::new` below is the one
            // allocation this arm ever pays, made AFTER the translate so a
            // non-zero origin never boxes a path it is about to discard.
            let path = resolve_path_clip(target, size);
            let path = if origin == Offset::ZERO {
                path
            } else {
                path.translate(origin)
            };
            Layer::ClipPath(ClipPathLayer::new(Arc::new(path), behavior))
        }
    }
}

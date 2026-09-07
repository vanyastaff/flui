//! Paint phase implementation for `PipelineOwner<PaintPhase>`.

use flui_foundation::{LayerId, RenderId};
use flui_layer::{
    BackdropFilterLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer, FollowerLayer, Layer,
    LayerNode, LayerTree, LeaderLayer, LinkRegistry, OffsetLayer, OpacityLayer, PictureLayer,
    ShaderMaskLayer, TransformLayer,
};
use flui_painting::DisplayList;
use flui_types::Offset;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{
    context::{FragmentOp, FragmentRecorder, FragmentScope},
    pipeline::{
        phase::{Idle, PaintPhase, Semantics},
        scheduler::PhaseKind,
    },
};

use super::{PipelineOwner, rebind_phase, subtree_arena::ensure_stack};

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
    pub fn run_paint(&mut self) -> crate::error::RenderResult<()> {
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

        // Deepest-first ordering retained (Flutter `flushPaint`): the
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
            // repaint after its layer-update mark drops out here, matching
            // Flutter's `if (node._needsPaint) repaintCompositedChild(node)
            // else updateLayerProperties(node)`.
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

            let mut composer = FragmentComposer::new(self.device_pixel_ratio, root_boundary);
            match self.paint_subtree(
                &mut composer,
                root_id,
                Offset::ZERO,
                &dirty_ids,
                &layer_updates,
            ) {
                Ok(()) => {
                    let (
                        layer_tree,
                        link_registry,
                        follower_correlations,
                        retained_captures,
                        layer_patches,
                        consumed_updates,
                        visited,
                    ) = composer.finish();
                    reached = visited;
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
                        layer_patches.iter().map(|(id, _)| *id).collect();
                    for (boundary_id, patches) in layer_patches {
                        let Some(subtree) = self.retained_boundaries.get_mut(&boundary_id) else {
                            continue;
                        };
                        for (index, layer) in patches {
                            if let Some(node) = subtree.nodes.get_mut(index) {
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
                        // …and every capture that EMBEDS one of them. An
                        // enclosing boundary's capture flattens the inner
                        // one's layers, so evicting only the inner leaves the
                        // outer replaying the same stale output — and once the
                        // queue clears, the graft-time `nested_boundaries`
                        // check no longer sees the inner as dirty, so nothing
                        // stops it. Reachable when the enclosing boundary was
                        // ALSO out of reach that frame (both under a
                        // suppressed ancestor), which is why evicting the
                        // inner alone is not enough.
                        //
                        // Exactly what the residue scan below does for the
                        // paint case, for the same reason.
                        self.retained_boundaries.retain(|id, subtree| {
                            !unserved.contains(id)
                                && !unserved
                                    .iter()
                                    .any(|inner| subtree.nested_boundaries.contains(inner))
                        });
                    }

                    // ADR-0015: resolve each paint-phase-correlated
                    // follower's composite-resolved offset against the SAME
                    // fully-built layer_tree/link_registry the GPU path
                    // (flui-engine's `render_layer_recursive`) resolves
                    // against, reusing the identical `resolve_follower_offset`
                    // — one algorithm, two consumers (pixels + hit-test), not
                    // two copies of the logic. Runs before these values are
                    // handed to `last_layer_tree`/`last_link_registry` (and
                    // eventually taken by the binding via
                    // `take_link_registry()`).
                    let mut follower_offsets = FxHashMap::default();
                    let mut hidden_follower_ids = FxHashSet::default();
                    for (render_id, follower_layer_id) in follower_correlations {
                        let Some(follower) = layer_tree
                            .get_layer(follower_layer_id)
                            .and_then(Layer::as_follower)
                        else {
                            continue;
                        };
                        match flui_layer::resolve_follower_offset(
                            &layer_tree,
                            &link_registry,
                            follower_layer_id,
                            follower,
                        ) {
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

                    self.last_layer_tree = Some(layer_tree);
                    self.last_link_registry = Some(link_registry);
                }
                Err(e) => {
                    // Restore the debug invariant before propagating so
                    // the owner stays consistent on the error path.
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
                // …and every capture that EMBEDS it. `mark_needs_paint` stops
                // at the nearest established boundary, so an enclosing one is
                // clean by construction, and its capture replays this
                // boundary's old layers wholesale. The graft-time
                // `nested_boundaries` check that normally prevents that reuse
                // is keyed on the inner boundary still being dirty — which the
                // clear above has just undone, so the check can no longer
                // fire and the eviction has to stand in for it.
                self.retained_boundaries
                    .retain(|_, subtree| !subtree.nested_boundaries.contains(&dirty_node.id));
            }
        }
        // `clear()` retains capacity (preserve Vec backing across frames).
        self.scheduler.clear_paint_queue();

        // exit_phase clears debug_doing_paint AND drains mid-paint marks back
        // into dirty so paint marks made during this pass become next-frame
        // work rather than being stranded — Flutter's flushPaint semantics.
        //
        // Finding 2 (intentional improvement over pre-refactor behavior):
        // exit_phase also drains mid-marks on the ERROR path (the early-return
        // above calls exit_phase before returning Err). Pre-refactor, the error
        // path only cleared debug_doing_paint and did NOT drain mid-marks, so
        // any mark made between enter_phase and the error was silently lost.
        // The always-drain contract of exit_phase is the correct behavior:
        // mid-paint marks scheduled before the error survive into the next
        // frame's retry rather than being dropped.
        let _ = self.scheduler.exit_phase(PhaseKind::Paint);

        Ok(())
    }

    /// Records one node's paint fragment and replays it into the
    /// composer, recursing at child markers.
    ///
    /// Per-node order follows Flutter's `PaintingContext._paintWithContext`:
    /// `WAS_REPAINT_BOUNDARY` is written and `NEEDS_PAINT` cleared
    /// **before** the node paints, so a paint body that re-marks its own
    /// node is caught by the debug check below instead of silently
    /// erasing the evidence.
    fn paint_subtree(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
        dirty_set: &FxHashSet<RenderId>,
        layer_updates: &FxHashMap<RenderId, SmallVec<[RenderId; 2]>>,
    ) -> crate::error::RenderResult<()> {
        ensure_stack(|| {
            self.paint_subtree_impl(composer, node_id, origin, dirty_set, layer_updates)
        })
    }

    /// The effect-layer replacements a grafted `subtree` needs this frame, or
    /// `None` when one of them cannot be expressed as a patch.
    ///
    /// Walks the nodes whose own effect layers this capture holds and rebuilds
    /// those of them that asked for a composited-layer update, through the
    /// same [`own_effect_layers`] the paint walk uses. Everything else in the
    /// capture is replayed untouched.
    ///
    /// Returns `None` — meaning "repaint instead" — when a node's effect
    /// layers no longer have the same SHAPE they were captured with: a
    /// different count, or a different layer kind at the same position. Both
    /// are structural changes a patch cannot express (an alpha crossing out of
    /// the layered range drops its layer entirely), and both are correctly
    /// served by a repaint. Comparing the discriminant per position rather
    /// than just the count is what catches the case where one effect appears
    /// as another disappears and the count is unchanged.
    fn layer_patches_for(
        &self,
        subtree: &RetainedSubtree,
        targets: &[RenderId],
    ) -> Option<LayerPatch> {
        // A node that asked for an update this frame but owns no slot in this
        // capture cannot be served by a patch: its effect layers did not exist
        // when the capture was taken, so there is nothing to replace and the
        // loop below would never even look at it. That is a shape change, and
        // a repaint is the only thing that can express it.
        //
        // Latent today — every shipped caller reports a repaint itself when its
        // effect layers appear or disappear — but the loop below walks EXISTING
        // slots, so without this the next caller to get that wrong would graft
        // stale output silently instead of failing loudly.
        if targets
            .iter()
            .any(|target| !subtree.effect_slots.contains_key(target))
        {
            return None;
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
        for (&render_id, slots) in &subtree.effect_slots {
            let Some(node) = self.render_tree.get(render_id) else {
                // The node is gone but its layers are still in the capture.
                // Nothing asked for an update, so replaying them is what a
                // clean frame would do anyway.
                continue;
            };
            if !node.needs_composited_layer_update() {
                continue;
            }
            let fresh = own_effect_layers(
                node.paint_alpha(),
                node.paint_layer_blend(),
                node.paint_transform(),
                slots.origin,
            );
            if fresh.len() != slots.indices.len() {
                return None;
            }
            for (&index, layer) in slots.indices.iter().zip(fresh) {
                // Indices come from this same capture, so they are in range by
                // construction — but this runs inside the paint walk, where an
                // out-of-range index would panic a frame rather than lose one
                // reuse. Refusing degrades to a repaint, which is always
                // correct.
                let Some(captured) = subtree.nodes.get(index) else {
                    debug_assert!(false, "BUG: effect slot index outside its own capture");
                    return None;
                };
                // Defence in depth, and deliberately not claimed as more than
                // that: `own_effect_layers` emits a FIXED order (alpha, then
                // transform), so a same-count shape change maps positionally
                // onto the same kinds and patching it happens to produce what a
                // repaint would. A mutation run confirms no test distinguishes
                // this branch today. It earns its place by making that
                // coincidence explicit rather than load-bearing: adding a third
                // effect type, or making the order conditional, would otherwise
                // silently turn a positional patch into a wrong layer.
                if std::mem::discriminant(&layer) != std::mem::discriminant(&captured.layer) {
                    return None;
                }
                patches.push((index, layer));
            }
            consumed.push(render_id);
        }
        Some(LayerPatch { patches, consumed })
    }

    /// Body of [`Self::paint_subtree`]; split out so every recursion
    /// level enters through the [`ensure_stack`] probe.
    fn paint_subtree_impl(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
        dirty_set: &FxHashSet<RenderId>,
        layer_updates: &FxHashMap<RenderId, SmallVec<[RenderId; 2]>>,
    ) -> crate::error::RenderResult<()> {
        let Some(render_node) = self.render_tree.get(node_id) else {
            return Ok(());
        };
        // Reached, whatever happens below: the early returns for skip-paint,
        // pending layout and invisible slivers are all decisions the descent
        // MADE about this node, not evidence it never got here.
        composer.visited.insert(node_id);

        let is_repaint_boundary = render_node.is_repaint_boundary();

        let alpha = render_node.paint_alpha();
        let layer_blend = render_node.paint_layer_blend();
        let transform = render_node.paint_transform();
        let child_ids: Vec<RenderId> = render_node.children().to_vec();
        // The generation this node's most recent layout stamped onto the
        // children it laid out; a child carrying anything else was not part of
        // that pass and is skipped at the splice below.
        let parent_generation = render_node.layout_generation();

        // Written unconditionally PRE-paint (Flutter object.dart:3560):
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
        // Recorded before the early returns below for the same reason Flutter
        // clears it outside `_paintWithContext`: a node skipped for layout must
        // not strand the request either.
        if render_node.needs_composited_layer_update() {
            composer.consumed_updates.push(node_id);
        }

        // Fully transparent subtree: skip recording entirely. Children
        // keep whatever dirty flags they carry; the residue scan in
        // run_paint clears them with a warning.
        // Uses `skip_paint()` rather than `alpha == Some(0)` so that
        // `paint_alpha()` encoding only controls layer-emission; the
        // skip-paint decision is a separate, explicit contract
        // (Flutter: `if (_alpha == 0) return;` in RenderOpacity.paint).
        if render_node.skip_paint() {
            return Ok(());
        }

        // Flutter object.dart:3497 — a node that still needs layout must
        // not paint stale geometry. Layout runs before paint in the
        // pipeline, so this guards descendant-error and partial-frame
        // paths where a poisoned layout left the flag set.
        if render_node.needs_layout() {
            return Ok(());
        }

        // Sliver visibility cull: a sliver with zero paint extent
        // (`!visible`) paints nothing and splices no children (Flutter:
        // the viewport skips invisible slivers). The gate lives here, in
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
        let debug_name = render_node.debug_name();
        let mut recorder = FragmentRecorder::new(origin, self.device_pixel_ratio);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_node.paint_raw(&mut recorder, child_ids.len());
        }))
        .map_err(|_| crate::error::RenderError::poisoned(debug_name, "paint"))?;
        let fragment = recorder.finish();

        debug_assert!(
            !render_node.needs_paint(),
            "paint-must-not-redirty: a render object marked ITSELF \
             needs-paint during its own paint; derive visual changes \
             from state read at paint time instead of re-marking",
        );

        // Effect hooks wrap the ENTIRE node fragment (self draws AND
        // children). The pre-fragment walk wrapped children only; hook
        // implementors draw nothing themselves, so the visible result
        // is identical and the new rule matches Flutter (RenderOpacity
        // wraps its child's whole paint).
        // Alpha–blend coupling: the layer is emitted only when `paint_alpha()` returns
        // `Some`.  A render object that overrides `paint_layer_blend() -> Some(mode)`
        // but leaves `paint_alpha() -> None` will silently drop the blend layer —
        // the advanced compositor never sees it.  When wiring the first render-tree
        // consumer of an advanced blend, override BOTH hooks and return `Some(255)`
        // from `paint_alpha()` for an opaque-blend-only layer.
        let own_effects = own_effect_layers(alpha, layer_blend, transform, origin);
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
                    // ADR-0015: capture the RenderId -> LayerId
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
                    let child_is_boundary = child_node.is_repaint_boundary();

                    if child_is_boundary {
                        // Boundary children rebase to ZERO under their
                        // own OffsetLayer so a future offset-only move
                        // is a layer-property update, not a repaint.
                        let boundary_root = composer.push_boundary_layer(
                            Layer::Offset(OffsetLayer::new(origin + child_offset)),
                            child_id,
                        );

                        // Reuse the previous frame's output when this boundary
                        // is clean. Sound only because the queue is exact:
                        // `run_layout` marks every boundary layout touched and
                        // `mark_needs_paint` marks the rest, so absence from
                        // `dirty_set` genuinely means unchanged content.
                        // Every boundary is recorded into the captures open
                        // above it, so each learns what is nested beneath.
                        composer.note_boundary(child_id);

                        // Three dispositions, read straight off the queue:
                        // queued for a repaint, queued only for a
                        // composited-layer update, or not queued at all. The
                        // last two both reuse the retained output; only the
                        // first descends.
                        let update_targets = layer_updates.get(&child_id);
                        let queued_for_repaint =
                            update_targets.is_none() && dirty_set.contains(&child_id);
                        let retained = (!queued_for_repaint)
                            .then(|| self.retained_boundaries.get(&child_id))
                            .flatten()
                            // A cached subtree replays its nested boundaries'
                            // layers too, so it is reusable only while none of
                            // them has pending work of ANY kind — see
                            // `nested_boundaries`.
                            //
                            // A layer update counts. It is tempting not to let
                            // it, since a nested boundary's effect layers are
                            // flattened into this capture and the patch below
                            // could reach them — but serving the update from
                            // here patches only THIS capture and clears the
                            // flag, while the nested boundary keeps its own
                            // capture at the old value. The next frame that
                            // repaints this boundary descends, finds the nested
                            // one clean, and grafts that stale capture: the
                            // property silently reverts, permanently. Declining
                            // sends the walk down to the nested boundary, which
                            // patches its own capture, and this one re-captures
                            // the result on the way back out.
                            .filter(|subtree| {
                                !subtree
                                    .nested_boundaries
                                    .iter()
                                    .any(|nested| dirty_set.contains(nested))
                            });
                        // `None` here means the structure changed under a
                        // layer-update request (a node gained or lost an effect
                        // layer), which a patch cannot express — fall back to a
                        // full repaint, which rebuilds it correctly.
                        let patch = retained.and_then(|subtree| {
                            self.layer_patches_for(
                                subtree,
                                update_targets.map_or(&[][..], |targets| targets),
                            )
                        });
                        if let (Some(subtree), Some(patch)) = (retained, patch) {
                            let LayerPatch { patches, consumed } = patch;
                            if !patches.is_empty() {
                                tracing::trace!(
                                    boundary = ?child_id,
                                    layers = patches.len(),
                                    "paint: composited-layer update, subtree replayed"
                                );
                            }
                            // Reused rather than descended into, but reached.
                            composer.visited.insert(child_id);
                            // A graft clones this boundary's whole flattened
                            // output in, INCLUDING the layers of every boundary
                            // nested beneath it — but the walk does not descend,
                            // so `note_boundary` never fires for any of them and
                            // the enclosing capture would embed boundaries it
                            // does not name. That is not a bookkeeping nicety:
                            // the graft refusal and both evictions are keyed on
                            // this list, so an unnamed grandchild can go dirty
                            // while its enclosing capture is reused, replaying
                            // stale layers that nothing then repaints.
                            //
                            // Re-noting the capture's own list is what makes the
                            // relation transitive. Two levels of nesting always
                            // repaint fresh and hide the need for it.
                            for &nested in &subtree.nested_boundaries {
                                composer.note_boundary(nested);
                            }
                            composer.graft(subtree, &patches);
                            composer.consumed_updates.extend(consumed);
                            // Pushed even when empty: this doubles as the
                            // record that the frame SERVED this boundary, which
                            // `run_paint` uses to tell a boundary it reused
                            // from one the walk never reached. The write-back
                            // loop treats an empty patch list as a no-op.
                            //
                            // The stored capture has to move with the frame.
                            // Patching only the emitted tree would leave the
                            // cache at the old value, and a later frame that
                            // grafts it for an unrelated reason would silently
                            // revert the property for good.
                            composer.layer_patches.push((child_id, patches));
                        } else {
                            composer.open_capture();
                            // The result is held rather than propagated with
                            // `?` so the scope closes on the error path too.
                            // Today an errored paint abandons the whole frame
                            // and the composer with it, but an unbalanced
                            // stack would mis-assign nested lists rather than
                            // fail loudly the moment paint errors become
                            // recoverable.
                            let painted = self.paint_subtree(
                                composer,
                                child_id,
                                Offset::ZERO,
                                dirty_set,
                                layer_updates,
                            );
                            // Seal before capturing: the boundary's
                            // trailing run is part of its output, and
                            // `pop_layer` would otherwise flush it after
                            // the snapshot was taken.
                            composer.seal_picture();
                            let nested_boundaries = composer.close_capture();
                            painted?;
                            // `None` evicts: a subtree that GAINED a
                            // Leader/Follower must not be served its
                            // pre-link form. See `capture`.
                            let captured = composer.capture(boundary_root).map(|mut subtree| {
                                subtree.nested_boundaries = nested_boundaries;
                                subtree
                            });
                            composer.retained_captures.push((child_id, captured));
                        }
                        composer.pop_layer();
                    } else {
                        // Inline children bake into the shared picture
                        // space — runs merge, no extra layer.
                        self.paint_subtree(
                            composer,
                            child_id,
                            origin + child_offset,
                            dirty_set,
                            layer_updates,
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
}

// ============================================================================
// Fragment composition (paint phase plumbing)
// ============================================================================

/// A repaint boundary's painted output, kept for reuse on a later frame.
///
/// Flattened in preorder with local indices: `parent` is `None` for a node
/// that hung directly off the boundary's own `OffsetLayer`, and `Some(i)` for
/// a child of `nodes[i]`. Local indices rather than `LayerId`s because every
/// frame builds a fresh `LayerTree` whose slab indices differ — grafting mints
/// new ids and maps them through this vector.
///
/// This is offset-independent, which is what makes it reusable at all: the
/// paint walk rebases a boundary's subtree to `Offset::ZERO` under its own
/// `OffsetLayer`, so a boundary that merely MOVED needs a different offset on
/// the layer above and nothing else.
#[derive(Clone, Debug)]
pub(super) struct RetainedSubtree {
    nodes: Vec<RetainedNode>,
    /// Every repaint boundary nested anywhere inside this capture.
    ///
    /// A nested boundary's layers are flattened into this subtree, so replaying
    /// it replays THEIR last output too. `mark_needs_paint` stops at the
    /// nearest established boundary, which means invalidating something deep
    /// inside leaves every enclosing boundary clean — and grafting one of those
    /// would replay the inner boundary's stale layers and never descend into
    /// it. Worse, the residue scan then clears the inner boundary's dirty flag,
    /// so the next frame does not retry.
    ///
    /// Checking this list at graft time is the bounded fix: an outer boundary
    /// declines to reuse itself while anything under it is dirty, and repaints
    /// in full. It costs the outer boundary's reuse on those frames. Keeping
    /// the reuse would mean capturing nested boundaries as HOLES and
    /// re-descending into them at graft time, which is a different and larger
    /// design.
    nested_boundaries: Vec<RenderId>,
    /// Where each render node's OWN effect layers landed in `nodes`.
    ///
    /// This is what makes an update-only commit addressable: a node whose
    /// alpha changed but whose painted content did not can have exactly these
    /// entries rebuilt while the rest of the capture is replayed untouched.
    ///
    /// Keyed by the node that pushed them, so it covers every node inside the
    /// boundary, not just the boundary itself — an `Opacity` nested three
    /// levels down is addressable without being a boundary of its own, which
    /// is the whole reason this design does not need to promote one.
    effect_slots: FxHashMap<RenderId, EffectSlots>,
}

/// One boundary's worth of composited-layer update work, decided during the
/// paint walk and applied by `run_paint` once the frame commits.
struct LayerPatch {
    /// `(index into RetainedSubtree::nodes, replacement layer)`.
    patches: Vec<(usize, Layer)>,
    /// The nodes whose pending-update flag this patch serves.
    consumed: SmallVec<[RenderId; 2]>,
}

/// Where one render node's own effect layers live inside a [`RetainedSubtree`],
/// plus what is needed to rebuild them.
#[derive(Clone, Debug)]
struct EffectSlots {
    /// Indices into [`RetainedSubtree::nodes`], in push order (outermost
    /// first) — the same order [`own_effect_layers`] returns.
    indices: SmallVec<[usize; 2]>,
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
    offset: Option<flui_types::Offset<flui_types::geometry::Pixels>>,
    /// The stamp a captured node carried, so a nested boundary survives its
    /// enclosing boundary's reuse — see `graft`.
    render_id: Option<RenderId>,
}

/// The layers a node pushes for its OWN effects, in push order (outermost
/// first), built from the hooks the paint walk reads off the node.
///
/// One function with two callers, deliberately: the paint walk pushes these
/// while painting, and the composited-layer-update arm rebuilds them without
/// painting. Two copies of this construction would drift, and the drift would
/// be a wrong-looking layer on a frame no test paints.
///
/// `origin` matters only to the transform: the node reports its matrix in
/// LOCAL coordinates while every run inside the layer space carries the
/// accumulated origin, so the matrix is conjugated by it. The opacity layer is
/// origin-independent (always `Offset::ZERO`), which is why an alpha-only
/// update is correct no matter where the node sits.
///
/// Alpha-blend coupling: the layer is emitted only when `paint_alpha()`
/// returns `Some`. A render object that overrides `paint_layer_blend() ->
/// Some(mode)` but leaves `paint_alpha() -> None` silently drops the blend
/// layer — the advanced compositor never sees it. When wiring the first
/// render-tree consumer of an advanced blend, override BOTH hooks and return
/// `Some(255)` from `paint_alpha()` for an opaque-blend-only layer.
fn own_effect_layers(
    alpha: Option<u8>,
    layer_blend: Option<flui_types::painting::BlendMode>,
    transform: Option<flui_types::Matrix4>,
    origin: Offset,
) -> SmallVec<[Layer; 2]> {
    let mut layers = SmallVec::new();
    if let Some(alpha) = alpha {
        let alpha_f32 = f32::from(alpha) / 255.0;
        layers.push(Layer::Opacity(match layer_blend {
            Some(blend) => OpacityLayer::with_blend(alpha_f32, Offset::ZERO, blend),
            None => OpacityLayer::with_offset(alpha_f32, Offset::ZERO),
        }));
    }
    if let Some(matrix) = transform {
        // Same math and same reason as the per-child `PushTransform` fragment
        // op (RenderFlow and friends).
        layers.push(Layer::Transform(TransformLayer::new(conjugate(
            matrix, origin,
        ))));
    }
    layers
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
    /// Leader/follower link relationships, populated as a byproduct of
    /// [`Self::push_layer`] pushing a `Layer::Leader`/`Layer::Follower`.
    /// Handed to `Scene::with_links` by the binding layer so `flui-engine`
    /// can resolve follower positions at render time against this same
    /// frame's fully-built `tree` (design research plan).
    link_registry: LinkRegistry,
    /// `(RenderId, LayerId)` correlation for each `Layer::Follower` pushed
    /// this paint pass (ADR-0015) — the general `RenderId -> LayerId`
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
    /// Open capture scopes, innermost last. Every boundary the walk meets is
    /// recorded into all of them, so each capture learns the boundaries nested
    /// anywhere beneath it — see `RetainedSubtree::nested_boundaries`.
    capture_scopes: Vec<Vec<RenderId>>,
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
    layer_patches: Vec<(RenderId, Vec<(usize, Layer)>)>,
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
}

impl FragmentComposer {
    /// `device_pixel_ratio` becomes the root layer's scale: the
    /// framework paints in LOGICAL pixels, the engine rasterizes in
    /// physical surface pixels — the root transform is the single
    /// place the two meet (Flutter's RenderView root transform).
    ///
    /// `root_boundary` stamps the root layer when the root render object
    /// declares itself a repaint boundary (`RenderView` does). It has to be
    /// threaded in here rather than applied later, because the root is never
    /// reached by `push_boundary_layer`: that fires from the PARENT's child
    /// loop, and the root has no parent. Without it the tree carries a
    /// boundary nothing can identify -- the worst shape for anything pairing
    /// boundaries across frames, which is what `render_id` exists for.
    fn new(device_pixel_ratio: f32, root_boundary: Option<RenderId>) -> Self {
        let mut tree = LayerTree::new();
        let root_layer = if (device_pixel_ratio - 1.0).abs() < f32::EPSILON {
            Layer::Offset(OffsetLayer::zero())
        } else {
            Layer::Transform(TransformLayer::new(flui_types::Matrix4::scaling(
                device_pixel_ratio,
                device_pixel_ratio,
                1.0,
            )))
        };
        let root = match root_boundary {
            Some(id) => tree.insert_node(LayerNode::new(root_layer).with_render_id(id)),
            None => tree.insert(root_layer),
        };
        tree.set_root(Some(root));
        Self {
            tree,
            stack: vec![root],
            open: DisplayList::new(),
            link_registry: LinkRegistry::new(),
            follower_correlations: Vec::new(),
            retained_captures: Vec::new(),
            capture_scopes: Vec::new(),
            effect_owner: FxHashMap::default(),
            layer_patches: Vec::new(),
            consumed_updates: Vec::new(),
            visited: FxHashSet::default(),
        }
    }

    /// Merges a sealed fragment run into the open picture.
    fn append_run(&mut self, run: DisplayList) {
        self.open.append(run);
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
        if flui_painting::DisplayListCore::is_empty(&self.open) {
            return;
        }
        let list = std::mem::take(&mut self.open);
        let layer_id = self.tree.insert(Layer::from(PictureLayer::new(list)));
        let parent = self.current_parent();
        self.tree.add_child(parent, layer_id);
    }

    /// Inserts `layer` under the current stack top, returning its freshly
    /// minted `LayerId` — the caller (`paint_subtree_impl`'s fragment-op
    /// replay loop) uses this to record the `RenderId -> LayerId`
    /// correlation for `Layer::Follower` pushes (ADR-0015).
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
    /// repaint cover the screen). The other half of that comparison —
    /// constant-time "is this content unchanged" — already exists, since
    /// `PictureLayer` shares its `DisplayList` behind an `Arc`.
    ///
    /// **Exactly one layer per boundary carries the stamp**, and it is this
    /// one: the `OffsetLayer` a boundary child rebases under, which is also
    /// the node a retained subtree is grafted beneath. Stamping a node's
    /// effect layers or a fragment's structural pushes as well would put
    /// several layers under one id and make pairing ambiguous — so those go
    /// through [`Self::push_layer`] and stay unstamped.
    fn push_boundary_layer(&mut self, layer: Layer, boundary_id: RenderId) -> LayerId {
        self.push_layer_node(LayerNode::new(layer).with_render_id(boundary_id))
    }

    fn push_layer_node(&mut self, node: LayerNode) -> LayerId {
        self.seal_picture();
        let layer = node.layer();
        // Extract the link-registry-relevant fields BEFORE `layer` moves
        // into the tree — `Leader`/`Follower` are `Copy`-field-bearing, so
        // this is a cheap read, not a clone of the layer itself.
        let leader_registration = layer
            .as_leader()
            .map(|leader| (leader.link(), leader.get_offset(), leader.size()));
        let follower_link = layer.as_follower().map(FollowerLayer::link);

        let id = self.tree.insert_node(node);
        if let Some((link, offset, size)) = leader_registration {
            self.link_registry.register_leader(link, id, offset, size);
        }
        if let Some(link) = follower_link {
            self.link_registry.register_follower(id, link);
        }

        let parent = self.current_parent();
        self.tree.add_child(parent, id);
        self.stack.push(id);
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
    /// `Layer::Follower` node (ADR-0015).
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
    /// Those register into `link_registry` as a side effect of
    /// [`Self::push_layer`], and a graft re-inserts layers without replaying
    /// that registration — a retained follower would lose its link on every
    /// frame it was reused. Refusing to retain them is the bounded answer;
    /// replaying the registration (and the `RenderId` correlation a follower
    /// also needs, which the flattened form does not carry) is the complete
    /// one.
    /// Record a boundary into every open capture scope.
    fn note_boundary(&mut self, id: RenderId) {
        for scope in &mut self.capture_scopes {
            scope.push(id);
        }
    }

    fn open_capture(&mut self) {
        self.capture_scopes.push(Vec::new());
    }

    fn close_capture(&mut self) -> Vec<RenderId> {
        self.capture_scopes.pop().expect(
            "BUG: close_capture without a matching open_capture — the paint walk pairs them",
        )
    }

    fn capture(&self, root: LayerId) -> Option<RetainedSubtree> {
        let mut nodes: Vec<RetainedNode> = Vec::new();
        // `effect_owner` is maintained incrementally by `record_effect_layer`,
        // so this walk answers "is this layer somebody's effect layer" in O(1)
        // without re-indexing anything per capture. The same layer can be
        // captured more than once when boundaries nest, which is why the
        // per-capture SLOTS below are still built here rather than shared.
        let mut effect_slots: FxHashMap<RenderId, EffectSlots> = FxHashMap::default();

        // (tree id, parent index in `nodes`)
        let mut stack: Vec<(LayerId, Option<usize>)> = self
            .tree
            .children(root)?
            .iter()
            .rev()
            .map(|&id| (id, None))
            .collect();

        while let Some((id, parent)) = stack.pop() {
            let node = self.tree.get(id)?;
            let layer = node.layer();
            if layer.as_leader().is_some() || layer.as_follower().is_some() {
                return None;
            }
            let index = nodes.len();
            if let Some(&(render_id, origin)) = self.effect_owner.get(&id) {
                effect_slots
                    .entry(render_id)
                    .or_insert_with(|| EffectSlots {
                        indices: SmallVec::new(),
                        origin,
                    })
                    .indices
                    .push(index);
            }
            nodes.push(RetainedNode {
                layer: layer.clone(),
                parent,
                offset: node.offset(),
                render_id: node.render_id(),
            });
            for &child in node.children().iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        Some(RetainedSubtree {
            nodes,
            nested_boundaries: Vec::new(),
            effect_slots,
        })
    }

    /// Re-insert a captured subtree under the current stack top.
    ///
    /// Mints fresh ids and maps the flattened parent indices through them, so
    /// the result is structurally identical to what painting would have
    /// produced — the layers themselves are clones, cheap because a
    /// `PictureLayer` shares its `DisplayList` behind an `Arc`.
    fn graft(&mut self, retained: &RetainedSubtree, patches: &[(usize, Layer)]) {
        // Seal first: an open picture run belongs BEFORE the grafted content
        // in draw order, exactly as it would if the boundary had painted.
        self.seal_picture();
        let root = self.current_parent();

        let mut minted: Vec<LayerId> = Vec::with_capacity(retained.nodes.len());
        for (index, node) in retained.nodes.iter().enumerate() {
            // Built as a whole `LayerNode` rather than inserted-then-mutated:
            // `offset` and `render_id` are construction-time fields with no
            // setters, which is also the shape that keeps a disposed node from
            // being resurrected by a stray mutation.
            //
            // The `render_id` carry is load-bearing for NESTED boundaries and
            // for them only. A top-level boundary's stamp sits on the
            // `OffsetLayer` its parent pushes, which is above the capture root
            // and re-created by that parent's own paint every frame. A nested
            // boundary's is not: its parent paints INSIDE the outer capture,
            // so its stamped `OffsetLayer` is one of these very nodes
            // (`RetainedSubtree` flattens nested boundaries — see its doc).
            // Dropping the carry makes every nested boundary unidentifiable
            // the moment its enclosing boundary is reused.
            // A patched index re-emits this node's layer built from the
            // render object's CURRENT properties instead of the captured one;
            // everything else is the cheap `Arc`-sharing clone retention is
            // built on.
            let layer = patches
                .iter()
                .find(|(patched, _)| *patched == index)
                .map_or_else(|| node.layer.clone(), |(_, layer)| layer.clone());
            let mut layer_node = flui_layer::LayerNode::new(layer);
            if let Some(render_id) = node.render_id {
                layer_node = layer_node.with_render_id(render_id);
            }
            if let Some(offset) = node.offset {
                layer_node = layer_node.with_offset(offset);
            }
            let id = self.tree.insert_node(layer_node);
            let parent = node.parent.map_or(root, |i| minted[i]);
            self.tree.add_child(parent, id);
            minted.push(id);
        }
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
        LinkRegistry,
        Vec<(RenderId, LayerId)>,
        Vec<(RenderId, Option<RetainedSubtree>)>,
        Vec<(RenderId, Vec<(usize, Layer)>)>,
        Vec<RenderId>,
        FxHashSet<RenderId>,
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
            self.link_registry,
            self.follower_correlations,
            self.retained_captures,
            self.layer_patches,
            self.consumed_updates,
            self.visited,
        )
    }
}

/// Conjugates `matrix` so it pivots around this layer's local `origin`
/// rather than the layer tree's own (0, 0).
///
/// Both callers report a transform in LOCAL coordinates while every run
/// they bracket carries the accumulated `origin` baked into its canvas
/// transform: the per-node [`RenderObject::paint_transform`](crate::traits::RenderObject::paint_transform)
/// hook (one transform for the whole node, applied here) and the
/// per-child [`FragmentOp::PushTransform`] op (`RenderFlow` and any
/// other Variable-arity node giving each child its own paint-time
/// transform). Flutter `PaintingContext.pushTransform`:
/// `T(offset)·M·T(−offset)`.
fn conjugate(matrix: flui_types::Matrix4, origin: Offset) -> flui_types::Matrix4 {
    if origin == Offset::ZERO {
        matrix
    } else {
        let (dx, dy) = (origin.dx.get(), origin.dy.get());
        flui_types::Matrix4::translation(dx, dy, 0.0)
            * matrix
            * flui_types::Matrix4::translation(-dx, -dy, 0.0)
    }
}

/// Maps a recorded effect-layer scope onto its `flui-layer` layer.
///
/// Scope shapes/bounds are recorded in the node's LOCAL coordinates, while
/// the runs they bracket carry the accumulated `origin` baked into their
/// canvas transforms — so every variant is shifted by `origin` here
/// (Flutter `pushClipRect`: `clipRect.shift(offset)`; `RenderShaderMask`'s
/// `maskRect = offset & size`; `RenderBackdropFilter`'s backdrop bounds
/// follow the same `offset & size` convention), or a scope away from the
/// parent origin would apply at the layer's (0,0) instead of the node's
/// position.
///
/// Always a real layer today; lowering non-composited clips back into
/// canvas clips inside the merged picture is a composer-side optimization
/// gated on the `needs_compositing` bits — correctness is identical
/// either way, so the recording API does not expose the choice.
fn scope_layer(scope: FragmentScope, origin: Offset) -> Layer {
    match scope {
        FragmentScope::Rect { rect, behavior } => {
            Layer::ClipRect(ClipRectLayer::new(rect.translate_offset(origin), behavior))
        }
        FragmentScope::RRect { rrect, behavior } => Layer::ClipRRect(ClipRRectLayer::new(
            rrect.translate_offset(origin),
            behavior,
        )),
        FragmentScope::Path { path, behavior } => {
            let path = if origin == Offset::ZERO {
                *path
            } else {
                path.translate(origin)
            };
            Layer::ClipPath(Box::new(ClipPathLayer::new(path, behavior)))
        }
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
        } => Layer::BackdropFilter(BackdropFilterLayer::new(
            filter,
            blend_mode,
            bounds.translate_offset(origin),
        )),
        // Oracle `LeaderLayer(link: link, offset: offset)` — `offset` is
        // this node's own accumulated position, exactly what `origin`
        // already is at this call site (`:270`). Unlike the clip/mask
        // variants above, `size` is NOT shifted by `origin` — it is a
        // pure dimension, not a position.
        FragmentScope::Leader { link, size } => {
            Layer::Leader(LeaderLayer::with_offset(link, size, origin))
        }
        // `Layer::Follower` carries no resolved position at all —
        // matching oracle, where a `FollowerLayer`'s `linkedOffset`/
        // `unlinkedOffset` are inputs to a LATER resolution pass, never
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

// ============================================================================
// Tests (ADR-0015 Slices A/B — the correlation byproduct + the resolution)
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_layer::LayerLink;
    use flui_tree::{Exact, Leaf};
    use flui_types::{Size, geometry::px, painting::Alignment};

    use super::*;
    use crate::{
        constraints::BoxConstraints,
        context::{BoxLayoutContext, PaintCx},
        parent_data::BoxParentData,
        protocol::BoxProtocol,
        traits::{RenderBox, RenderObject},
    };

    /// Minimal leaf that publishes a `Layer::Leader` — a local stand-in
    /// for `flui_objects::RenderLeaderLayer` (this crate's own tests must
    /// not depend on flui_objects). Always a repaint boundary so
    /// `paint.rs` wraps it in its own `Layer::Offset`, letting tests place
    /// two of these under DIFFERENT ancestor offsets — the
    /// cross-repaint-boundary case ADR-0015 targets.
    #[derive(Debug)]
    struct LeaderStub {
        link: LayerLink,
        size: Size,
    }

    impl flui_foundation::Diagnosticable for LeaderStub {}

    impl RenderBox for LeaderStub {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constraints().constrain(self.size)
        }

        fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
            let size = ctx.size();
            ctx.with_leader(self.link, size, |_ctx| {});
        }

        fn is_repaint_boundary(&self) -> bool {
            true
        }
    }

    /// Minimal leaf that publishes a `Layer::Follower` — the
    /// `RenderFollowerLayer` analogue of [`LeaderStub`].
    #[derive(Debug)]
    struct FollowerStub {
        link: LayerLink,
        size: Size,
        show_when_unlinked: bool,
    }

    impl flui_foundation::Diagnosticable for FollowerStub {}

    impl RenderBox for FollowerStub {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constraints().constrain(self.size)
        }

        fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
            let size = ctx.size();
            ctx.with_follower(
                self.link,
                size,
                Offset::ZERO,
                self.show_when_unlinked,
                Alignment::TOP_LEFT,
                Alignment::TOP_LEFT,
                |_ctx| {},
            );
        }

        fn is_repaint_boundary(&self) -> bool {
            true
        }
    }

    /// Two-slot container that positions each child at an explicit
    /// offset — the minimal stand-in for a `Stack`/`Flex` parent needed
    /// to place a leader and a follower under two DIFFERENT
    /// `Layer::Offset` ancestors without depending on flui_objects.
    #[derive(Debug, Default)]
    struct TwoSlotStub {
        offsets: [Offset; 2],
    }

    impl flui_foundation::Diagnosticable for TwoSlotStub {}

    impl RenderBox for TwoSlotStub {
        type Arity = Exact<2>;
        type ParentData = BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut BoxLayoutContext<'_, Exact<2>, BoxParentData>,
        ) -> Size {
            let constraints = *ctx.constraints();
            for i in 0..2 {
                ctx.layout_child(i, constraints);
                ctx.position_child(i, self.offsets[i]);
            }
            constraints.smallest()
        }

        fn paint(&self, ctx: &mut PaintCx<'_, Exact<2>>) {
            ctx.paint_children_in_order();
        }
    }

    /// Painting a tree containing a single follower node yields
    /// exactly one `(RenderId, LayerId)` correlation, whose `LayerId`
    /// names the pushed `Layer::Follower` node.
    #[test]
    fn fragment_composer_records_one_follower_correlation_for_a_follower_node() {
        let mut owner = PipelineOwner::new();
        let link = LayerLink::new();
        let root_id = owner.insert(Box::new(FollowerStub {
            link,
            size: Size::new(px(20.0), px(20.0)),
            show_when_unlinked: true,
        }) as Box<dyn RenderObject<BoxProtocol>>);
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(20.0), px(20.0)))));

        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout should succeed");
        let owner = owner.into_compositing();
        let owner = owner.into_paint();

        let mut composer = FragmentComposer::new(1.0, None);
        let dirty_ids = FxHashSet::default();
        owner
            .paint_subtree(
                &mut composer,
                root_id,
                Offset::ZERO,
                &dirty_ids,
                &FxHashMap::default(),
            )
            .expect("paint_subtree should succeed");
        let (
            layer_tree,
            _link_registry,
            follower_correlations,
            _retained,
            _patches,
            _consumed,
            _visited,
        ) = composer.finish();

        assert_eq!(
            follower_correlations.len(),
            1,
            "exactly one Layer::Follower push must yield exactly one \
             correlation entry, got {follower_correlations:?}"
        );
        let (correlated_render_id, correlated_layer_id) = follower_correlations[0];
        assert_eq!(
            correlated_render_id, root_id,
            "the correlation must name the follower's own RenderId"
        );
        assert!(
            layer_tree
                .get_layer(correlated_layer_id)
                .is_some_and(Layer::is_follower),
            "the correlated LayerId must point at the pushed Layer::Follower node"
        );
    }

    /// A leader+follower pair under two DIFFERENT `Layer::Offset`
    /// ancestors (the cross-repaint-boundary case) populates
    /// `last_follower_offsets` with the correctly-resolved displaced
    /// offset — the same value `flui_layer::resolve_follower_offset`
    /// itself returns for this shape (see
    /// `resolve_follower_offset_linked_across_offset_boundaries` in
    /// `flui-layer`).
    #[test]
    fn run_paint_resolves_follower_offset_across_different_offset_ancestors() {
        let mut owner = PipelineOwner::new();
        let link = LayerLink::new();

        let root_id = owner.insert(Box::new(TwoSlotStub {
            offsets: [Offset::ZERO, Offset::new(px(0.0), px(90.0))],
        }) as Box<dyn RenderObject<BoxProtocol>>);
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::new(
            px(0.0),
            px(300.0),
            px(0.0),
            px(300.0),
        )));

        owner
            .insert_child_render_object(
                root_id,
                Box::new(LeaderStub {
                    link,
                    size: Size::new(px(20.0), px(20.0)),
                }),
            )
            .expect("leader child insert");
        let follower_id = owner
            .insert_child_render_object(
                root_id,
                Box::new(FollowerStub {
                    link,
                    size: Size::new(px(10.0), px(10.0)),
                    show_when_unlinked: true,
                }),
            )
            .expect("follower child insert");

        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout should succeed");
        let owner = owner.into_compositing();
        let mut owner = owner.into_paint();
        owner.run_paint().expect("paint should succeed");

        let resolved = owner
            .last_follower_offsets
            .get(&follower_id)
            .copied()
            .expect("a linked, visible follower must resolve to an entry");

        // Leader chain offset (slot 0 = ZERO) + leader's own local offset
        // (ZERO — it registers at the origin its OWN boundary rebases to)
        // minus the follower chain offset (slot 1 = (0, 90)); default
        // TOP_LEFT/TOP_LEFT anchors and zero target_offset contribute
        // nothing further.
        assert_eq!(
            resolved,
            Offset::new(px(0.0), px(-90.0)),
            "resolved offset must sum the ancestor chains across the two \
             DIFFERENT Layer::Offset boundaries, not assume a shared parent"
        );
        assert!(
            owner.last_hidden_follower_ids.is_empty(),
            "a visible, resolved follower must not also be marked hidden"
        );
    }

    /// An unlinked follower with `show_when_unlinked == false`
    /// produces no `last_follower_offsets` entry, and is recorded in
    /// `last_hidden_follower_ids` so the hit-test walk can skip
    /// its subtree instead of silently falling through to normal
    /// traversal.
    #[test]
    fn run_paint_marks_unlinked_follower_hidden_when_show_when_unlinked_is_false() {
        let mut owner = PipelineOwner::new();
        let link = LayerLink::new(); // No leader is ever registered under this link.
        let follower_id = owner.insert(Box::new(FollowerStub {
            link,
            size: Size::new(px(10.0), px(10.0)),
            show_when_unlinked: false,
        }) as Box<dyn RenderObject<BoxProtocol>>);
        owner.set_root_id(Some(follower_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(10.0), px(10.0)))));

        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout should succeed");
        let owner = owner.into_compositing();
        let mut owner = owner.into_paint();
        owner.run_paint().expect("paint should succeed");

        assert!(
            !owner.last_follower_offsets.contains_key(&follower_id),
            "a hidden follower must not get a resolved-offset entry"
        );
        assert!(
            owner.last_hidden_follower_ids.contains(&follower_id),
            "a hidden follower must be recorded so the hit-test walk can \
             skip its subtree rather than falling through to normal \
             traversal"
        );
    }
}

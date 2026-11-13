//! Paint phase implementation for `PipelineOwner<PaintPhase>`.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_layer::{
    AnnotatedRegionLayer, BackdropFilterLayer, BoundaryContent, ClipPathLayer, ClipRRectLayer,
    ClipRectLayer, ClipSuperellipseLayer, ContentBuilder, EntryIndex, Layer, LeaderLayer, Lineage,
    OffsetLayer, OpacityLayer, Placement, SceneNodeId, SceneOp, SceneWriter, ShaderMaskLayer,
    TransformLayer,
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

    /// Paints the boundaries the queue owes and buffers the patch that
    /// publishes them in `writer` (ADR-0304 §3, §8).
    ///
    /// Phase 3 of the rendering pipeline, as a **fragment composition**
    /// (sans-IO paint model): each node's `paint_raw` records a
    /// node-local fragment — draw runs, child markers, clip scopes —
    /// which is immediately replayed into its boundary's
    /// [`BoundaryContent`]. Adjacent inline draw runs of one render object
    /// merge into a chunk, a run of another seals it (ADR-0304 §2); a
    /// repaint-boundary child gets a content of its own at `Offset::ZERO`,
    /// which its parent's content slots; clip scopes become effects.
    ///
    /// Only owed boundaries paint: a boundary absent from the queue emits
    /// nothing, one that moved emits a `Place`, and one queued only for a
    /// composited-layer update is republished with just the effect layers of
    /// the node that asked rebuilt — see `layer_patches_for`. The caller
    /// applies the patch and reports the version with
    /// [`scene_applied`](PipelineOwner::scene_applied); a patch never
    /// reported, a renewed lineage or a new pixel ratio makes the next pass
    /// repaint from the root.
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
    pub fn run_paint(&mut self, writer: &mut SceneWriter<'_>) -> crate::error::RenderResult<()> {
        // Frame-completeness gate, BEFORE the empty-paint-queue early
        // return: the paint phase began with layout work still pending. A
        // direct phase chain that skipped `run_layout` with a serviced
        // queue must be refused, not silently paint stale geometry; the
        // same chain with an EMPTY layout queue is the legitimate bench
        // pattern (`benches/semantics_assembly.rs`) and passes through.
        if self.scheduler.has_layout_work() {
            return Err(crate::error::RenderError::PaintBeforeLayout);
        }

        // A pass with nothing queued still repaints from the root when the
        // scene it would patch is not the one the records describe.
        #[expect(
            clippy::float_cmp,
            reason = "an unchanged ratio is bit-identical, not approximately equal"
        )]
        let owed = self.root_id.is_some()
            && (self.staged_scene.is_some()
                || self.retained_pixel_ratio != self.device_pixel_ratio
                || self
                    .scene_frame
                    .as_ref()
                    .is_none_or(|frame| frame.node.lineage() != writer.current().id().lineage()));
        if !self.scheduler.has_paint_work() && !owed {
            return Ok(());
        }

        let _span = tracing::debug_span!(
            target: "flui.frame",
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
        // A panic that unwinds out of the pass (a `tracing` subscriber, a
        // visual-update callback) leaves the paint queue as the error arm
        // does, for the retry to run on; the phase is left before it resumes,
        // so the next frame does not route its marks as mid-paint ones.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the paint pass leaves its phase and keeps the queue on a panic"
        )]
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.paint_in_phase(writer)))
        {
            Ok(result) => result,
            Err(payload) => {
                let _ = self.local_dirty.end_paint();
                let _ = self.scheduler.exit_phase(PhaseKind::Paint);
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// The paint pass proper, between entering and leaving the phase: see
    /// [`Self::run_paint`].
    fn paint_in_phase(&mut self, writer: &mut SceneWriter<'_>) -> crate::error::RenderResult<()> {
        let lineage = writer.current().id().lineage();
        // The records name nodes of the version `writer` patches only while the
        // last staged patch was applied (`scene_applied`) and the owner kept
        // its lineage; otherwise they describe a scene nobody holds, and the
        // frame repaints from the root. Retained output recorded for another
        // pixel ratio is dropped too: text shaping and hairline snapping
        // depend on it.
        #[expect(
            clippy::float_cmp,
            reason = "an unchanged ratio is bit-identical, not approximately equal"
        )]
        let ratio_changed = self.retained_pixel_ratio != self.device_pixel_ratio;
        // A patch never reported applied holds the only copies of the contents
        // it recorded: the scene dropped its buffered operations when this
        // patch opened. They go to the writer, which drops them outside this
        // owner's borrow: their annotations are user values.
        let unapplied = match self.staged_scene.take() {
            Some(staged) => {
                self.discard_staged(staged, writer);
                true
            }
            None => false,
        };
        let renewed = self
            .scene_frame
            .as_ref()
            .is_some_and(|frame| frame.node.lineage() != lineage);
        if ratio_changed || unapplied || renewed {
            self.forget_scene();
            self.retained_pixel_ratio = self.device_pixel_ratio;
        }

        // Deepest first, the order ADR-0304 §8 paints owed boundaries in.
        self.scheduler.sort_paint_deep_first();

        // The pass's working collections, back in the owner once it commits;
        // a pass that fails or unwinds leaves the next one fresh ones.
        let mut scratch = self.paint_scratch.take().unwrap_or_default();
        scratch.dirty_ids.clear();
        scratch.layer_updates.clear();
        // Whether the walk finished, so the composer's `visited` holds which
        // queued nodes it reached — the residue scan's oracle. False when
        // there is no root to paint from, which correctly makes every queued
        // entry unreached.
        let mut walked = false;
        if let Some(root_id) = self.root_id
            && let Some(root) = self.render_tree.get(root_id)
        {
            let PaintScratch {
                dirty_ids,
                layer_updates,
                composer: work,
            } = &mut *scratch;
            dirty_ids.extend(self.scheduler.nodes_needing_paint().iter().map(|d| d.id));
            // Boundaries queued only to have some node's own effect layers
            // rebuilt. The queue carries its own reason, so there is no second
            // record to reconcile and nothing here reads a node flag: after a
            // pass that failed partway the queue is what the retry runs on,
            // while every flag the walk touched is already clear.
            layer_updates.extend(self.scheduler.nodes_needing_paint().iter().filter_map(|e| {
                match &e.kind {
                    crate::pipeline::PaintKind::LayerUpdate(targets) => {
                        Some((e.id, targets.clone()))
                    }
                    crate::pipeline::PaintKind::Repaint => None,
                }
            }));
            let root_is_boundary = root.is_repaint_boundary();
            // The frame node is repainted when there is none, when the root
            // paints inline and is queued, or when the root changed: anything
            // under it is decided boundary by boundary below.
            let frame_dirty = match &self.scene_frame {
                None => true,
                Some(frame) => {
                    frame.root != root_id
                        || frame.root_is_boundary != root_is_boundary
                        || (!root_is_boundary && dirty_ids.contains(&root_id))
                }
            };
            let inputs = PaintInputs {
                dirty_set: dirty_ids,
                layer_updates,
                frame_dirty,
            };
            let mut composer = FragmentComposer::new(writer, work);
            let painted = self.paint_owed(&mut composer, root_id, root_is_boundary, &inputs);
            match painted {
                Ok(()) => {
                    let PaintOutcome { staged, counts } = composer.finish(self, lineage);
                    self.counters.nodes_painted += counts.nodes_painted;
                    self.counters.layers_produced += counts.layers_produced;
                    self.counters.layers_reused += counts.layers_reused;
                    self.counters.scene_ops += staged.ops;
                    tracing::debug!(ops = staged.ops, "run_paint: patch staged");
                    self.staged_scene = Some(staged);

                    // The requests the patch serves are satisfied once the frame
                    // commits. An error above returned before this point, leaving
                    // the flags set so the retry re-derives them; a mark refuses
                    // itself while its flag is set, so a flag never cleared would
                    // stop the boundary being re-queued.
                    for render_id in work.consumed_updates.drain(..) {
                        if let Some(node) = self.render_tree.get(render_id) {
                            node.clear_needs_composited_layer_update();
                        }
                    }
                    // Child paint orders: a node repainted this pass replaces what it
                    // recorded before; one left clean keeps it. A node that left the
                    // tree takes its entry with it.
                    for (render_id, order) in work.child_paint_orders.drain(..) {
                        match order {
                            Some(order) => {
                                self.last_child_paint_orders.insert(render_id, order);
                            }
                            None => {
                                self.last_child_paint_orders.remove(&render_id);
                            }
                        }
                    }
                    let tree = &self.render_tree;
                    self.last_child_paint_orders
                        .retain(|render_id, _| tree.get(*render_id).is_some());
                    walked = true;
                }
                Err(e) => {
                    // Nothing of the pass is kept: the patch buffered in
                    // `writer` is never applied (ADR-0304 §10), and the queue
                    // stays for the retry. Restore the debug invariant before
                    // propagating so the owner stays consistent.
                    let _ = self.local_dirty.end_paint();
                    let _ = self.scheduler.exit_phase(PhaseKind::Paint);
                    return Err(e);
                }
            }
        }

        // Dirty-list residue scan: any queued node the paint did not REACH
        // (multi-root, detached subtree, or a child its parent stopped laying
        // out). Warn + clear so the bug is visible AND the dirty list doesn't
        // accumulate across frames.
        //
        // Keyed on what the walk recorded, not on the node still being flagged
        // needs-paint. That flag is cleared by the walk itself while
        // `run_paint`'s error arm keeps the queue for a retry, so after a pass
        // that failed partway the flag test skips exactly the entries this scan
        // exists to catch.
        for dirty_node in self.scheduler.nodes_needing_paint() {
            let reached = walked && scratch.composer.visited.contains(&dirty_node.id);
            if !reached && let Some(render_node) = self.render_tree.get(dirty_node.id) {
                tracing::warn!(
                    id = dirty_node.id.as_u64(),
                    depth = dirty_node.depth,
                    "run_paint: dirty node not reached by root descent (multi-root, \
                     detached subtree, or a child its parent stopped laying out); \
                     paint dropped, flag cleared, boundary record dropped"
                );
                render_node.clear_needs_paint();
                // Dropping the invalidation must drop the recorded output with
                // it: the next frame that reaches this boundary would otherwise
                // slot content already known to be stale. Its node stays
                // published until the parent that names it repaints.
                if let Some(staged) = self.staged_scene.as_mut() {
                    staged.stale.push(dirty_node.id);
                }
            }
        }
        // `clear()` retains capacity (preserve Vec backing across frames).
        self.scheduler.clear_paint_queue();
        scratch.recycle();
        self.paint_scratch = Some(scratch);

        // exit_phase clears debug_doing_paint AND drains mid-paint marks back
        // into dirty so paint marks made during this pass become next-frame
        // work rather than being stranded; on the error path above too.
        let layout_during_paint = self.local_dirty.end_paint();
        let _ = self.scheduler.exit_phase(PhaseKind::Paint);
        debug_assert!(
            layout_during_paint.is_none(),
            "BUG: render object {layout_during_paint:?} requested layout during paint; \
             paint must not change geometry"
        );

        Ok(())
    }

    /// Paints what the queue owes (ADR-0304 §8): each queued boundary with a
    /// record and no repainting ancestor, alone, deepest first; then the
    /// frame node when it is dirty, which reaches every boundary that has no
    /// record yet through its parent's paint.
    ///
    /// A boundary under an ancestor that repaints this frame is left to that
    /// ancestor, which reaches it again if it still places it: painted alone
    /// first, it would read the anchor of the layout before, and painted
    /// twice, it would cost twice.
    fn paint_owed(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
        root_id: RenderId,
        root_is_boundary: bool,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        // Served once every repaint above has staged the placements it may
        // move them by.
        let mut settled_later: SmallVec<[RenderId; 4]> = SmallVec::new();
        for queued in self.scheduler.nodes_needing_paint() {
            let id = queued.id;
            if !self.boundary_records.contains_key(&id) {
                continue;
            }
            if self.slotted_by_a_repaint(id, inputs) {
                continue;
            }
            if self.under_a_repaint(id, inputs) {
                settled_later.push(id);
                continue;
            }
            self.serve_alone(composer, id, inputs)?;
        }
        if inputs.frame_dirty {
            self.paint_frame(composer, root_id, root_is_boundary, inputs)?;
        }
        // Shallowest first, so a deferred boundary's placement is staged
        // before one nested in it reads it.
        for &id in settled_later.iter().rev() {
            self.serve_alone(composer, id, inputs)?;
        }
        Ok(())
    }

    /// Paints the frame node's content: the device-pixel-ratio layer over the
    /// root, inline or slotted.
    fn paint_frame(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
        root_id: RenderId,
        root_is_boundary: bool,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        composer.begin_boundary(None);
        let root_layer = if (self.device_pixel_ratio - 1.0).abs() < f64::EPSILON {
            Layer::Offset(OffsetLayer::zero())
        } else {
            Layer::Transform(TransformLayer::new(
                flui_foundation::geometry::Matrix4::scaling(
                    self.device_pixel_ratio,
                    self.device_pixel_ratio,
                    1.0,
                ),
            ))
        };
        // The device-pixel-ratio layer is the frame's own: no render object
        // pushed it, so it is filed under the root, after the root's effects
        // in ordinal space because they live in another content.
        composer.push_layer(root_id, root_layer);
        let painted = if root_is_boundary {
            // The root sits at the frame's origin whatever its own offset says,
            // as it always has: its record carries a zero override.
            self.place_boundary(
                composer,
                root_id,
                Offset::ZERO,
                Some(Offset::ZERO),
                None,
                inputs,
            )
        } else {
            self.paint_subtree(composer, root_id, Offset::ZERO, None, inputs)
        };
        composer.pop_layer();
        painted?;
        composer.finish_frame(self, root_id, root_is_boundary);
        Ok(())
    }

    /// Whether the walk of another content reaches the boundary `id` this
    /// frame, so it is not served alone: its parent's content (the frame
    /// node's, for a top-level boundary) is repainted, which paints it at its
    /// slot. A repaint reaches only the boundaries its content slots; one
    /// nested under a clean boundary is served alone. A boundary whose chain
    /// no longer reaches the frame node is not served at all.
    fn slotted_by_a_repaint(&self, id: RenderId, inputs: &PaintInputs<'_>) -> bool {
        let Some(record) = self.boundary_records.get(&id) else {
            return true;
        };
        let reached = match record.placed.parent {
            None => inputs.frame_dirty,
            Some(parent) => inputs.repaints(parent),
        };
        if reached {
            return true;
        }
        let mut at = record.placed.parent;
        while let Some(ancestor) = at {
            let Some(record) = self.boundary_records.get(&ancestor) else {
                // A chain broken by a removed ancestor reaches nothing.
                return true;
            };
            at = record.placed.parent;
        }
        false
    }

    /// Whether a boundary above `id`'s parent repaints this frame, or the
    /// frame node does above a chain of clean boundaries: that repaint may
    /// move the clean boundaries between, so `id` is served alone only once
    /// their placements are staged.
    fn under_a_repaint(&self, id: RenderId, inputs: &PaintInputs<'_>) -> bool {
        let mut at = self
            .boundary_records
            .get(&id)
            .and_then(|record| record.placed.parent);
        while let Some(ancestor) = at {
            let Some(record) = self.boundary_records.get(&ancestor) else {
                return false;
            };
            match record.placed.parent {
                Some(parent) if inputs.repaints(parent) => return true,
                Some(parent) => at = Some(parent),
                None => return inputs.frame_dirty,
            }
        }
        false
    }

    /// Where `id`'s slot puts it: the placement staged this frame when its
    /// parent's repaint placed it, else the one its record holds.
    fn placed_of(&self, composer: &FragmentComposer<'_, '_, '_>, id: RenderId) -> Option<Placed> {
        composer
            .staged_placed(id)
            .or_else(|| self.boundary_records.get(&id).map(|record| record.placed))
    }

    /// The paint anchor a walk from the root would hand the boundary `id` at
    /// its slot, derived from its placements' `parent` chain (ADR-0304 §8):
    /// O(depth), and never stale, since an ancestor that moved changed only
    /// its own placement, staged this frame or recorded before.
    fn inherited_anchor(
        &self,
        composer: &FragmentComposer<'_, '_, '_>,
        id: RenderId,
    ) -> Option<AnchorFrame> {
        let mut chain: SmallVec<[Placed; 8]> = SmallVec::new();
        let mut at = Some(id);
        while let Some(current) = at {
            let placed = self.placed_of(composer, current)?;
            chain.push(placed);
            if placed.slot_anchor.is_some() {
                break;
            }
            at = placed.parent;
        }
        // Outermost first: the frame that reaches the slot of the innermost.
        let mut frame: Option<AnchorFrame> = None;
        for (depth, placed) in chain.iter().enumerate().rev() {
            let at_slot = placed.slot_anchor.or(frame);
            if depth == 0 {
                return at_slot;
            }
            frame = at_slot.map(|anchor| anchor.rebased(placed.placement));
        }
        None
    }

    /// Serves a queued boundary whose ancestors are all clean, where its
    /// record says it stands.
    fn serve_alone(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
        id: RenderId,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        if composer.served(id) {
            return Ok(());
        }
        let Some(record) = self.boundary_records.get(&id) else {
            return Ok(());
        };
        let Some(node) = self.render_tree.get(id) else {
            return Ok(());
        };
        if !node.is_repaint_boundary() {
            return Ok(());
        }
        let placement = self
            .placed_of(composer, id)
            .map_or(record.placed.placement, |placed| placed.placement);
        let region_anchor = self
            .inherited_anchor(composer, id)
            .map(|anchor| anchor.rebased(placement));
        let fresh = self.refresh_boundary(composer, id, Some(record), region_anchor, inputs)?;
        if let Some(fresh) = fresh {
            composer.stage_record(id, fresh);
        }
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
        composer: &mut FragmentComposer<'_, '_, '_>,
        node_id: RenderId,
        origin: Offset,
        anchor: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        ensure_stack(|| self.paint_subtree_impl(composer, node_id, origin, anchor, inputs))
    }

    /// Fills `patches` with the effect-layer replacements `record`'s content
    /// needs this frame and returns the nodes whose requests they serve, or
    /// `Ok(None)` when one of them cannot be expressed as a patch.
    ///
    /// Walks the nodes whose own effect layers this content holds and rebuilds
    /// those of them that asked for a composited-layer update, through the
    /// same [`own_effect_layers`] the paint walk uses. Everything else in the
    /// content is kept untouched.
    ///
    /// Returns `Ok(None)` — meaning "repaint instead" — when a node's effect
    /// layers no longer have the same SHAPE they were recorded with: a
    /// different count, or a different layer kind at the same position. Both
    /// are structural changes a patch cannot express (an alpha crossing out of
    /// the layered range drops its layer entirely), and both are correctly
    /// served by a repaint.
    ///
    /// A target that owns no effect in this content cannot be served by a
    /// patch either: its effect layers did not exist when the content was
    /// recorded. A childless `RenderRotatedBox` is a real caller of that
    /// branch: its setter reports `COMPOSITED_LAYER_UPDATE` for a same-parity
    /// turn change whether or not it has a child, while `paint_effects`
    /// returns no transform without one. An empty patch reported as `Some`
    /// would keep the stale content and strand the node's own flag.
    ///
    /// # Errors
    ///
    /// `Poisoned` with phase [`PoisonPhase::LayerUpdate`] when a target's
    /// `paint_effects` — or the resolution of a `PaintClip::PathTarget` it
    /// reports — panics while the patch is built; the frame is discarded and
    /// the queued update survives for the retry.
    fn layer_patches_for(
        &self,
        record: &BoundaryRecord,
        targets: &[RenderId],
        patches: &mut Vec<(EntryIndex, Layer)>,
    ) -> crate::error::RenderResult<Option<SmallVec<[RenderId; 2]>>> {
        if targets.iter().any(|target| {
            !record
                .effect_slots()
                .iter()
                .any(|(owner, _)| owner == target)
        }) {
            return Ok(None);
        }
        patches.clear();
        // Nodes whose request this patch serves, cleared by `run_paint` only
        // once the frame commits: the walk can still fail after this point.
        let mut consumed: SmallVec<[RenderId; 2]> = SmallVec::new();
        // In record order, so a target resolves in the sequence a full
        // repaint's paint walk would visit it in.
        for (render_id, slots) in record.effect_slots() {
            let render_id = *render_id;
            let Some(node) = self.render_tree.get(render_id) else {
                // The node is gone but its layers are still in the content.
                // Nothing asked for an update, so keeping them is what a clean
                // frame would do anyway.
                continue;
            };
            if !node.needs_composited_layer_update() {
                continue;
            }
            // A panic here poisons once rather than falling back to a repaint,
            // which would call the same `paint_effects` and panic again; the
            // return comes before `consumed` reaches the caller, so the request
            // stays queued for the retry.
            let mut fresh = SmallVec::<[Layer; 3]>::new();
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: paint_effects runs contained; a panic poisons without a retry loop"
            )]
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                own_effect_layers(node.paint_effects(), slots.origin, &mut fresh);
            }));
            if let Err(payload) = built {
                drop(fresh);
                flui_foundation::panic::discard_payload(payload);
                return Err(crate::error::RenderError::poisoned(
                    node.debug_name(),
                    PoisonPhase::LayerUpdate,
                ));
            }
            if fresh.len() != slots.indices.len() {
                return Ok(None);
            }
            for (&index, layer) in slots.indices.iter().zip(fresh) {
                let Some(recorded) = record.content.effect_layer(index) else {
                    debug_assert!(false, "BUG: an effect slot names no effect of its content");
                    return Ok(None);
                };
                // Under the fixed order `own_effect_layers` emits (opacity,
                // clip, transform), the count check plus this per-position
                // kind compare is a complete shape check.
                if std::mem::discriminant(&layer) != std::mem::discriminant(recorded) {
                    return Ok(None);
                }
                patches.push((index, layer));
            }
            consumed.push(render_id);
        }
        Ok(Some(consumed))
    }

    /// Body of [`Self::paint_subtree`]; split out so every recursion
    /// level enters through the [`ensure_stack`] probe.
    fn paint_subtree_impl(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
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
            composer.work.visited.insert(node_id);
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
            composer.work.consumed_updates.push(node_id);
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
        // The node records after its ancestors' unreplayed operations in the
        // pass's one recorder, and its children record after its own, so a
        // node that paints only its child allocates nothing. Its runs record
        // into the recorder's one canvas, which keeps the paints the nodes
        // before it interned.
        let recorder = &mut composer.work.recorder.0;
        recorder.begin(
            origin,
            self.device_pixel_ratio,
            anchor.map(|frame| frame.at(origin)),
        );
        let own_effects = &mut composer.work.own_effects;
        debug_assert!(
            own_effects.is_empty(),
            "BUG: the last node took its effects"
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a render object's paint runs contained; a panic poisons the node"
        )]
        let painted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_node.paint_raw(recorder, child_ids.len());
            own_effect_layers(render_node.paint_effects(), origin, own_effects);
        }));
        if let Err(payload) = painted {
            composer.work.own_effects.clear();
            composer.work.recorder.0.abandon();
            flui_foundation::panic::discard_payload(payload);
            return Err(crate::error::RenderError::poisoned(
                debug_name,
                PoisonPhase::Paint,
            ));
        }
        let start = composer.work.recorder.0.end();
        let end = composer.work.recorder.0.ops_mut().len();
        composer.note_painted();

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
        // Taken out while they are pushed, and put back empty: the next
        // node collects into the same buffer. A node without effects moves
        // no layer.
        let mut own_effects = std::mem::take(&mut composer.work.own_effects);
        let effect_layers = own_effects.len();
        for layer in own_effects.drain(..) {
            let layer_id = composer.push_layer(node_id, layer);
            // Record where this node's own effect layers landed, so a later
            // frame can rebuild just these and replay everything else.
            composer.record_effect_layer(node_id, layer_id, origin);
        }
        composer.work.own_effects = own_effects;

        let mut painted_children = ChildPaintOrder::new();
        // The group opacity of each scope the fragment has open, so a child painted inside
        // one (a flow delegate's `paint_child_with_opacity`) records what it is drawn at.
        let mut scope_opacity: SmallVec<[f64; 4]> = SmallVec::new();
        for at in start..end {
            // Taken out one at a time: a child painted below appends after
            // `end` and leaves the buffer as long as it found it.
            let Some(slot) = composer.work.recorder.0.ops_mut().get_mut(at) else {
                debug_assert!(false, "BUG: a child shortened its parent's fragment");
                break;
            };
            let op = std::mem::replace(slot, FragmentOp::Pop);
            match op {
                FragmentOp::Run(list) => composer.append_run(node_id, list),
                FragmentOp::Push(scope) => {
                    // Follower hit-testing: capture the RenderId -> LayerId
                    // correlation as a near-free byproduct of pushing a
                    // Layer::Follower — `node_id` is already in scope from
                    // this fragment-op replay loop, and `push_layer` hands
                    // back the freshly-minted LayerId.
                    let is_follower = matches!(*scope, FragmentScope::Follower(_));
                    scope_opacity.push(match &*scope {
                        FragmentScope::Opacity(opacity) => opacity.alpha(),
                        _ => 1.0,
                    });
                    let layer_id = composer.push_layer(node_id, scope_layer(*scope, origin));
                    if is_follower {
                        composer.record_follower_correlation(node_id, layer_id, origin);
                    }
                }
                FragmentOp::PushTransform(matrix) => {
                    scope_opacity.push(1.0);
                    composer.push_layer(
                        node_id,
                        Layer::Transform(TransformLayer::new(conjugate(*matrix, origin))),
                    );
                }
                FragmentOp::Pop => {
                    scope_opacity.pop();
                    composer.pop_layer();
                }
                FragmentOp::Child {
                    index,
                    offset_override,
                } => {
                    painted_children.push(PaintedChild {
                        slot: index,
                        opacity: scope_opacity.iter().product(),
                    });
                    let Some(&child_id) = child_ids.get(index) else {
                        debug_assert!(
                            false,
                            "fragment child marker {index} out of range ({} children) — \
                             PaintContext bounds-checks markers, so a mismatch means the \
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
                        // Inline children paint in the parent's layer space,
                        // origin baked in, into pictures of their own.
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
        // Every child returned to `end`; what is left is this node's
        // replayed operations. A pass that fails above leaves the rest for
        // the next pass's `clear`.
        let ops = composer.work.recorder.0.ops_mut();
        ops.truncate(start);
        if start == 0 && ops.capacity() > KEPT_FRAGMENT_OPS {
            *ops = Vec::new();
        }

        // The children that went down, in the order they lie over each other and at the
        // opacity of the scopes around them: what the input-region and presented walks
        // follow. Most parents paint every child first to last outside any scope; only the
        // rest keep their list, a parent that leaves children out included.
        if !child_ids.is_empty() {
            let plain = painted_children.len() == child_ids.len()
                && painted_children
                    .iter()
                    .enumerate()
                    .all(|(position, child)| child.slot == position && child.opacity == 1.0);
            // A plain order is the absence of an entry: one is queued only
            // to record a list or to drop the one an earlier pass kept, so
            // the queue the pass keeps between frames does not grow with
            // every parent.
            if !plain {
                composer
                    .work
                    .child_paint_orders
                    .push((node_id, Some(painted_children)));
            } else if self.last_child_paint_orders.contains_key(&node_id) {
                composer.work.child_paint_orders.push((node_id, None));
            }
        }

        for _ in 0..effect_layers {
            composer.pop_layer();
        }

        Ok(())
    }

    /// Places the repaint boundary `child_id` in the content the composer is
    /// building: serves it (paints it afresh, patches its composited layers,
    /// or leaves it as it is), slots its node there, and places the node
    /// where the walk put it.
    ///
    /// `origin` is the parent's accumulated origin in the current layer space
    /// and `anchor` the paint-anchor frame in force at the parent.
    fn place_boundary(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
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

    fn place_boundary_impl(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
        child_id: RenderId,
        origin: Offset,
        offset_override: Option<Offset>,
        anchor: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        let Some(child_node) = self.render_tree.get(child_id) else {
            return Ok(());
        };
        let computed = origin + offset_override.unwrap_or_else(|| child_node.offset());
        let committed = self.boundary_records.get(&child_id);
        // Non-finite geometry is never published (ADR-0054): the boundary stays
        // where its slot last put it, at its parent's origin when it has never
        // been placed, and its record holds that placement, so the next frame
        // finds nothing to move.
        let placement = if computed.is_finite() {
            computed
        } else {
            tracing::warn!(
                boundary = child_id.as_u64(),
                offset = ?computed,
                "paint: a boundary placed at a non-finite offset keeps its last placement"
            );
            committed.map_or_else(
                || {
                    if origin.is_finite() {
                        origin
                    } else {
                        Offset::ZERO
                    }
                },
                |record| record.placed.placement,
            )
        };
        let region_anchor = anchor.map(|frame| frame.rebased(placement));
        let node = if composer.served(child_id) {
            composer.node_of(self, child_id)
        } else {
            match self.refresh_boundary(composer, child_id, committed, region_anchor, inputs)? {
                Some(fresh) => {
                    let node = fresh.node;
                    composer.stage_record(child_id, fresh);
                    Some(node)
                }
                None => committed.map(|record| record.node),
            }
        };
        let Some(node) = node else {
            debug_assert!(false, "BUG: a served boundary has a node");
            return Ok(());
        };
        let placed = Placed {
            origin,
            offset_override,
            // A capture stores the frame only when the anchor is a node of the
            // region being painted; an inherited one is derived again from the
            // parent's record.
            slot_anchor: anchor.filter(|frame| frame.local),
            parent: composer.current_boundary(),
            placement,
        };
        composer.slot(child_id, node);
        let before = committed.filter(|record| record.node == node);
        if before.is_none_or(|record| record.placed.placement != placement) {
            composer.place(node, placement);
        }
        if before.is_none_or(|record| record.placed != placed) {
            composer.stage_placed(child_id, placed);
        }
        Ok(())
    }

    /// What `id`'s content becomes this frame: a fresh record when it is
    /// repainted or its composited layers are patched, `None` when it stays
    /// as `committed` holds it.
    ///
    /// A boundary without a record, queued for a repaint, or whose record was
    /// dropped as stale is painted afresh in a content of its own, at its own
    /// origin. One queued only for a composited-layer update is republished
    /// with that node's effects rebuilt, every other entry shared.
    fn refresh_boundary(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
        id: RenderId,
        committed: Option<&BoundaryRecord>,
        region_anchor: Option<AnchorFrame>,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<Option<BoundaryRecord>> {
        composer.mark_served(id);
        if inputs.dirty_set.contains(&id) {
            composer.work.visited.insert(id);
        }
        let update_targets = inputs.layer_updates.get(&id);
        let kept = committed.filter(|record| {
            !(record.stale || update_targets.is_none() && inputs.dirty_set.contains(&id))
        });
        if let Some(record) = kept {
            let Some(targets) = update_targets else {
                return Ok(None);
            };
            // Lent out of the composer while the patch is built; an error
            // leaves the composer a fresh one.
            let mut patches = std::mem::take(&mut composer.work.layer_patches);
            let served = self.layer_patches_for(record, targets, &mut patches)?;
            let fresh = served.map(|consumed| {
                composer.work.consumed_updates.extend(consumed);
                if patches.is_empty() {
                    return None;
                }
                tracing::trace!(
                    boundary = id.as_u64(),
                    layers = patches.len(),
                    "paint: composited-layer update, content republished"
                );
                let rebuilt = patches.len() as u64;
                let content = composer
                    .writer
                    .with_effects(&record.content, patches.drain(..))
                    .expect("BUG: a layer patch names effects of the content it patches");
                composer.counts.layers_produced += rebuilt;
                composer.counts.layers_reused +=
                    (content.entries().len() as u64).saturating_sub(rebuilt);
                let fresh = BoundaryRecord {
                    content,
                    ..record.clone()
                };
                // The patched content keeps every slot of the one it replaces.
                composer
                    .work
                    .slotted
                    .extend(fresh.children.iter().map(|&(_, node)| node));
                composer.publish(fresh.node, Arc::clone(&fresh.content));
                Some(fresh)
            });
            patches.clear();
            composer.work.layer_patches = patches;
            if let Some(fresh) = fresh {
                return Ok(fresh);
            }
        }
        composer.begin_boundary(Some(id));
        let painted = self.paint_subtree(composer, id, Offset::ZERO, region_anchor, inputs);
        let build = composer.end_boundary();
        painted?;
        let node = match committed {
            Some(record) => record.node,
            None => composer.mint_node(),
        };
        let fresh = build.into_record(
            node,
            committed.map_or(Placed::UNPLACED, |record| record.placed),
        );
        composer.publish(node, Arc::clone(&fresh.content));
        Ok(Some(fresh))
    }
}

// ============================================================================
// Fragment composition (paint phase plumbing)
// ============================================================================

/// What the owner keeps for each repaint boundary it published (ADR-0304 §8):
/// its node, the content it published there, and the values a walk from the
/// root would hand it at its slot, so a boundary painted alone paints as it
/// would under its parent.
#[derive(Clone, Debug)]
pub(super) struct BoundaryRecord {
    node: SceneNodeId,
    content: Arc<BoundaryContent>,
    placed: Placed,
    /// Each node whose own effect layers this content holds, the entries of
    /// those layers, and the origin they were built at: what a
    /// composited-layer update rebuilds; shared by the record a
    /// composited-layer update makes, and `None` when there are none.
    effect_slots: Option<Arc<[(RenderId, EffectSlots)]>>,
    /// Each follower effect in the content, with the origin its subtree paints
    /// at, for hit-testing and input regions.
    followers: SmallVec<[(RenderId, EntryIndex, Offset); 1]>,
    /// The boundaries the content slots.
    children: SmallVec<[(RenderId, SceneNodeId); 4]>,
    /// Dropped by the residue scan: a paint was owed and never made, so the
    /// next frame that reaches the boundary repaints it into the same node.
    stale: bool,
}

impl BoundaryRecord {
    /// The node the boundary is published at.
    pub(super) fn node(&self) -> SceneNodeId {
        self.node
    }

    /// Each node whose own effect layers the content holds, with their
    /// entries and origin.
    fn effect_slots(&self) -> &[(RenderId, EffectSlots)] {
        self.effect_slots.as_deref().unwrap_or_default()
    }
}

/// Where a boundary's slot put it, as its parent's paint last placed it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placed {
    /// The parent's accumulated origin in its layer space at the slot.
    origin: Offset,
    /// The offset the parent painted it at instead of its own, if any.
    offset_override: Option<Offset>,
    /// The paint anchor in force at the slot when the anchor is a node of the
    /// parent's region; an inherited one is derived from the parent's record.
    slot_anchor: Option<AnchorFrame>,
    /// The boundary that slots it, `None` for the frame node.
    parent: Option<RenderId>,
    /// Its node's offset in the parent's content: `origin` plus its position.
    placement: Offset,
}

impl Placed {
    /// A record not placed yet; the slot that names it places it.
    const UNPLACED: Self = Self {
        origin: Offset::ZERO,
        offset_override: None,
        slot_anchor: None,
        parent: None,
        placement: Offset::ZERO,
    };
}

/// The frame node a pipeline publishes its root under: the device-pixel-ratio
/// layer, and the root's slot or its inline paint.
#[derive(Clone, Debug)]
pub(super) struct FrameRecord {
    node: SceneNodeId,
    root: RenderId,
    root_is_boundary: bool,
    followers: SmallVec<[(RenderId, EntryIndex, Offset); 1]>,
    children: SmallVec<[(RenderId, SceneNodeId); 4]>,
}

impl FrameRecord {
    /// The node the frame is published at, which the scene's root slots.
    pub(super) fn node(&self) -> SceneNodeId {
        self.node
    }
}

/// A paint pass's patch, staged until the patch it buffered is applied.
#[derive(Debug, Default)]
pub(super) struct StagedScene {
    /// Records replaced (`Some`) or dropped (`None`).
    pub(super) records: FxHashMap<RenderId, Option<BoundaryRecord>>,
    /// New placements of records the pass kept.
    placed: FxHashMap<RenderId, Placed>,
    /// Records the residue scan found owing a paint.
    stale: Vec<RenderId>,
    frame: Option<FrameRecord>,
    /// Nodes of removed boundaries still named by a parent the pass did not
    /// republish.
    orphans: Vec<SceneNodeId>,
    /// Operations the pass buffered.
    pub(super) ops: u64,
}

impl<Phase: crate::pipeline::PipelinePhase> PipelineOwner<Phase> {
    /// Takes in what the patch this owner's last paint buffered became: the
    /// version it was applied as (ADR-0304 §8).
    ///
    /// Commits the paint's records, then places followers for hit-testing
    /// and input regions and reads the leaders for watches off `version`.
    /// A patch never applied is never reported: the next paint finds it
    /// staged still, drops every record, and repaints from the root, as it
    /// does after a refused patch renews the owner's lineage (ADR-0304 §10).
    ///
    /// Returns the version the watches held before, which the caller drops
    /// once it holds no borrow of this owner: its annotations are user
    /// values whose `Drop` is user code.
    #[must_use = "the version this one replaces drops its annotations, user code: drop it once no pipeline borrow is held"]
    pub fn scene_applied(
        &mut self,
        version: &flui_layer::SceneVersion,
    ) -> Option<flui_layer::SceneVersion> {
        let staged = self.staged_scene.take()?;
        let StagedScene {
            mut records,
            mut placed,
            mut stale,
            frame,
            orphans,
            ops: _,
        } = staged;
        for (id, record) in records.drain() {
            if let Some(record) = record {
                if record.followers.is_empty() {
                    self.follower_boundaries.remove(&id);
                } else {
                    self.follower_boundaries.insert(id);
                }
                self.boundary_records.insert(id, record);
            } else {
                self.follower_boundaries.remove(&id);
                self.boundary_records.remove(&id);
            }
        }
        for (id, placed) in placed.drain() {
            if let Some(record) = self.boundary_records.get_mut(&id) {
                record.placed = placed;
            }
        }
        for id in stale.drain(..) {
            if let Some(record) = self.boundary_records.get_mut(&id) {
                record.stale = true;
            }
        }
        let previous_orphans = std::mem::replace(&mut self.orphaned_nodes, orphans);
        if let Some(scratch) = self.paint_scratch.as_mut() {
            scratch.recycle_staged(records, placed, stale, previous_orphans);
        }
        if let Some(frame) = frame {
            self.scene_frame = Some(frame);
            // A patch with a frame retired every abandoned node it could
            // still find.
            self.abandoned_nodes.clear();
        }

        // Follower hit-testing: each follower's composite-resolved transform,
        // read off the applied version the engine resolves against, one
        // algorithm for pixels and hit-tests.
        self.last_follower_transforms.clear();
        self.last_hidden_follower_ids.clear();
        let frame_followers = self.scene_frame.iter().flat_map(|frame| {
            frame
                .followers
                .iter()
                .map(|follower| (frame.node, follower))
        });
        let boundary_followers = self.follower_boundaries.iter().filter_map(|id| {
            let record = self.boundary_records.get(id)?;
            Some(
                record
                    .followers
                    .iter()
                    .map(|follower| (record.node, follower)),
            )
        });
        for (node, &(render_id, entry, origin)) in
            frame_followers.chain(boundary_followers.flatten())
        {
            match version.follower_transform(flui_layer::EntryAddress { node, entry }) {
                // The subtree is recorded with its accumulated origin baked
                // in and the engine applies `transform` outside it, while the
                // hit-test walk and input regions have already subtracted that
                // origin: conjugating by it gives them the mapping paint
                // applies.
                Some(transform) => {
                    self.last_follower_transforms
                        .insert(render_id, conjugate(transform, -origin));
                }
                None => {
                    self.last_hidden_follower_ids.insert(render_id);
                }
            }
        }
        // Queued, not run: listeners run in the post-frame lane.
        self.leader_watches.check(version)
    }

    /// Gives up a patch that was never applied: its contents go to `writer`,
    /// its collections back to the scratch.
    fn discard_staged(&mut self, staged: StagedScene, writer: &mut SceneWriter<'_>) {
        let StagedScene {
            mut records,
            placed,
            stale,
            orphans,
            ..
        } = staged;
        for record in records.drain().filter_map(|(_, record)| record) {
            writer.discard(record.content);
        }
        if let Some(scratch) = self.paint_scratch.as_mut() {
            scratch.recycle_staged(records, placed, stale, orphans);
        }
    }

    /// The node this owner publishes its frame at, which the scene's root
    /// content slots; `None` before its first paint was applied.
    #[must_use]
    pub fn scene_frame_node(&self) -> Option<flui_layer::SceneNodeId> {
        self.staged_scene
            .as_ref()
            .and_then(|staged| staged.frame.as_ref())
            .or(self.scene_frame.as_ref())
            .map(FrameRecord::node)
    }

    /// Whether the last paint buffered a patch not reported as applied yet.
    #[must_use]
    pub fn scene_staged(&self) -> bool {
        self.staged_scene.is_some()
    }

    /// Drops every record of the scene: the next paint repaints from the
    /// root into new nodes.
    ///
    /// The nodes the records named stay owed a retirement: the scene the
    /// writer patches may still hold them, and only a `Retire` frees a node.
    pub(super) fn forget_scene(&mut self) {
        let frame = self.scene_frame.take().map(|frame| frame.node);
        let boundaries = self.boundary_records.drain().map(|(_, record)| record.node);
        self.abandoned_nodes
            .extend(frame.into_iter().chain(boundaries));
        self.abandoned_nodes.append(&mut self.orphaned_nodes);
        self.follower_boundaries.clear();
        self.staged_scene = None;
    }
}

/// The effect entries one node pushed in a content, and the origin it
/// painted at.
#[derive(Clone, Debug)]
struct EffectSlots {
    indices: SmallVec<[EntryIndex; 3]>,
    origin: Offset,
}

/// Adds to `layers` the layers a node pushes for its OWN effects, in push order (outermost
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
fn own_effect_layers(effects: PaintEffects, origin: Offset, layers: &mut impl Extend<Layer>) {
    if let Some(opacity) = effects.opacity {
        layers.extend(Some(Layer::Opacity(OpacityLayer::with_offset(
            opacity.alpha(),
            Offset::ZERO,
        ))));
    }
    if let Some(clip) = effects.clip {
        layers.extend(Some(clip_layer(clip, origin)));
    }
    if let Some(matrix) = effects.transform {
        // Same math and same reason as the per-child `PushTransform` fragment
        // op (RenderFlow and friends).
        layers.extend(Some(Layer::Transform(TransformLayer::new(conjugate(
            matrix, origin,
        )))));
    }
}

/// The nearest paint anchor as the walk carries it down: where the current
/// layer space's origin sits in the anchor's coordinates, and the anchor's
/// size.
///
/// A node at `origin` in the layer space is at `base + origin` from the
/// anchor. Inline children share the layer space and the frame; a repaint
/// boundary starts a layer space at its own origin, so its subtree gets the
/// frame [`rebased`](Self::rebased) there.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AnchorFrame {
    base: Offset,
    size: Size,
    /// `true` while the anchor is a node of the repaint-boundary region being
    /// painted, `false` when the region inherited it from outside. A capture
    /// stores the frame only in the first case.
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

/// The paint queue as one pass reads it.
struct PaintInputs<'a> {
    /// Every queued node: a boundary absent from it is clean.
    dirty_set: &'a FxHashSet<RenderId>,
    /// Boundaries queued only for composited-layer updates, with the nodes that
    /// asked.
    layer_updates: &'a FxHashMap<RenderId, SmallVec<[RenderId; 2]>>,
    /// Whether the frame node is repainted this pass.
    frame_dirty: bool,
}

impl PaintInputs<'_> {
    /// Whether the boundary `id` is queued for a repaint rather than only a
    /// composited-layer update.
    fn repaints(&self, id: RenderId) -> bool {
        self.dirty_set.contains(&id) && !self.layer_updates.contains_key(&id)
    }
}

/// One boundary's content while its paint is replayed into it.
#[derive(Debug)]
struct BoundaryBuild {
    /// The boundary, `None` for the frame node.
    boundary: Option<RenderId>,
    /// Which content of the pass this is: the key its render objects'
    /// ordinals and effect slots are filed under in the composer.
    serial: u64,
    builder: ContentBuilder,
    open: DisplayList,
    /// The render object whose runs `open` holds, `None` while it is empty: a
    /// chunk holds one render object's output (ADR-0304 §2), so a run of
    /// another seals it first.
    open_owner: Option<RenderId>,
    effect_slots: Vec<(RenderId, EffectSlots)>,
    followers: SmallVec<[(RenderId, EntryIndex, Offset); 1]>,
    children: SmallVec<[(RenderId, SceneNodeId); 4]>,
}

impl BoundaryBuild {
    fn new(boundary: Option<RenderId>, serial: u64) -> Self {
        Self {
            boundary,
            serial,
            builder: BoundaryContent::builder(),
            open: DisplayList::new(),
            open_owner: None,
            effect_slots: Vec::new(),
            followers: SmallVec::new(),
            children: SmallVec::new(),
        }
    }

    /// The record of the content built, published at `node`.
    fn into_record(self, node: SceneNodeId, placed: Placed) -> BoundaryRecord {
        BoundaryRecord {
            node,
            content: Arc::new(self.builder.finish()),
            placed,
            effect_slots: (!self.effect_slots.is_empty()).then(|| Arc::from(self.effect_slots)),
            followers: self.followers,
            children: self.children,
            stale: false,
        }
    }
}

/// The collections a paint pass fills and empties, kept by the owner across
/// frames so a pass allocates only when it outgrows them.
///
/// A pass takes them out of the owner and puts them back once it committed;
/// one that fails or unwinds leaves empty ones behind, which serve the next
/// pass the same. Each is cleared when it is taken, so what a failed pass
/// left in one is never read.
#[derive(Debug, Default)]
pub(super) struct PaintScratch {
    /// The queued nodes, as a set.
    dirty_ids: FxHashSet<RenderId>,
    /// The boundaries queued only for composited-layer updates.
    layer_updates: FxHashMap<RenderId, SmallVec<[RenderId; 2]>>,
    composer: ComposerScratch,
}

impl PaintScratch {
    /// Empties every collection for the next pass, dropping one this pass
    /// grew past [`KEPT_CAPACITY`]: clearing a hash table costs its capacity,
    /// so a large frame must not make every later small one pay for it.
    fn recycle(&mut self) {
        recycle_set(&mut self.dirty_ids);
        recycle_map(&mut self.layer_updates);
        let composer = &mut self.composer;
        recycle_vec(&mut composer.builds);
        recycle_vec(&mut composer.publishes);
        recycle_vec(&mut composer.places);
        recycle_set(&mut composer.served);
        recycle_set(&mut composer.slotted);
        recycle_vec(&mut composer.child_paint_orders);
        recycle_vec(&mut composer.consumed_updates);
        recycle_set(&mut composer.visited);
        recycle_map(&mut composer.ordinals);
        recycle_map(&mut composer.effect_owner);
        recycle_vec(&mut composer.layer_patches);
        recycle_set(&mut composer.republished);
        recycle_vec(&mut composer.retired);
        recycle_set(&mut composer.retired_set);
        recycle_vec(&mut composer.unslotted);
        recycle_map(&mut composer.records);
        recycle_map(&mut composer.placed);
        recycle_vec(&mut composer.stale);
        recycle_vec(&mut composer.orphans);
        recycle_vec(&mut composer.own_effects);
        recycle_vec(composer.recorder.0.ops_mut());
    }

    /// Hands back the maps a staged patch held, for the next pass to stage
    /// into.
    fn recycle_staged(
        &mut self,
        mut records: FxHashMap<RenderId, Option<BoundaryRecord>>,
        mut placed: FxHashMap<RenderId, Placed>,
        mut stale: Vec<RenderId>,
        mut orphans: Vec<SceneNodeId>,
    ) {
        recycle_map(&mut records);
        recycle_map(&mut placed);
        recycle_vec(&mut stale);
        recycle_vec(&mut orphans);
        let composer = &mut self.composer;
        composer.records = records;
        composer.placed = placed;
        composer.stale = stale;
        composer.orphans = orphans;
    }
}

/// The most entries a collection [`PaintScratch`] keeps between passes may
/// have room for.
const KEPT_CAPACITY: usize = 1024;

/// The most fragment operations the pass's shared buffer keeps room for
/// between passes (an operation is 176 bytes): enough for the path down a
/// deep tree, not the child list of a long column, which the next pass
/// allocates again.
const KEPT_FRAGMENT_OPS: usize = 64;

fn recycle_map<K, V>(map: &mut FxHashMap<K, V>) {
    if map.capacity() > KEPT_CAPACITY {
        *map = FxHashMap::default();
    } else {
        map.clear();
    }
}

fn recycle_set<K>(set: &mut FxHashSet<K>) {
    if set.capacity() > KEPT_CAPACITY {
        *set = FxHashSet::default();
    } else {
        set.clear();
    }
}

fn recycle_vec<T>(vec: &mut Vec<T>) {
    if vec.capacity() > KEPT_CAPACITY {
        *vec = Vec::new();
    } else {
        vec.clear();
    }
}

/// What [`FragmentComposer`] works in, between passes.
#[derive(Debug, Default)]
struct ComposerScratch {
    builds: Vec<BoundaryBuild>,
    publishes: Vec<(SceneNodeId, Arc<BoundaryContent>)>,
    places: Vec<(SceneNodeId, Offset)>,
    served: FxHashSet<RenderId>,
    slotted: FxHashSet<SceneNodeId>,
    child_paint_orders: Vec<(RenderId, Option<ChildPaintOrder>)>,
    consumed_updates: Vec<RenderId>,
    visited: FxHashSet<RenderId>,
    ordinals: FxHashMap<(u64, RenderId), (u32, u32)>,
    effect_owner: FxHashMap<(u64, RenderId), usize>,
    layer_patches: Vec<(EntryIndex, Layer)>,
    republished: FxHashSet<SceneNodeId>,
    retired: Vec<SceneNodeId>,
    retired_set: FxHashSet<SceneNodeId>,
    unslotted: Vec<RenderId>,
    /// A staged patch's maps, handed back by `scene_applied`.
    records: FxHashMap<RenderId, Option<BoundaryRecord>>,
    placed: FxHashMap<RenderId, Placed>,
    stale: Vec<RenderId>,
    /// The orphan list the owner held before the last applied patch
    /// replaced it, emptied, for the next pass to collect into.
    orphans: Vec<SceneNodeId>,
    /// The recorder every node of the pass records into, in turn: its
    /// buffer holds the fragments of the nodes on the path being painted,
    /// each after its parent's, and its canvas keeps the last shader-free
    /// paints it interned, from node to node and pass to pass. A node whose
    /// paint panics leaves it a fresh canvas.
    recorder: PassRecorder,
    /// The layers of the node being painted for its own effects, collected
    /// inside its paint's containment and pushed before its children paint:
    /// empty between nodes.
    own_effects: Vec<Layer>,
}

/// The recorder a pass reuses from node to node, in place: moving it per node
/// would copy its canvas each time.
#[derive(Debug)]
struct PassRecorder(FragmentRecorder);

impl Default for PassRecorder {
    fn default() -> Self {
        Self(FragmentRecorder::new(Offset::ZERO, 1.0))
    }
}

impl ComposerScratch {
    /// Empties every collection; each was emptied when the last pass that
    /// used it ended, so this costs nothing unless that pass failed.
    fn clear(&mut self) {
        self.builds.clear();
        self.publishes.clear();
        self.places.clear();
        self.served.clear();
        self.slotted.clear();
        self.child_paint_orders.clear();
        self.consumed_updates.clear();
        self.visited.clear();
        self.ordinals.clear();
        self.effect_owner.clear();
        self.layer_patches.clear();
        self.republished.clear();
        self.retired.clear();
        self.retired_set.clear();
        self.unslotted.clear();
        self.records.clear();
        self.placed.clear();
        self.stale.clear();
        self.recorder.0.reset();
        self.own_effects.clear();
    }
}

/// What a paint pass leaves for `run_paint` to commit.
struct PaintOutcome {
    staged: StagedScene,
    counts: PaintCounts,
}

/// Replays paint fragments into the contents of the boundaries a pass
/// paints, and buffers the patch that publishes them (ADR-0304 §3, §8).
///
/// Sealing discipline mirrors the recorder's: the open run is sealed into a
/// [`Chunk`](flui_layer::Chunk) whenever an effect or a slot needs ordering, whenever the
/// render object painting the next run changes, and when the boundary's
/// content ends. A chunk therefore holds one render object's output
/// (ADR-0304 §2), and a render object that repaints changes only its own
/// chunks.
///
/// Operations are buffered here and handed to the writer when the pass ends,
/// in the order the owner applies them: every content child first, then
/// placements, then retirements.
struct FragmentComposer<'w, 'o, 's> {
    writer: &'w mut SceneWriter<'o>,
    /// The pass's working collections, which the owner keeps between passes.
    work: &'s mut ComposerScratch,
    /// The serial the next content begun takes.
    next_serial: u64,
    staged: StagedScene,
    /// Set when the owner's sequence ran out for a chunk: the patch is then
    /// refused by `apply` too, which renews the lineage (ADR-0304 §10).
    exhausted: bool,
    /// Nodes painted and layers created or kept this pass, folded into the
    /// owner's `PipelineCounters` by `run_paint` on the commit path only.
    counts: PaintCounts,
}

impl<'w, 'o, 's> FragmentComposer<'w, 'o, 's> {
    fn new(writer: &'w mut SceneWriter<'o>, work: &'s mut ComposerScratch) -> Self {
        work.clear();
        let staged = StagedScene {
            records: std::mem::take(&mut work.records),
            placed: std::mem::take(&mut work.placed),
            stale: std::mem::take(&mut work.stale),
            orphans: std::mem::take(&mut work.orphans),
            ..StagedScene::default()
        };
        Self {
            writer,
            work,
            next_serial: 0,
            staged,
            exhausted: false,
            counts: PaintCounts::default(),
        }
    }

    fn note_painted(&mut self) {
        self.counts.nodes_painted += 1;
    }

    fn build(&mut self) -> &mut BoundaryBuild {
        self.work
            .builds
            .last_mut()
            .expect("BUG: paint replays only inside a boundary's content")
    }

    /// The boundary whose content is being built, `None` for the frame node.
    fn current_boundary(&self) -> Option<RenderId> {
        self.work.builds.last().and_then(|build| build.boundary)
    }

    fn begin_boundary(&mut self, boundary: Option<RenderId>) {
        let serial = self.next_serial;
        self.next_serial = serial
            .checked_add(1)
            .expect("BUG: a pass begins fewer than 2^64 contents");
        self.work.builds.push(BoundaryBuild::new(boundary, serial));
    }

    /// Ends the innermost content, its open run sealed.
    fn end_boundary(&mut self) -> BoundaryBuild {
        self.seal_picture();
        self.work
            .builds
            .pop()
            .expect("BUG: a boundary's content ends after it began")
    }

    fn served(&self, id: RenderId) -> bool {
        self.work.served.contains(&id)
    }

    fn mark_served(&mut self, id: RenderId) {
        self.work.served.insert(id);
    }

    /// The node of a boundary served this pass.
    fn node_of(&self, owner: &PipelineOwner<PaintPhase>, id: RenderId) -> Option<SceneNodeId> {
        match self.staged.records.get(&id) {
            Some(Some(record)) => Some(record.node),
            Some(None) => None,
            None => owner.boundary_records.get(&id).map(|record| record.node),
        }
    }

    fn stage_record(&mut self, id: RenderId, record: BoundaryRecord) {
        self.staged.placed.remove(&id);
        self.staged.records.insert(id, Some(record));
    }

    /// The placement a repaint staged for `id` this pass, if any.
    fn staged_placed(&self, id: RenderId) -> Option<Placed> {
        match self.staged.records.get(&id) {
            Some(Some(record)) => Some(record.placed),
            _ => self.staged.placed.get(&id).copied(),
        }
    }

    fn stage_placed(&mut self, id: RenderId, placed: Placed) {
        match self.staged.records.get_mut(&id) {
            Some(Some(record)) => record.placed = placed,
            _ => {
                self.staged.placed.insert(id, placed);
            }
        }
    }

    fn mint_node(&mut self) -> SceneNodeId {
        self.writer.mint_node()
    }

    fn publish(&mut self, node: SceneNodeId, content: Arc<BoundaryContent>) {
        self.work.publishes.push((node, content));
    }

    fn place(&mut self, node: SceneNodeId, placement: Offset) {
        self.work.places.push((node, placement));
    }

    /// Slots the child boundary `id`, published at `node`, where the
    /// innermost content stands.
    fn slot(&mut self, id: RenderId, node: SceneNodeId) {
        self.seal_picture();
        let build = self.build();
        build.builder.slot(node);
        build.children.push((id, node));
        self.work.slotted.insert(node);
    }

    /// Merges a sealed run of `owner`'s paint while keeping its canvas clips
    /// local to that run; a run of another render object seals the open
    /// chunk first.
    fn append_run(&mut self, owner: RenderId, run: DisplayList) {
        // An empty run draws nothing, so it seals nothing; it is still
        // appended for the refusal counts it carries.
        if !run.is_empty() && self.build().open_owner != Some(owner) {
            self.seal_picture();
            self.build().open_owner = Some(owner);
        }
        self.build().open.append_isolated(run);
    }

    fn seal_picture(&mut self) {
        let Some(build) = self.work.builds.last_mut() else {
            return;
        };
        let owner = build.open_owner.take();
        if build.open.is_empty() {
            return;
        }
        let mut list = std::mem::take(&mut build.open);
        let Some(owner) = owner else {
            debug_assert!(
                false,
                "BUG: a non-empty run has the render object that painted it"
            );
            return;
        };
        let Ok(version) = self.writer.next_content_version() else {
            // The patch cannot be applied either; its refusal renews the
            // lineage and the next frame repaints in full.
            self.exhausted = true;
            return;
        };
        let build = self
            .work
            .builds
            .last_mut()
            .expect("BUG: the content sealed is still open");
        let ordinal = &mut self
            .work
            .ordinals
            .entry((build.serial, owner))
            .or_default()
            .1;
        // Written into a chunk the scene owner reclaimed when it holds one,
        // whose emptied buffer the canvas records the next run into.
        let chunk = self.writer.chunk(owner, *ordinal, version, &mut list);
        *ordinal += 1;
        build.builder.chunk(chunk);
        self.counts.layers_produced += 1;
        self.work.recorder.0.record_into(list);
    }

    /// Pushes `layer`, an effect `owner` painted, and opens it.
    fn push_layer(&mut self, owner: RenderId, layer: Layer) -> EntryIndex {
        self.seal_picture();
        let build = self
            .work
            .builds
            .last_mut()
            .expect("BUG: paint replays only inside a boundary's content");
        let ordinal = &mut self
            .work
            .ordinals
            .entry((build.serial, owner))
            .or_default()
            .0;
        let index = build.builder.push_effect(layer, owner, *ordinal);
        *ordinal += 1;
        self.counts.layers_produced += 1;
        index
    }

    fn pop_layer(&mut self) {
        self.seal_picture();
        let closed = self.build().builder.pop_effect();
        debug_assert!(
            closed.is_some(),
            "composer pop without matching push — fragment scope ops are \
             balanced by the recorder, so an underflow means the replay \
             loop pushed/popped asymmetrically",
        );
    }

    /// Files the effect at `index` as one of `render_id`'s own, built at
    /// `origin`, for a later composited-layer update.
    fn record_effect_layer(&mut self, render_id: RenderId, index: EntryIndex, origin: Offset) {
        let build = self
            .work
            .builds
            .last_mut()
            .expect("BUG: paint replays only inside a boundary's content");
        if let Some(&at) = self.work.effect_owner.get(&(build.serial, render_id)) {
            build.effect_slots[at].1.indices.push(index);
        } else {
            self.work
                .effect_owner
                .insert((build.serial, render_id), build.effect_slots.len());
            let mut indices = SmallVec::new();
            indices.push(index);
            build
                .effect_slots
                .push((render_id, EffectSlots { indices, origin }));
        }
    }

    /// Notes the follower effect at `index`, pushed by `render_id` painting at
    /// `origin`, to place for hit-testing once the patch is applied.
    fn record_follower_correlation(
        &mut self,
        render_id: RenderId,
        index: EntryIndex,
        origin: Offset,
    ) {
        self.build().followers.push((render_id, index, origin));
    }

    /// Ends the frame node's content and publishes it.
    fn finish_frame(
        &mut self,
        owner: &PipelineOwner<PaintPhase>,
        root: RenderId,
        root_is_boundary: bool,
    ) {
        let build = self.end_boundary();
        let lineage = self.writer.current().id().lineage();
        let node = match &owner.scene_frame {
            Some(frame) if frame.node.lineage() == lineage => frame.node,
            _ => self.mint_node(),
        };
        self.publish(node, Arc::new(build.builder.finish()));
        self.staged.frame = Some(FrameRecord {
            node,
            root,
            root_is_boundary,
            followers: build.followers,
            children: build.children,
        });
    }

    /// Retires what no content names any longer, hands the buffered patch to
    /// the writer, and returns what the owner commits; the pass's consumed
    /// updates, reached nodes and child paint orders stay in its working
    /// collections.
    fn finish(mut self, owner: &PipelineOwner<PaintPhase>, lineage: Lineage) -> PaintOutcome {
        debug_assert!(
            self.work.builds.is_empty(),
            "BUG: a boundary's content was left open"
        );
        let mut republished = std::mem::take(&mut self.work.republished);
        let mut retired = std::mem::take(&mut self.work.retired);
        let mut retired_set = std::mem::take(&mut self.work.retired_set);
        // The children a republished content no longer slots, with every
        // boundary recorded under them: unplaced by their parent's latest
        // layout, or moved to a parent that did not slot them this pass.
        let mut unslotted = std::mem::take(&mut self.work.unslotted);
        republished.clear();
        retired.clear();
        retired_set.clear();
        unslotted.clear();
        republished.extend(self.work.publishes.iter().map(|(node, _)| *node));
        let old_children = |id: &Option<RenderId>| -> &[(RenderId, SceneNodeId)] {
            match id {
                Some(id) => owner
                    .boundary_records
                    .get(id)
                    .map_or(&[][..], |record| &record.children[..]),
                None => owner
                    .scene_frame
                    .as_ref()
                    .map_or(&[][..], |frame| &frame.children[..]),
            }
        };
        let mut republished_boundaries: SmallVec<[Option<RenderId>; 8]> = self
            .staged
            .records
            .iter()
            .filter_map(|(id, record)| record.as_ref().map(|_| Some(*id)))
            .collect();
        if self.staged.frame.is_some() {
            republished_boundaries.push(None);
        }
        for boundary in &republished_boundaries {
            for &(child, node) in old_children(boundary) {
                if !self.work.slotted.contains(&node) && retired_set.insert(node) {
                    retired.push(node);
                    unslotted.push(child);
                }
            }
        }
        while let Some(id) = unslotted.pop() {
            let below: SmallVec<[(RenderId, SceneNodeId); 4]> = match self.staged.records.get(&id) {
                Some(Some(record)) => record.children.clone(),
                Some(None) => SmallVec::new(),
                None => owner
                    .boundary_records
                    .get(&id)
                    .map_or_default(|record| record.children.clone()),
            };
            self.staged.records.insert(id, None);
            for (child, node) in below {
                if !self.work.slotted.contains(&node) && retired_set.insert(node) {
                    retired.push(node);
                    unslotted.push(child);
                }
            }
        }
        // Nodes of a scene whose records the owner dropped: retired while the
        // version this patch applies to still holds them. The frame this pass
        // published replaces the one the host slotted; a pass without one
        // leaves them for the pass that has.
        let current = self.writer.current();
        let abandoned = if self.staged.frame.is_some() {
            &owner.abandoned_nodes[..]
        } else {
            &[]
        };
        for &node in abandoned {
            if node.lineage() == lineage
                && current.node(node).is_some()
                && !self.work.slotted.contains(&node)
                && retired_set.insert(node)
            {
                retired.push(node);
            }
        }
        // Removed boundaries: retired once no unchanged parent names them.
        let mut orphans = std::mem::take(&mut self.staged.orphans);
        orphans.clear();
        orphans.extend(
            owner
                .orphaned_nodes
                .iter()
                .copied()
                .filter(|node| node.lineage() == lineage && current.node(*node).is_some()),
        );
        loop {
            let before = orphans.len();
            orphans.retain(|&node| {
                if retired_set.contains(&node) {
                    return false;
                }
                let parent = current.node(node).and_then(flui_layer::SceneNode::parent);
                let free = parent.is_none_or(|parent| {
                    republished.contains(&parent) || retired_set.contains(&parent)
                });
                if free && !self.work.slotted.contains(&node) {
                    retired_set.insert(node);
                    retired.push(node);
                    false
                } else {
                    true
                }
            });
            if orphans.len() == before {
                break;
            }
        }
        self.staged.orphans = orphans;

        let mut ops = 0u64;
        for (node, content) in self.work.publishes.drain(..) {
            self.writer.push(SceneOp::Publish { node, content });
            ops += 1;
        }
        for (node, offset) in self.work.places.drain(..) {
            let placement = Placement::try_new(offset)
                .expect("BUG: a boundary is placed only at a finite offset");
            self.writer.push(SceneOp::Place { node, placement });
            ops += 1;
        }
        for node in retired.drain(..) {
            self.writer.push(SceneOp::Retire { node });
            ops += 1;
        }
        if self.exhausted {
            tracing::warn!("paint: scene content versions exhausted; the patch will be refused");
        }
        self.work.republished = republished;
        self.work.retired = retired;
        self.work.retired_set = retired_set;
        self.work.unslotted = unslotted;
        self.staged.ops = ops;
        let staged = std::mem::take(&mut self.staged);
        let counts = self.counts;
        PaintOutcome { staged, counts }
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

/// A backdrop filter recorded in a node's local space, moved into the space
/// of a layer that draws the node at `origin`: a transform filter's focal
/// point stays where the node put it. A move that leaves the transform
/// non-finite keeps the filter as recorded, so nothing non-finite reaches
/// the scene.
fn backdrop_filter_at(
    filter: flui_painting::paint::ImageFilter,
    origin: Offset,
) -> flui_painting::paint::ImageFilter {
    if origin == Offset::ZERO {
        return filter;
    }
    match filter.try_translated(origin) {
        Ok(moved) => moved,
        Err(error) => {
            tracing::warn!(?origin, %error, "a backdrop filter's transform could not follow its node");
            filter
        }
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
/// canvas clips inside a picture is a composer-side optimization
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
            BackdropFilterLayer::new(
                backdrop_filter_at(filter, origin),
                blend_mode,
                bounds.translate_offset(origin),
            )
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
        // `Layer::Follower` carries no resolved transform at all — its
        // anchors and offsets are inputs to the composite-time
        // `flui_layer::resolve_follower_transform`. `target_offset` is
        // recorded as-authored (not origin-shifted), like `size`.
        FragmentScope::Follower(follower) => Layer::Follower(follower),
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

/// The children a parent painted, in the order it painted them.
pub(super) type ChildPaintOrder = SmallVec<[PaintedChild; 4]>;

/// One child a parent painted: its slot, and the product of the group opacities of the
/// fragment scopes the parent had open around it (`1.0` outside any).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PaintedChild {
    pub(super) slot: usize,
    pub(super) opacity: f64,
}

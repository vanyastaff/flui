//! Paint phase implementation for `PipelineOwner<PaintPhase>`.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_layer::{Effect, EntryIndex, OffsetLayer, SceneWriter, TransformLayer};
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{
    context::{FragmentOp, FragmentScope, PaintAnchor},
    error::PoisonPhase,
    pipeline::{
        phase::{Idle, PaintPhase, Semantics},
        scheduler::PhaseKind,
    },
};

use super::{PipelineOwner, rebind_phase, subtree_arena::ensure_stack};

mod composer;
mod effect_layers;
mod kept_chunks;
mod records;
mod scratch;

use composer::{FragmentComposer, PaintOutcome};
use effect_layers::{conjugate, own_effect_layers, scope_layer};
use kept_chunks::PaintedSlots;
pub(super) use records::{BoundaryRecord, FrameRecord, StagedScene};
use records::{Placed, Refreshed};
pub(super) use scratch::PaintScratch;

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
    /// [`BoundaryContent`](flui_layer::BoundaryContent). Adjacent inline draw runs of one render object
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
    /// reported, a renewed lineage, a new pixel ratio or a pass with no root
    /// to walk makes the next pass with a root repaint from it.
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
        let owed = self.root_id.is_some()
            && (self.staged_scene.is_some()
                || self.retained_pixel_ratio != self.device_pixel_ratio
                || self
                    .scene_frame
                    .as_ref()
                    .is_none_or(|frame| frame.node.lineage() != writer.current().id().lineage()));
        if !self.scheduler.has_paint_work() && !owed {
            // A patch still staged here was built for a root that is gone
            // (`owed` holds whenever a root and a staged patch coexist). It
            // describes nothing this frame publishes, so a later report must
            // not commit it; the records it was built against are forgotten
            // with it, and the next root repaints from scratch.
            if let Some(staged) = self.staged_scene.take() {
                self.discard_staged(staged, writer);
                self.forget_scene();
            }
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
        // A pass with no root to walk drops what is queued (the residue scan
        // below), so the records stop describing the tree: forgotten, they
        // make the next root's first pass repaint from scratch, with every
        // change made while the root was gone.
        let rootless = self
            .root_id
            .is_none_or(|root| self.render_tree.get(root).is_none());
        if ratio_changed || unapplied || renewed || rootless {
            self.forget_scene();
            self.retained_pixel_ratio = self.device_pixel_ratio;
        }

        // Deepest first, the order ADR-0304 §8 paints owed boundaries in.
        self.scheduler.sort_paint_deep_first();

        // A pass that failed or unwound cleared the flags of the nodes it
        // painted and committed none of their output, so until a pass commits,
        // a node found unmarked may still differ from its records and no chunk
        // keeps its recording (ADR-0304 §2).
        let keep_chunks = !self.unrecorded_paint;
        self.unrecorded_paint = true;

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
            let mut composer = FragmentComposer::new(writer, work, keep_chunks);
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

        self.unrecorded_paint = false;
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
        composer.begin_boundary(None, self.scene_frame.as_ref().map(|frame| &frame.content));
        let root_layer =
            if self.device_pixel_ratio == flui_foundation::geometry::DevicePixelRatio::ONE {
                Effect::from(OffsetLayer::zero())
            } else {
                Effect::from(TransformLayer::new(
                    flui_foundation::geometry::Matrix4::scaling(
                        self.device_pixel_ratio.get(),
                        self.device_pixel_ratio.get(),
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
            self.paint_subtree(composer, root_id, Offset::ZERO, None, true, inputs)
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
    ///
    /// `placed_as_before` says that the node is painted where the content's
    /// previous paint painted it: its content's root is, and so is a child
    /// whose parent was, laid out by no layout since, marked for paint
    /// itself by no mark, read no paint anchor, and paints it for the first
    /// time in the content (ADR-0304 §2).
    fn paint_subtree(
        &self,
        composer: &mut FragmentComposer<'_, '_, '_>,
        node_id: RenderId,
        origin: Offset,
        anchor: Option<AnchorFrame>,
        placed_as_before: bool,
        inputs: &PaintInputs<'_>,
    ) -> crate::error::RenderResult<()> {
        ensure_stack(|| {
            self.paint_subtree_impl(composer, node_id, origin, anchor, placed_as_before, inputs)
        })
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
        patches: &mut Vec<(EntryIndex, Effect)>,
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
            let mut fresh = SmallVec::<[Effect; 3]>::new();
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
                    node.kind(),
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
                if std::mem::discriminant(layer.as_layer()) != std::mem::discriminant(recorded) {
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
        placed_as_before: bool,
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

        // Read before the flags clear: a node that owes no paint work and is
        // painted where it was may keep the recordings of the chunks it paints
        // again (ADR-0304 §2). Layout marks paint on its dirty roots and
        // boundaries alone, so a node that laid out owes paint too. Both flags
        // are taken here, on every node reached, so they speak of what
        // happened since this paint; either one may have moved the node's
        // children.
        let laid_out = render_node.take_laid_out_since_paint();
        let paint_changed = render_node.take_paint_changed();
        let owes_paint = laid_out
            || paint_changed
            || render_node.needs_paint()
            || render_node.needs_composited_layer_update();
        let may_have_moved_children = laid_out || paint_changed;

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

        // Record the node's fragment. paint_raw sees ONLY the recorder and
        // the node's text source (sans-IO): no tree access, no layer access,
        // no recursion.
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
        let kind = render_node.kind();
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
        // The node's text source: a loan it takes while painting records it
        // beside the nodes layout recorded, so a font change repaints it.
        let text = crate::pipeline::TextLender::new(
            self.text.cell(),
            &self.text_measurers,
            &self.text_sizing,
        )
        .source(node_id);
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a render object's paint runs contained; a panic poisons the node"
        )]
        let painted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_node.paint_raw(recorder, text, child_ids.len());
            own_effect_layers(render_node.paint_effects(), origin, own_effects);
        }));
        if let Err(payload) = painted {
            composer.work.own_effects.clear();
            composer.work.recorder.0.abandon();
            flui_foundation::panic::discard_payload(payload);
            return Err(crate::error::RenderError::poisoned(
                kind,
                PoisonPhase::Paint,
            ));
        }
        let start = composer.work.recorder.0.end();
        let end = composer.work.recorder.0.ops_mut().len();
        composer.note_painted();
        // A paint that read its anchor recorded, and may have placed its
        // children, where the anchor is, which no flag of this node follows.
        // Read before the replay below, where each child's paint begins anew.
        let read_anchor = composer.work.recorder.0.anchor_read();
        let visit = composer.begin_visit(node_id, !owes_paint && placed_as_before && !read_anchor);
        let children_placed_as_before =
            placed_as_before && !may_have_moved_children && !read_anchor;

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
        // Which slots this paint painted, to tell a child painted again.
        let mut painted_slots = PaintedSlots::default();
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
                FragmentOp::Run(list) => composer.append_run(&visit, list),
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
                        Effect::from(TransformLayer::new(conjugate(*matrix, origin))),
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
                    // A second paint of a child in the content is not where
                    // its first one was, and neither is its subtree's.
                    let repeated = painted_slots.paint(
                        &mut composer.work.painted_slots,
                        child_ids.len(),
                        &painted_children,
                        index,
                    );
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
                            children_placed_as_before && !repeated,
                            inputs,
                        )?;
                    }
                }
            }
        }
        // Every child returned to `end`; what is left is this node's
        // replayed operations. A pass that fails above leaves the rest for
        // the next pass's `clear`.
        let work = &mut *composer.work;
        painted_slots.release(&mut work.painted_slots);
        let ops = work.recorder.0.ops_mut();
        work.fragment_peak = work.fragment_peak.max(ops.len());
        ops.truncate(start);

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
            let refreshed =
                self.refresh_boundary(composer, child_id, committed, region_anchor, inputs)?;
            let node = match &refreshed {
                Some(Refreshed::Repainted(fresh)) => Some(fresh.node),
                Some(Refreshed::Patched { .. }) | None => committed.map(|record| record.node),
            };
            if let Some(refreshed) = refreshed {
                composer.stage_record(child_id, refreshed);
            }
            node
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
    /// repainted, the patched content when its composited layers are
    /// updated, `None` when it stays as `committed` holds it.
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
    ) -> crate::error::RenderResult<Option<Refreshed>> {
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
                // The patched content keeps every slot of the one it replaces.
                composer
                    .work
                    .slotted
                    .extend(record.children.iter().map(|&(_, node)| node));
                composer.publish(record.node, Arc::clone(&content));
                Some(Refreshed::Patched {
                    content,
                    placed: record.placed,
                })
            });
            patches.clear();
            composer.work.layer_patches = patches;
            if let Some(fresh) = fresh {
                return Ok(fresh);
            }
        }
        // A stale record's paint was owed and never made: the residue scan
        // cleared the boundary's flag, so its chunks cannot be told apart
        // from clean ones and none is kept.
        let previous = committed
            .filter(|record| !record.stale)
            .map(|record| &record.content);
        composer.begin_boundary(Some(id), previous);
        let painted = self.paint_subtree(composer, id, Offset::ZERO, region_anchor, true, inputs);
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
        Ok(Some(Refreshed::Repainted(fresh)))
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

/// The children a parent painted, in the order it painted them.
pub(super) type ChildPaintOrder = SmallVec<[PaintedChild; 4]>;

/// One child a parent painted: its slot, and the product of the group opacities of the
/// fragment scopes the parent had open around it (`1.0` outside any).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PaintedChild {
    pub(super) slot: usize,
    pub(super) opacity: f64,
}

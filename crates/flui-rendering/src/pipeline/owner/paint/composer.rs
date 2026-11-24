//! The composer that replays paint fragments into the contents of the boundaries a pass paints,
//! and the outcome it hands to `run_paint`.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Offset;
use flui_layer::{
    BoundaryContent, ContentBuilder, Effect, EntryIndex, Lineage, Placement, SceneNodeId, SceneOp,
    SceneWriter,
};
use flui_painting::DisplayList;
use smallvec::SmallVec;

use super::super::{PipelineOwner, counters::PaintCounts};
use super::{
    effect_layers::EffectSlots,
    kept_chunks::PreviousChunks,
    records::{BoundaryRecord, FrameRecord, Placed, Refreshed, StagedScene},
    scratch::ComposerScratch,
};

/// One boundary's content while its paint is replayed into it.
#[derive(Debug)]
pub(super) struct BoundaryBuild {
    /// The boundary, `None` for the frame node.
    boundary: Option<RenderId>,
    /// Which content of the pass this is: the key its render objects'
    /// ordinals and effect slots are filed under in the composer.
    serial: u64,
    builder: ContentBuilder,
    open: DisplayList,
    /// The paint whose runs `open` holds, `None` while it is empty: a chunk
    /// holds the output of one paint of one render object (ADR-0304 §2), so
    /// a run of another seals it first.
    open_run: Option<PaintVisit>,
    effect_slots: Vec<(RenderId, EffectSlots)>,
    followers: SmallVec<[(RenderId, EntryIndex, Offset); 1]>,
    children: SmallVec<[(RenderId, SceneNodeId); 4]>,
    /// What the boundary published before, when its chunks may be kept.
    previous: Option<PreviousChunks>,
}

impl BoundaryBuild {
    fn new(boundary: Option<RenderId>, serial: u64, previous: Option<PreviousChunks>) -> Self {
        Self {
            boundary,
            serial,
            builder: BoundaryContent::builder(),
            open: DisplayList::new(),
            open_run: None,
            effect_slots: Vec::new(),
            followers: SmallVec::new(),
            children: SmallVec::new(),
            previous,
        }
    }

    /// The record of the content built, published at `node`.
    pub(super) fn into_record(self, node: SceneNodeId, placed: Placed) -> BoundaryRecord {
        BoundaryRecord {
            node,
            content: Arc::new(self.builder.finish()),
            placed,
            effect_slots: (!self.effect_slots.is_empty())
                .then(|| self.effect_slots.into_boxed_slice()),
            followers: self.followers,
            children: self.children,
            stale: false,
        }
    }
}

/// One paint of a render object in a content.
#[derive(Clone, Copy, Debug)]
pub(super) struct PaintVisit {
    /// Which paint of the pass it is.
    id: u64,
    owner: RenderId,
    /// Whether its chunks may keep their recordings (ADR-0304 §2): the
    /// render object owes no paint work, is painted where it was, and read
    /// no paint anchor.
    clean: bool,
}

/// What a paint pass leaves for `run_paint` to commit.
pub(super) struct PaintOutcome {
    pub(super) staged: StagedScene,
    pub(super) counts: PaintCounts,
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
pub(super) struct FragmentComposer<'w, 'o, 's> {
    pub(super) writer: &'w mut SceneWriter<'o>,
    /// The pass's working collections, which the owner keeps between passes.
    pub(super) work: &'s mut ComposerScratch,
    /// The serial the next content begun takes.
    next_serial: u64,
    staged: StagedScene,
    /// Set when the owner's sequence ran out for a chunk: the patch is then
    /// refused by `apply` too, which renews the lineage (ADR-0304 §10).
    exhausted: bool,
    /// Nodes painted and layers created or kept this pass, folded into the
    /// owner's `PipelineCounters` by `run_paint` on the commit path only.
    pub(super) counts: PaintCounts,
    /// Whether a chunk whose render object owes no paint work may keep its
    /// recording: false on a pass after one that did not commit.
    keep_chunks: bool,
    /// The id the next paint of a render object takes.
    next_visit: u64,
}

impl<'w, 'o, 's> FragmentComposer<'w, 'o, 's> {
    pub(super) fn new(
        writer: &'w mut SceneWriter<'o>,
        work: &'s mut ComposerScratch,
        keep_chunks: bool,
    ) -> Self {
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
            keep_chunks,
            next_visit: 1,
        }
    }

    pub(super) fn note_painted(&mut self) {
        self.counts.nodes_painted += 1;
    }

    /// Starts `owner`'s paint into the innermost content, `clean` when its
    /// chunks may keep their recordings.
    pub(super) fn begin_visit(&mut self, owner: RenderId, clean: bool) -> PaintVisit {
        let id = self.next_visit;
        self.next_visit = id
            .checked_add(1)
            .expect("BUG: a pass paints fewer than 2^64 render objects");
        PaintVisit { id, owner, clean }
    }

    fn build(&mut self) -> &mut BoundaryBuild {
        self.work
            .builds
            .last_mut()
            .expect("BUG: paint replays only inside a boundary's content")
    }

    /// The boundary whose content is being built, `None` for the frame node.
    pub(super) fn current_boundary(&self) -> Option<RenderId> {
        self.work.builds.last().and_then(|build| build.boundary)
    }

    /// Begins the content of `boundary`, `None` for the frame node, over
    /// the content it published before, whose unchanged chunks it keeps
    /// unless this pass keeps none.
    pub(super) fn begin_boundary(
        &mut self,
        boundary: Option<RenderId>,
        previous: Option<&Arc<BoundaryContent>>,
    ) {
        let serial = self.next_serial;
        self.next_serial = serial
            .checked_add(1)
            .expect("BUG: a pass begins fewer than 2^64 contents");
        let previous = previous
            .filter(|_| self.keep_chunks)
            .map(|content| PreviousChunks::new(serial, content));
        self.work
            .builds
            .push(BoundaryBuild::new(boundary, serial, previous));
    }

    /// Ends the innermost content, its open run sealed.
    pub(super) fn end_boundary(&mut self) -> BoundaryBuild {
        self.seal_picture();
        self.work
            .builds
            .pop()
            .expect("BUG: a boundary's content ends after it began")
    }

    pub(super) fn served(&self, id: RenderId) -> bool {
        self.work.served.contains(&id)
    }

    pub(super) fn mark_served(&mut self, id: RenderId) {
        self.work.served.insert(id);
    }

    /// The node of a boundary served this pass.
    pub(super) fn node_of(&self, owner: &PipelineOwner, id: RenderId) -> Option<SceneNodeId> {
        match self.staged.records.get(&id) {
            Some(Some(Refreshed::Repainted(record))) => Some(record.node),
            Some(None) => None,
            Some(Some(Refreshed::Patched { .. })) | None => {
                owner.boundary_records.get(&id).map(|record| record.node)
            }
        }
    }

    /// Stages what the pass made of `id`; the placement it carries, the
    /// committed one for a patched content, replaces any staged before.
    pub(super) fn stage_record(&mut self, id: RenderId, refreshed: Refreshed) {
        self.staged.placed.remove(&id);
        self.staged.records.insert(id, Some(refreshed));
    }

    /// The placement a repaint staged for `id` this pass, if any.
    pub(super) fn staged_placed(&self, id: RenderId) -> Option<Placed> {
        match self.staged.records.get(&id) {
            Some(Some(Refreshed::Repainted(record))) => Some(record.placed),
            Some(Some(Refreshed::Patched { placed, .. })) => Some(*placed),
            _ => self.staged.placed.get(&id).copied(),
        }
    }

    pub(super) fn stage_placed(&mut self, id: RenderId, placed: Placed) {
        match self.staged.records.get_mut(&id) {
            Some(Some(refreshed)) => *refreshed.placed_mut() = placed,
            _ => {
                self.staged.placed.insert(id, placed);
            }
        }
    }

    pub(super) fn mint_node(&mut self) -> SceneNodeId {
        self.writer.mint_node()
    }

    pub(super) fn publish(&mut self, node: SceneNodeId, content: Arc<BoundaryContent>) {
        self.work.publishes.push((node, content));
    }

    pub(super) fn place(&mut self, node: SceneNodeId, placement: Offset) {
        self.work.places.push((node, placement));
    }

    /// Slots the child boundary `id`, published at `node`, where the
    /// innermost content stands.
    pub(super) fn slot(&mut self, id: RenderId, node: SceneNodeId) {
        self.seal_picture();
        let build = self.build();
        build.builder.slot(node);
        build.children.push((id, node));
        self.work.slotted.insert(node);
    }

    /// Merges a sealed run of `visit`'s paint while keeping its canvas clips
    /// local to that run; a run of another paint seals the open chunk first,
    /// so a chunk's ordinal names its place in one paint of its owner.
    pub(super) fn append_run(&mut self, visit: &PaintVisit, run: DisplayList) {
        // An empty run draws nothing, so it seals nothing; it is still
        // appended for the refusal counts it carries.
        if !run.is_empty() && self.build().open_run.is_none_or(|open| open.id != visit.id) {
            self.seal_picture();
            self.build().open_run = Some(*visit);
        }
        self.build().open.append_isolated(run);
    }

    /// Seals the open run into a chunk: the one the boundary published
    /// before at the same owner and ordinal, when its paint may keep it
    /// (ADR-0304 §2), else a new recording.
    fn seal_picture(&mut self) {
        let Some(build) = self.work.builds.last_mut() else {
            return;
        };
        let open = build.open_run.take();
        if build.open.is_empty() {
            return;
        }
        let mut list = std::mem::take(&mut build.open);
        let Some(open) = open else {
            debug_assert!(
                false,
                "BUG: a non-empty run has the render object that painted it"
            );
            return;
        };
        let ordinal = &mut self
            .work
            .ordinals
            .entry((build.serial, open.owner))
            .or_default()
            .1;
        let lineage = self.writer.current().id().lineage();
        let kept = match &mut build.previous {
            Some(previous) if open.clean => self
                .work
                .chunk_index
                .kept(previous, open.owner, *ordinal, lineage),
            _ => None,
        };
        let chunk = if let Some(chunk) = kept {
            // The run just recorded is the kept chunk's commands again: its
            // buffer goes back to the canvas.
            list.clear();
            self.counts.layers_reused += 1;
            chunk
        } else {
            let Ok(version) = self.writer.next_content_version() else {
                // The patch cannot be applied either; its refusal renews the
                // lineage and the next frame repaints in full.
                self.exhausted = true;
                return;
            };
            self.counts.layers_produced += 1;
            // Written into a chunk the scene owner reclaimed when it holds
            // one, whose emptied buffer the canvas records the next run into.
            self.writer.chunk(open.owner, *ordinal, version, &mut list)
        };
        *ordinal += 1;
        build.builder.chunk(chunk);
        self.work.recorder.0.record_into(list);
    }

    /// Pushes `layer`, an effect `owner` painted, and opens it.
    pub(super) fn push_layer(&mut self, owner: RenderId, layer: Effect) -> EntryIndex {
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

    pub(super) fn pop_layer(&mut self) {
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
    pub(super) fn record_effect_layer(
        &mut self,
        render_id: RenderId,
        index: EntryIndex,
        origin: Offset,
    ) {
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
    pub(super) fn record_follower_correlation(
        &mut self,
        render_id: RenderId,
        index: EntryIndex,
        origin: Offset,
    ) {
        self.build().followers.push((render_id, index, origin));
    }

    /// Ends the frame node's content and publishes it.
    pub(super) fn finish_frame(
        &mut self,
        owner: &PipelineOwner,
        root: RenderId,
        root_is_boundary: bool,
    ) {
        let build = self.end_boundary();
        let lineage = self.writer.current().id().lineage();
        let node = match &owner.scene_frame {
            Some(frame) if frame.node.lineage() == lineage => frame.node,
            _ => self.mint_node(),
        };
        let content = Arc::new(build.builder.finish());
        self.publish(node, Arc::clone(&content));
        self.staged.frame = Some(FrameRecord {
            node,
            root,
            root_is_boundary,
            followers: build.followers,
            children: build.children,
            content,
        });
    }

    /// Retires what no content names any longer, hands the buffered patch to
    /// the writer, and returns what the owner commits; the pass's consumed
    /// updates, reached nodes and child paint orders stay in its working
    /// collections.
    pub(super) fn finish(mut self, owner: &PipelineOwner, lineage: Lineage) -> PaintOutcome {
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
                Some(Some(Refreshed::Repainted(record))) => record.children.clone(),
                Some(None) => SmallVec::new(),
                Some(Some(Refreshed::Patched { .. })) | None => owner
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
        self.staged.orphans_read = owner.orphaned_nodes.len();
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

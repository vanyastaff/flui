//! What the owner keeps for each repaint boundary it published, the frame record, and the scene a
//! paint pass stages for `run_paint` to commit.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Offset;
use flui_layer::{BoundaryContent, EntryIndex, SceneNodeId, SceneWriter};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::pipeline::phase::PipelinePhase;

use super::super::PipelineOwner;
use super::{
    AnchorFrame,
    effect_layers::{EffectSlots, conjugate},
};

/// What the owner keeps for each repaint boundary it published (ADR-0304 §8):
/// its node, the content it published there, and the values a walk from the
/// root would hand it at its slot, so a boundary painted alone paints as it
/// would under its parent.
#[derive(Debug)]
pub(in crate::pipeline::owner) struct BoundaryRecord {
    pub(super) node: SceneNodeId,
    pub(super) content: Arc<BoundaryContent>,
    pub(super) placed: Placed,
    /// Each node whose own effect layers this content holds, the entries of
    /// those layers, and the origin they were built at: what a
    /// composited-layer update rebuilds; `None` when there are none.
    pub(super) effect_slots: Option<Box<[(RenderId, EffectSlots)]>>,
    /// Each follower effect in the content, with the origin its subtree paints
    /// at, for hit-testing and input regions.
    pub(super) followers: SmallVec<[(RenderId, EntryIndex, Offset); 1]>,
    /// The boundaries the content slots.
    pub(super) children: SmallVec<[(RenderId, SceneNodeId); 4]>,
    /// Dropped by the residue scan: a paint was owed and never made, so the
    /// next frame that reaches the boundary repaints it into the same node.
    pub(super) stale: bool,
}

impl BoundaryRecord {
    /// The node the boundary is published at.
    pub(in crate::pipeline::owner) fn node(&self) -> SceneNodeId {
        self.node
    }

    /// Each node whose own effect layers the content holds, with their
    /// entries and origin.
    pub(super) fn effect_slots(&self) -> &[(RenderId, EffectSlots)] {
        self.effect_slots.as_deref().unwrap_or_default()
    }
}

/// What a paint pass made of a boundary it served: a record that replaces
/// the committed one, or the committed record's content patched by a
/// composited-layer update at its placement, which keeps every other field
/// of that record.
#[derive(Debug)]
pub(super) enum Refreshed {
    /// Painted afresh: the record replaces the committed one.
    Repainted(BoundaryRecord),
    /// The committed record, republished with `content` at `placed`: its
    /// node, effect slots, followers and children stay as they are.
    Patched {
        content: Arc<BoundaryContent>,
        placed: Placed,
    },
}

impl Refreshed {
    /// The content the pass published.
    fn into_content(self) -> Arc<BoundaryContent> {
        match self {
            Self::Repainted(record) => record.content,
            Self::Patched { content, .. } => content,
        }
    }

    /// Where the boundary's slot puts it.
    pub(super) fn placed_mut(&mut self) -> &mut Placed {
        match self {
            Self::Repainted(record) => &mut record.placed,
            Self::Patched { placed, .. } => placed,
        }
    }
}

/// Where a boundary's slot put it, as its parent's paint last placed it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placed {
    /// The parent's accumulated origin in its layer space at the slot.
    pub(super) origin: Offset,
    /// The offset the parent painted it at instead of its own, if any.
    pub(super) offset_override: Option<Offset>,
    /// The paint anchor in force at the slot when the anchor is a node of the
    /// parent's region; an inherited one is derived from the parent's record.
    pub(super) slot_anchor: Option<AnchorFrame>,
    /// The boundary that slots it, `None` for the frame node.
    pub(super) parent: Option<RenderId>,
    /// Its node's offset in the parent's content: `origin` plus its position.
    pub(super) placement: Offset,
}

impl Placed {
    /// A record not placed yet; the slot that names it places it.
    pub(super) const UNPLACED: Self = Self {
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
pub(in crate::pipeline::owner) struct FrameRecord {
    pub(super) node: SceneNodeId,
    pub(super) root: RenderId,
    pub(super) root_is_boundary: bool,
    pub(super) followers: SmallVec<[(RenderId, EntryIndex, Offset); 1]>,
    pub(super) children: SmallVec<[(RenderId, SceneNodeId); 4]>,
    /// The content published at `node`, whose chunks the frame's next paint
    /// keeps the unchanged ones of.
    pub(super) content: Arc<BoundaryContent>,
}

impl FrameRecord {
    /// The node the frame is published at, which the scene's root slots.
    pub(super) fn node(&self) -> SceneNodeId {
        self.node
    }
}

/// A paint pass's patch, staged until the patch it buffered is applied.
#[derive(Debug, Default)]
pub(in crate::pipeline::owner) struct StagedScene {
    /// What the pass made of each boundary it republished (`Some`), and the
    /// records it dropped (`None`).
    pub(super) records: FxHashMap<RenderId, Option<Refreshed>>,
    /// New placements of records the pass kept.
    pub(super) placed: FxHashMap<RenderId, Placed>,
    /// Records the residue scan found owing a paint.
    pub(super) stale: Vec<RenderId>,
    pub(super) frame: Option<FrameRecord>,
    /// Nodes of removed boundaries still named by a parent the pass did not
    /// republish.
    pub(super) orphans: Vec<SceneNodeId>,
    /// How many entries of the owner's orphan list the pass read; a
    /// removal after the pass adds its nodes after them.
    pub(super) orphans_read: usize,
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
            mut orphans,
            orphans_read,
            ops: _,
        } = staged;
        // Boundaries removed since the pass read the owner's list left their
        // nodes after the entries it read: they wait for a later pass.
        if let Some(late) = self.orphaned_nodes.get(orphans_read..) {
            orphans.extend_from_slice(late);
        }
        for (id, refreshed) in records.drain() {
            match refreshed {
                // A boundary removed since the pass commits no record: the
                // node the pass published for it waits with the orphans, as
                // the removal left the node of the record it took. A node
                // listed twice is retired once (`retired_set` in `finish`).
                Some(Refreshed::Repainted(record)) if self.render_tree.get(id).is_none() => {
                    orphans.push(record.node);
                }
                Some(Refreshed::Repainted(record)) => {
                    if record.followers.is_empty() {
                        self.follower_boundaries.remove(&id);
                    } else {
                        self.follower_boundaries.insert(id);
                    }
                    self.boundary_records.insert(id, record);
                }
                // A boundary removed since the pass took its record with it;
                // its node is retired with the orphans.
                Some(Refreshed::Patched { content, placed }) => {
                    if let Some(record) = self.boundary_records.get_mut(&id) {
                        record.content = content;
                        record.placed = placed;
                    }
                }
                None => {
                    self.follower_boundaries.remove(&id);
                    self.boundary_records.remove(&id);
                }
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
    pub(super) fn discard_staged(&mut self, staged: StagedScene, writer: &mut SceneWriter<'_>) {
        let StagedScene {
            mut records,
            placed,
            stale,
            orphans,
            ..
        } = staged;
        for refreshed in records.drain().filter_map(|(_, refreshed)| refreshed) {
            writer.discard(refreshed.into_content());
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

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Number of repaint boundaries whose published content is currently
    /// recorded.
    ///
    /// Diagnostic surface for the boundary records — a test asserting that
    /// removing a node drops its entry has nothing else to look at, and the
    /// alternative is making the map itself public.
    #[must_use]
    pub fn boundary_record_count(&self) -> usize {
        self.boundary_records.len()
    }

    /// The scene node the repaint boundary `id` is published at in the last
    /// applied version, `None` without a record.
    ///
    /// Diagnostic surface beside [`Self::boundary_record_count`]: what a test
    /// or an inspector reads a boundary's published content through.
    #[must_use]
    pub fn boundary_scene_node(&self, id: RenderId) -> Option<flui_layer::SceneNodeId> {
        self.boundary_records.get(&id).map(BoundaryRecord::node)
    }
}

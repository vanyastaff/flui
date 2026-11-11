//! The scene owner: node ids, patches and their all-or-nothing apply
//! (ADR-0304 §3, §7).

use std::num::NonZeroU64;
use std::sync::Arc;

use flui_foundation::SurfaceGeneration;
use flui_foundation::generation::GenSlots;
use flui_foundation::geometry::Offset;
use flui_foundation::sequence::IdMint;

use super::content::BoundaryContent;
use super::footprint;
use super::id::{ContentVersion, Lineage, NodeKey, SceneNodeId, SceneVersionId};
use super::table::{Table, TableCost};
use super::version::{NodeRecord, SceneVersion, VersionInner};
use crate::scene_snapshot::{DamageRegion, FullReason, NonFinite, ResyncReason};

/// A node's offset in its parent's slot space; finite by construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement(Offset<f64>);

impl Placement {
    /// No offset.
    pub const ZERO: Self = Self(Offset::ZERO);

    /// The placement at `offset`.
    ///
    /// # Errors
    ///
    /// [`NonFinite`] when either coordinate is not finite.
    pub fn try_new(offset: Offset<f64>) -> Result<Self, NonFinite> {
        if offset.is_finite() {
            Ok(Self(offset))
        } else {
            Err(NonFinite)
        }
    }

    /// The offset.
    #[must_use]
    pub fn offset(self) -> Offset<f64> {
        self.0
    }
}

/// One typed change to a scene.
#[derive(Debug, Clone)]
pub enum SceneOp {
    /// Replaces the node's content, or creates the node, unattached until a
    /// parent's slot names it, when the owner minted the id in this patch.
    Publish {
        /// The node.
        node: SceneNodeId,
        /// What it paints now.
        content: Arc<BoundaryContent>,
    },
    /// Moves the node in its parent's slot space without republishing
    /// either content.
    Place {
        /// The node.
        node: SceneNodeId,
        /// Where it stands now.
        placement: Placement,
    },
    /// Frees a node no slot names.
    Retire {
        /// The node.
        node: SceneNodeId,
    },
}

/// Why a patch was refused; the version is as it was before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SceneError {
    /// The id comes from another owner, or from this one before a
    /// [`renew`](SceneOwner::renew).
    #[error("scene node {0:?} belongs to another scene lineage")]
    ForeignNode(SceneNodeId),
    /// The id names no node: minted for the patch, but no `Publish` of it
    /// created the node.
    #[error("scene node {0:?} was never published")]
    UnknownNode(SceneNodeId),
    /// The id's node was retired, or its patch was never applied.
    #[error("scene node {0:?} is stale")]
    StaleNode(SceneNodeId),
    /// A slot still names a retired node, or the patch retires the root.
    #[error("scene node {0:?} is retired while a slot names it")]
    RetiredInUse(SceneNodeId),
    /// Two slots name one node.
    #[error("scene node {0:?} is named by two slots")]
    TwoParents(SceneNodeId),
    /// A slot names the node it sits in or one of its ancestors, or the root.
    #[error("scene node {0:?} would be its own ancestor")]
    OwnAncestor(SceneNodeId),
    /// A node would sit deeper than `u32` counts.
    #[error("scene node {0:?} is nested past u32::MAX")]
    DepthOverflow(SceneNodeId),
    /// The owner's sequence is spent; renew its lineage.
    #[error("the scene owner's version sequence is exhausted")]
    VersionsExhausted,
}

/// The surface a version is applied for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceKey {
    /// Its size in device pixels.
    pub size: (u32, u32),
    /// Its configuration generation.
    pub generation: SurfaceGeneration,
}

/// What the owner hands the raster side: a version, the version the damage is
/// relative to, and the damage.
#[derive(Debug, Clone)]
pub struct ScenePublication {
    /// The version the raster side must hold for `damage` to apply; `None`
    /// means the damage is full.
    pub base: Option<SceneVersionId>,
    /// The version to draw.
    pub scene: SceneVersion,
    /// What changed since `base`.
    pub damage: DamageRegion,
}

/// Where a slot's generation stands in the owner's ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    /// No live id: in the free list, or closed for good.
    Free,
    /// Issued by the open writer; the node exists once a `Publish` of the
    /// patch creates it.
    Minted,
    /// Held by the current version.
    Live,
}

/// Owner-side bookkeeping of node slots, for one lineage.
#[derive(Debug, Default)]
struct Slots {
    generations: GenSlots,
    state: Vec<SlotState>,
    /// Released slots with a next generation, taken before a fresh one.
    free: Vec<u32>,
    /// The slots the open writer minted.
    minted: Vec<u32>,
}

impl Slots {
    fn mint(&mut self) -> NodeKey {
        while let Some(slot) = self.free.pop() {
            // A slot whose generation space is spent closes and stays Free.
            if let Some(release) = self.generations.peek(slot) {
                self.generations.commit(release);
                return self.issue(slot);
            }
        }
        let slot = u32::try_from(self.generations.slot_count())
            .expect("BUG: a scene holds at most 2^32 node slots");
        self.generations.mint_fresh();
        self.state.push(SlotState::Free);
        self.issue(slot)
    }

    fn issue(&mut self, slot: u32) -> NodeKey {
        self.state[slot as usize] = SlotState::Minted;
        self.minted.push(slot);
        NodeKey::new(
            slot,
            self.generations
                .generation(slot)
                .expect("BUG: an issued slot is carried"),
        )
    }

    /// Whether `id`, already of this lineage, is the live generation of its
    /// slot, and in which state.
    fn resolve(&self, id: SceneNodeId) -> Result<SlotState, SceneError> {
        let key = id.key();
        let live = self
            .generations
            .generation(key.slot())
            .ok_or(SceneError::UnknownNode(id))?;
        if key.generation() > live {
            return Err(SceneError::UnknownNode(id));
        }
        if key.generation() < live {
            return Err(SceneError::StaleNode(id));
        }
        match self.state[key.slot() as usize] {
            SlotState::Free => Err(SceneError::StaleNode(id)),
            state => Ok(state),
        }
    }

    /// Returns every slot the writer minted and no version holds: those ids
    /// are spent, and the slot's next id takes the next generation.
    fn release_unused(&mut self) {
        for slot in self.minted.drain(..) {
            let state = &mut self.state[slot as usize];
            if *state == SlotState::Minted {
                *state = SlotState::Free;
                self.free.push(slot);
            }
        }
    }
}

/// A set of slots cleared in O(1): a slot is in the set when its stamp is
/// the current epoch.
#[derive(Debug)]
struct SlotMarks {
    stamps: Vec<u32>,
    epoch: u32,
}

impl Default for SlotMarks {
    fn default() -> Self {
        // Stamps start at 0, so epoch 0 would hold every slot.
        Self {
            stamps: Vec::new(),
            epoch: 1,
        }
    }
}

impl SlotMarks {
    fn clear(&mut self) {
        if let Some(epoch) = self.epoch.checked_add(1) {
            self.epoch = epoch;
        } else {
            self.stamps.fill(0);
            self.epoch = 1;
        }
    }

    /// Adds `slot`; false when it was already in the set.
    fn insert(&mut self, slot: u32) -> bool {
        let index = slot as usize;
        if index >= self.stamps.len() {
            self.stamps.resize(index + 1, 0);
        }
        let added = self.stamps[index] != self.epoch;
        self.stamps[index] = self.epoch;
        added
    }

    fn contains(&self, slot: u32) -> bool {
        self.stamps.get(slot as usize) == Some(&self.epoch)
    }
}

/// The working sets of one apply, kept by the owner so a patch allocates only
/// when the scene outgrows them.
#[derive(Debug, Default)]
struct Scratch {
    /// Nodes the patch published or retired.
    touched: SlotMarks,
    /// Nodes the patch retired.
    retired: SlotMarks,
    /// Children a slot of a changed content names in this patch.
    claimed: SlotMarks,
    /// The children one parent's other content names, cleared per parent.
    named: SlotMarks,
    /// The touched nodes in first-touch order, so the build is deterministic.
    order: Vec<NodeKey>,
    retired_keys: Vec<NodeKey>,
    /// Nodes whose parent the patch changed.
    reparented: Vec<NodeKey>,
    tops: Vec<NodeKey>,
    pending: Vec<(NodeKey, u32)>,
    /// Nodes whose old index entries the patch takes out.
    unindexed: SlotMarks,
    /// The link and texture buckets those entries sat in.
    bucket_links: Vec<u64>,
    bucket_textures: Vec<u64>,
    /// Nodes the patch placed, each once.
    placed_marks: SlotMarks,
    placed: Vec<NodeKey>,
    /// Nodes whose new content enters the indexes; drained by every patch.
    added: Vec<(NodeKey, Arc<BoundaryContent>)>,
}

impl Scratch {
    fn reset(&mut self) {
        self.touched.clear();
        self.retired.clear();
        self.claimed.clear();
        self.order.clear();
        self.retired_keys.clear();
        self.reparented.clear();
        self.tops.clear();
        self.pending.clear();
        self.placed_marks.clear();
        self.placed.clear();
    }
}

/// What every apply so far cost, for the owner's tests and counters.
#[derive(Debug, Default, Clone, Copy)]
struct ApplyCost {
    table: TableCost,
    depth_updates: u64,
    followers_resolved: u64,
}

/// Owns one presentation's retained scene: mints its ids and applies its
/// patches.
#[derive(Debug)]
pub struct SceneOwner {
    lineage: Lineage,
    mint: IdMint<NonZeroU64>,
    current: SceneVersion,
    slots: Slots,
    ops: Vec<SceneOp>,
    scratch: Scratch,
    /// The surface the last patch was applied for.
    surface: Option<SurfaceKey>,
    /// The last applied patch's damage against the version before it.
    patch_damage: DamageRegion,
    /// The operation whose footprint the next apply drops: a planted defect
    /// for the property test.
    #[cfg(feature = "testing")]
    planted: Option<usize>,
    /// Why the next publication is full.
    full: FullReason,
    cost: ApplyCost,
}

impl Default for SceneOwner {
    fn default() -> Self {
        Self::new()
    }
}

/// A lineage for a new owner or a renewal.
fn fresh_lineage() -> Lineage {
    Lineage::fresh().expect("BUG: a process creates and renews fewer than 2^64 scene owners")
}

impl SceneOwner {
    /// An owner of a new lineage holding a version of one empty root.
    #[must_use]
    pub fn new() -> Self {
        Self::with_mint(IdMint::new())
    }

    /// An owner whose sequence continues after `last`, so a test reaches its
    /// exhaustion without applying 2^64 versions. The root version takes the
    /// first value.
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn with_sequence_after(last: u64) -> Self {
        Self::with_mint(IdMint::starting_after(last))
    }

    fn with_mint(mint: IdMint<NonZeroU64>) -> Self {
        let lineage = fresh_lineage();
        let mut slots = Slots::default();
        let current = Self::root_version(lineage, &mint, &mut slots);
        Self {
            lineage,
            mint,
            current,
            slots,
            ops: Vec::new(),
            scratch: Scratch::default(),
            surface: None,
            patch_damage: DamageRegion::Full(FullReason::FirstFrame),
            #[cfg(feature = "testing")]
            planted: None,
            full: FullReason::FirstFrame,
            cost: ApplyCost::default(),
        }
    }

    fn root_version(
        lineage: Lineage,
        mint: &IdMint<NonZeroU64>,
        slots: &mut Slots,
    ) -> SceneVersion {
        let root = slots.mint();
        slots.state[root.slot() as usize] = SlotState::Live;
        slots.minted.clear();
        let seq = mint
            .next()
            .expect("BUG: a fresh sequence issues the root version's id");
        let mut cost = TableCost::default();
        let mut nodes = Table::default();
        nodes.insert(
            root.table_key(),
            NodeRecord::new(root, Arc::new(BoundaryContent::default())),
            &mut cost,
        );
        SceneVersion::from_inner(VersionInner {
            id: SceneVersionId::new(lineage, seq),
            root,
            nodes,
            links: Table::default(),
            textures: Table::default(),
            filtered: Table::default(),
        })
    }

    /// The owner's lineage.
    #[must_use]
    pub fn lineage(&self) -> Lineage {
        self.lineage
    }

    /// The last applied version.
    #[must_use]
    pub fn current(&self) -> &SceneVersion {
        &self.current
    }

    /// Opens a patch: clears the operation buffer, and spends the ids minted
    /// for a patch that was never applied.
    pub fn writer(&mut self) -> SceneWriter<'_> {
        self.slots.release_unused();
        // A patch buffered and never applied drops after the ledger settles.
        self.ops.clear();
        SceneWriter { owner: self }
    }

    /// What the raster side draws next. Until the owner tracks completions,
    /// every publication is full and has no base.
    #[must_use]
    pub fn publication(&self) -> ScenePublication {
        ScenePublication {
            base: None,
            scene: self.current.clone(),
            damage: DamageRegion::Full(self.full),
        }
    }

    /// Starts over under a new lineage: the version holds an empty root, the
    /// sequence and the slots restart, every id of the old lineage is
    /// refused as [`SceneError::ForeignNode`], and the next publication is
    /// `Full(Resync(reason))`.
    pub fn renew(&mut self, reason: ResyncReason) {
        let mut ops = std::mem::take(&mut self.ops);
        self.lineage = fresh_lineage();
        self.mint = IdMint::new();
        // The lineage tells the old ids apart, so the slots start empty.
        self.slots = Slots::default();
        let replaced = std::mem::replace(
            &mut self.current,
            Self::root_version(self.lineage, &self.mint, &mut self.slots),
        );
        self.full = FullReason::Resync(reason);
        self.surface = None;
        self.patch_damage = DamageRegion::Full(FullReason::Resync(reason));
        // The buffered patch and the old lineage's version drop last, with the
        // owner renewed.
        ops.clear();
        self.ops = ops;
        drop(replaced);
    }

    /// Table nodes every apply so far copied out of a shared version.
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn table_copies(&self) -> u64 {
        self.cost.table.copies
    }

    /// Table nodes every apply so far allocated new.
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn table_nodes_created(&self) -> u64 {
        self.cost.table.created
    }

    /// Nodes whose depth every apply so far rewrote (ADR-0304's
    /// `scene_depth_updates`).
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn depth_updates(&self) -> u64 {
        self.cost.depth_updates
    }

    /// Follower resolutions every apply so far ran, in the version before a
    /// patch and after it (ADR-0304's `followers_resolved`).
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn followers_resolved(&self) -> u64 {
        self.cost.followers_resolved
    }

    /// The last applied patch's damage against the version before it: the
    /// footprint of its operations, closed over what filters read
    /// (ADR-0304 §4). The pending log folds it into a publication.
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn patch_damage(&self) -> DamageRegion {
        self.patch_damage
    }

    /// Makes the next apply drop the footprint of its `op`th operation: the
    /// defect the property test against the full diff must catch.
    #[cfg(feature = "testing")]
    pub fn plant_lost_footprint(&mut self, op: usize) {
        self.planted = Some(op);
    }

    /// Applies the operations the writer buffered, all or nothing, and
    /// returns the new version's id. The surface is the key the pending log
    /// will group damage by; until it exists every publication is full.
    ///
    /// Runs no code the scene does not own while it changes the owner's
    /// state: the version it replaces drops after the state is committed.
    ///
    /// # Errors
    ///
    /// A [`SceneError`] naming the first operation the patch cannot apply;
    /// the version is unchanged, and the ids and content versions minted for
    /// the patch stay spent.
    pub fn apply(&mut self, surface: SurfaceKey) -> Result<SceneVersionId, SceneError> {
        #[cfg(feature = "testing")]
        let planted = self.planted.take();
        #[cfg(not(feature = "testing"))]
        let planted = None;
        let mut ops = std::mem::take(&mut self.ops);
        let built =
            Patch::build(
                &self.current,
                &self.slots,
                &mut self.scratch,
                &ops,
                surface.size,
                planted,
            )
            .and_then(|built| {
                let seq = self
                    .mint
                    .next()
                    .map_err(|_| SceneError::VersionsExhausted)?;
                Ok((built, seq))
            });
        let (built, seq) = match built {
            Ok(built) => built,
            Err(error) => {
                self.slots.release_unused();
                // The refused contents drop (an annotation's `Drop` is user
                // code) once the owner's state is settled; the buffer keeps
                // its allocation for the next patch.
                ops.clear();
                self.ops = ops;
                return Err(error);
            }
        };
        let Built {
            mut next,
            cost,
            depth_updates,
            damage,
            followers_resolved,
        } = built;
        let id = SceneVersionId::new(self.lineage, seq);
        next.id = id;
        for key in &self.scratch.retired_keys {
            self.slots.state[key.slot() as usize] = SlotState::Free;
            self.slots.free.push(key.slot());
        }
        for slot in std::mem::take(&mut self.slots.minted) {
            let state = &mut self.slots.state[slot as usize];
            if next.nodes.get(u64::from(slot)).is_some() {
                *state = SlotState::Live;
            } else if *state == SlotState::Minted {
                *state = SlotState::Free;
                self.slots.free.push(slot);
            }
        }
        self.cost.table.copies += cost.copies;
        self.cost.table.created += cost.created;
        self.cost.depth_updates += depth_updates;
        self.cost.followers_resolved += followers_resolved;
        // A patch applied for another surface than the last one: its pixels
        // are not the ones the damage is relative to.
        self.patch_damage = match self.surface {
            Some(last) if last != surface => DamageRegion::Full(FullReason::SurfaceChanged),
            _ => damage,
        };
        self.surface = Some(surface);
        let replaced = std::mem::replace(&mut self.current, SceneVersion::from_inner(next));
        // The version before and the contents it alone held drop last, with
        // the owner's state committed.
        ops.clear();
        self.ops = ops;
        drop(replaced);
        Ok(id)
    }
}

/// Buffers one patch against the owner's current version.
#[derive(Debug)]
pub struct SceneWriter<'a> {
    owner: &'a mut SceneOwner,
}

impl SceneWriter<'_> {
    /// A new node id, valid until this patch is applied or refused. Its
    /// slot's generation advances now, so the id never names anything if the
    /// patch is not applied.
    ///
    /// # Panics
    ///
    /// When all 2^32 node slots are live or closed.
    pub fn mint_node(&mut self) -> SceneNodeId {
        SceneNodeId::new(self.owner.lineage, self.owner.slots.mint())
    }

    /// The next recording's content version.
    ///
    /// # Errors
    ///
    /// [`SceneError::VersionsExhausted`] once the owner's sequence is spent.
    pub fn next_content_version(&mut self) -> Result<ContentVersion, SceneError> {
        let seq = self
            .owner
            .mint
            .next()
            .map_err(|_| SceneError::VersionsExhausted)?;
        Ok(ContentVersion::new(self.owner.lineage, seq))
    }

    /// Buffers `op`. Its ids are checked against the owner's lineage when the
    /// patch is applied.
    pub fn push(&mut self, op: SceneOp) {
        self.owner.ops.push(op);
    }

    /// The version the patch applies to.
    #[must_use]
    pub fn current(&self) -> &SceneVersion {
        &self.owner.current
    }
}

/// A validated patch, built on a copy of the version.
struct Built {
    next: VersionInner,
    cost: TableCost,
    depth_updates: u64,
    damage: DamageRegion,
    followers_resolved: u64,
}

/// One patch's application to a copy of the current version; dropped
/// whole when the patch is refused.
struct Patch<'a> {
    lineage: Lineage,
    base: &'a VersionInner,
    slots: &'a Slots,
    scratch: &'a mut Scratch,
    next: VersionInner,
    cost: TableCost,
    depth_updates: u64,
}

impl<'a> Patch<'a> {
    fn build(
        current: &'a SceneVersion,
        slots: &'a Slots,
        scratch: &'a mut Scratch,
        ops: &[SceneOp],
        surface: (u32, u32),
        planted: Option<usize>,
    ) -> Result<Built, SceneError> {
        scratch.reset();
        let base = current.inner();
        let mut patch = Patch {
            lineage: base.id.lineage(),
            base,
            slots,
            scratch,
            next: base.clone(),
            cost: TableCost::default(),
            depth_updates: 0,
        };
        for op in ops {
            patch.op(op)?;
        }
        // The walks below read the touch order while they write the scratch
        // sets, so it is lent out and put back on every exit.
        let order = std::mem::take(&mut patch.scratch.order);
        let checked = patch.check(&order);
        let skip = planted.and_then(|index| ops.get(index)).map(|op| match op {
            SceneOp::Publish { node, .. } | SceneOp::Place { node, .. } | SceneOp::Retire { node } => {
                node.key()
            }
        });
        let footprint = checked.map(|()| {
            footprint::patch_damage(
                patch.base,
                &mut patch.next,
                &footprint::PatchFacts {
                    touched: &order,
                    placed: &patch.scratch.placed,
                    skip,
                },
                surface,
                &mut patch.cost,
            )
        });
        patch.scratch.order = order;
        let (damage, footprint_cost) = footprint?;
        Ok(Built {
            next: patch.next,
            cost: patch.cost,
            depth_updates: patch.depth_updates,
            damage,
            followers_resolved: footprint_cost.followers_resolved,
        })
    }

    fn check(&mut self, order: &[NodeKey]) -> Result<(), SceneError> {
        self.relink(order)?;
        self.check_retired()?;
        self.check_ancestry()?;
        self.depths(order)?;
        self.reindex(order);
        Ok(())
    }

    fn public(&self, key: NodeKey) -> SceneNodeId {
        SceneNodeId::new(self.lineage, key)
    }

    /// The key of an id of this lineage that is its slot's live generation.
    fn own(&self, id: SceneNodeId) -> Result<NodeKey, SceneError> {
        // Before the generation: another lineage's keys overlap this one's.
        if id.lineage() != self.lineage {
            return Err(SceneError::ForeignNode(id));
        }
        self.slots.resolve(id)?;
        Ok(id.key())
    }

    /// A node the patch may name now: live in the base and not retired, or
    /// created by an earlier `Publish` of the patch.
    fn existing(&self, id: SceneNodeId) -> Result<NodeKey, SceneError> {
        let key = self.own(id)?;
        if self.scratch.retired.contains(key.slot()) {
            return Err(SceneError::StaleNode(id));
        }
        if self.next.record(key).is_none() {
            // Minted, but no Publish created it.
            return Err(SceneError::UnknownNode(id));
        }
        Ok(key)
    }

    fn touch(&mut self, key: NodeKey) {
        if self.scratch.touched.insert(key.slot()) {
            self.scratch.order.push(key);
        }
    }

    fn op(&mut self, op: &SceneOp) -> Result<(), SceneError> {
        match op {
            SceneOp::Publish { node, content } => {
                let key = self.own(*node)?;
                if self.scratch.retired.contains(key.slot()) {
                    return Err(SceneError::StaleNode(*node));
                }
                self.touch(key);
                match self.next.nodes.get_mut(key.table_key(), &mut self.cost) {
                    Some(existing) => existing.set_content(Arc::clone(content)),
                    None => {
                        self.next.nodes.insert(
                            key.table_key(),
                            NodeRecord::new(key, Arc::clone(content)),
                            &mut self.cost,
                        );
                    }
                }
            }
            SceneOp::Place { node, placement } => {
                let key = self.existing(*node)?;
                if self.scratch.placed_marks.insert(key.slot()) {
                    self.scratch.placed.push(key);
                }
                self.next
                    .nodes
                    .get_mut(key.table_key(), &mut self.cost)
                    .expect("BUG: an existing node is in the table")
                    .set_placement(*placement);
            }
            SceneOp::Retire { node } => {
                let key = self.existing(*node)?;
                if key == self.base.root {
                    return Err(SceneError::RetiredInUse(*node));
                }
                self.touch(key);
                self.next.nodes.remove(key.table_key(), &mut self.cost);
                self.scratch.retired.insert(key.slot());
                self.scratch.retired_keys.push(key);
            }
        }
        Ok(())
    }

    /// The touched node's content in the base and in the patch, `None` where
    /// it was unchanged.
    fn changed(&self, key: NodeKey) -> Option<ContentChange> {
        let before = self.base.record(key).map(|node| Arc::clone(node.content()));
        let after = self.next.record(key).map(|node| Arc::clone(node.content()));
        match (&before, &after) {
            (Some(before), Some(after)) if Arc::ptr_eq(before, after) => None,
            _ => Some(ContentChange { before, after }),
        }
    }

    /// Detaches the children a changed content no longer names, attaches the
    /// ones it names for the first time, and records every node whose parent
    /// changed. A child named before and after keeps its parent untouched, so
    /// the cost is the change in the slots, not their number.
    fn relink(&mut self, order: &[NodeKey]) -> Result<(), SceneError> {
        // Release first, so a child moving between two republished parents is
        // attached once whatever order they were published in.
        for &parent in order {
            let Some(ContentChange {
                before: Some(before),
                after,
            }) = self.changed(parent)
            else {
                continue;
            };
            self.scratch.named.clear();
            for child in after.iter().flat_map(|after| after.slots()) {
                self.scratch.named.insert(child.key().slot());
            }
            for child in before.slots() {
                // The base's own content: every slot is of this lineage.
                let key = child.key();
                if self.scratch.named.contains(key.slot()) {
                    continue;
                }
                let detached = self
                    .next
                    .record(key)
                    .is_some_and(|node| node.parent() == Some(parent));
                if detached {
                    self.next
                        .nodes
                        .get_mut(key.table_key(), &mut self.cost)
                        .expect("BUG: a detached child is in the table")
                        .set_parent(None);
                    self.scratch.reparented.push(key);
                }
            }
        }
        for &parent in order {
            let Some(ContentChange {
                before,
                after: Some(after),
            }) = self.changed(parent)
            else {
                // Unchanged, or retired.
                continue;
            };
            self.scratch.named.clear();
            for child in before.iter().flat_map(|before| before.slots()) {
                self.scratch.named.insert(child.key().slot());
            }
            for child in after.slots() {
                self.claim(parent, child)?;
            }
        }
        let (base, next) = (self.base, &self.next);
        self.scratch.reparented.retain(|child| {
            let now = next.record(*child).map(NodeRecord::parent);
            now.is_some() && now != base.record(*child).map(NodeRecord::parent)
        });
        self.scratch
            .reparented
            .sort_unstable_by_key(|child| child.slot());
        self.scratch.reparented.dedup();
        Ok(())
    }

    /// Attaches `child`, named by a slot of `parent`'s new content, unless
    /// the old content named it too.
    fn claim(&mut self, parent: NodeKey, child: SceneNodeId) -> Result<(), SceneError> {
        if child.lineage() != self.lineage {
            return Err(SceneError::ForeignNode(child));
        }
        let key = child.key();
        if key == self.base.root || key == parent {
            return Err(SceneError::OwnAncestor(child));
        }
        self.slots.resolve(child)?;
        if self.scratch.retired.contains(key.slot()) {
            return Err(SceneError::RetiredInUse(child));
        }
        let Some(node) = self.next.record(key) else {
            return Err(SceneError::UnknownNode(child));
        };
        if !self.scratch.claimed.insert(key.slot()) {
            return Err(SceneError::TwoParents(child));
        }
        if self.scratch.named.contains(key.slot()) && node.parent() == Some(parent) {
            return Ok(());
        }
        if node.parent().is_some() {
            return Err(SceneError::TwoParents(child));
        }
        self.next
            .nodes
            .get_mut(key.table_key(), &mut self.cost)
            .expect("BUG: a claimed child is in the table")
            .set_parent(Some(parent));
        self.scratch.reparented.push(key);
        Ok(())
    }

    /// A retired node must not be named by a parent whose content the patch
    /// left as it was.
    fn check_retired(&self) -> Result<(), SceneError> {
        for &retired in &self.scratch.retired_keys {
            let parent = self.base.record(retired).and_then(NodeRecord::parent);
            let still_named = parent.is_some_and(|parent| {
                let before = self.base.record(parent).map(NodeRecord::content);
                let after = self.next.record(parent).map(NodeRecord::content);
                matches!((before, after), (Some(before), Some(after)) if Arc::ptr_eq(before, after))
            });
            if still_named {
                return Err(SceneError::RetiredInUse(self.public(retired)));
            }
        }
        Ok(())
    }

    /// No reparented node may sit under itself.
    fn check_ancestry(&self) -> Result<(), SceneError> {
        let limit = self.next.nodes.len();
        for &child in &self.scratch.reparented {
            let mut above = self.next.record(child).and_then(NodeRecord::parent);
            let mut steps = 0usize;
            while let Some(ancestor) = above {
                if ancestor == child || steps > limit {
                    return Err(SceneError::OwnAncestor(self.public(child)));
                }
                steps += 1;
                above = self.next.record(ancestor).and_then(NodeRecord::parent);
            }
        }
        Ok(())
    }

    /// Recomputes the depth of every reparented or created node and of the
    /// subtrees under them.
    fn depths(&mut self, order: &[NodeKey]) -> Result<(), SceneError> {
        let mut tops = std::mem::take(&mut self.scratch.tops);
        let mut pending = std::mem::take(&mut self.scratch.pending);
        tops.extend_from_slice(&self.scratch.reparented);
        tops.extend(
            order
                .iter()
                .copied()
                .filter(|key| self.base.record(*key).is_none() && self.next.record(*key).is_some()),
        );
        let walked = self.walk_depths(&tops, &mut pending);
        self.scratch.tops = tops;
        self.scratch.pending = pending;
        walked
    }

    fn walk_depths(
        &mut self,
        tops: &[NodeKey],
        pending: &mut Vec<(NodeKey, u32)>,
    ) -> Result<(), SceneError> {
        for &top in tops {
            let mut depth = 0u32;
            let mut above = self.next.record(top).and_then(NodeRecord::parent);
            while let Some(ancestor) = above {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| SceneError::DepthOverflow(self.public(top)))?;
                above = self.next.record(ancestor).and_then(NodeRecord::parent);
            }
            pending.push((top, depth));
            while let Some((key, depth)) = pending.pop() {
                let unchanged = self
                    .next
                    .record(key)
                    .is_none_or(|node| node.depth() == depth);
                if unchanged {
                    // The subtree below follows from this depth and is right.
                    continue;
                }
                let node = self
                    .next
                    .nodes
                    .get_mut(key.table_key(), &mut self.cost)
                    .expect("BUG: a node whose depth is recomputed is in the table");
                node.set_depth(depth);
                self.depth_updates += 1;
                let content = Arc::clone(node.content());
                let below = depth.checked_add(1);
                for child in content.slots() {
                    let child = child.key();
                    let attached = self
                        .next
                        .record(child)
                        .is_some_and(|node| node.parent() == Some(key));
                    if attached {
                        let depth =
                            below.ok_or_else(|| SceneError::DepthOverflow(self.public(child)))?;
                        pending.push((child, depth));
                    }
                }
            }
        }
        Ok(())
    }

    /// Moves every touched node's entries in the three indexes from its old
    /// content to its new one, skipping a content whose link, texture and
    /// filter entries are the old ones. Removals are batched: each bucket a
    /// removed entry sat in is filtered once per patch.
    fn reindex(&mut self, order: &[NodeKey]) {
        let mut links = std::mem::take(&mut self.scratch.bucket_links);
        let mut textures = std::mem::take(&mut self.scratch.bucket_textures);
        let mut added = std::mem::take(&mut self.scratch.added);
        links.clear();
        textures.clear();
        added.clear();
        self.scratch.unindexed.clear();
        for &key in order {
            let Some(ContentChange { before, after }) = self.changed(key) else {
                continue;
            };
            if let (Some(before), Some(after)) = (&before, &after)
                && before.same_indexes(after)
            {
                continue;
            }
            if let Some(before) = before {
                self.scratch.unindexed.insert(key.slot());
                links.extend(before.links().iter().map(|(link, _, _)| link.id()));
                textures.extend(before.textures().iter().map(|(texture, _)| texture.get()));
                if before.is_filtered() {
                    self.next.filtered.remove(key.table_key(), &mut self.cost);
                }
            }
            if let Some(after) = after {
                added.push((key, after));
            }
        }
        links.sort_unstable();
        links.dedup();
        textures.sort_unstable();
        textures.dedup();
        let unindexed = &self.scratch.unindexed;
        self.next.unindex(
            &links,
            &textures,
            |node| unindexed.contains(node.slot()),
            &mut self.cost,
        );
        for (key, after) in added.drain(..) {
            self.next.index(key, &after, &mut self.cost);
        }
        self.scratch.bucket_links = links;
        self.scratch.bucket_textures = textures;
        self.scratch.added = added;
        #[cfg(debug_assertions)]
        for &key in order {
            let Some(node) = self.next.record(key) else {
                continue;
            };
            for (link, _, _) in node.content().links() {
                let leaders = self
                    .next
                    .links
                    .get(link.id())
                    .map_or(0, |users| users.leaders.len());
                debug_assert!(
                    leaders <= 1,
                    "BUG: two LeaderLayers share {link:?} in one scene"
                );
            }
        }
    }
}

/// A touched node's content before and after the patch; `None` where the
/// node is absent.
struct ContentChange {
    before: Option<Arc<BoundaryContent>>,
    after: Option<Arc<BoundaryContent>>,
}

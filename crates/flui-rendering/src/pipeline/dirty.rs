//! Co-located dirty sets for the pipeline owner.
//!
//! The pipeline owner keeps its dirty work in one [`DirtySets`] struct, so the
//! queue pointers live together and "what dirty work is pending" reads as one
//! concept.
//!
//! Each phase's vector has a stable sort discipline applied at flush time:
//!
//! - **Layout / compositing-bits / semantics** sort shallow-first (root
//!   toward leaves) so ancestor layout can stamp constraints before
//!   descendants are visited.
//! - **Paint** sorts deep-first so leaves emit their layers before
//!   ancestor compositing decisions are taken.

use flui_foundation::{RenderId, SlotIndex};
use rustc_hash::FxHashSet;
use smallvec::SmallVec;

// ============================================================================
// DirtyNode
// ============================================================================

/// A node that needs processing in one of the pipeline phases.
///
/// Stores both the node's `RenderId` (1-based) and its depth in the tree
/// for efficient sorting. The `id` field is typed as `RenderId` to enforce
/// the ID offset convention at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyNode {
    /// The render object identifier (1-based `RenderId`).
    pub id: RenderId,
    /// The depth of the node in the render tree (root = 0).
    pub depth: usize,
}

impl DirtyNode {
    /// Creates a new dirty node entry.
    #[inline]
    pub fn new(id: RenderId, depth: usize) -> Self {
        Self { id, depth }
    }
}

// ============================================================================
// DirtySet — Vec + slot index for O(1) dedup
// ============================================================================

/// A single dirty set with O(1) dedup through a slot-addressed index.
///
/// The `Vec` preserves insertion order for sort-at-flush; the [`SlotIndex`]
/// answers the mark-dirty dedup check by the node's slot and confirms the
/// full id. Clearing costs the entries held, not the largest set ever held:
/// a hash set's `clear` walks every bucket it grew during a mount that
/// dirtied the whole tree, on every later frame.
#[derive(Debug, Default)]
pub struct DirtySet {
    vec: Vec<DirtyNode>,
    index: SlotIndex<RenderId>,
}

impl DirtySet {
    /// Creates an empty dirty set.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a node if not already present. Returns `true` if inserted.
    ///
    /// An entry of the same slot and another generation is replaced: the
    /// tree reused the slot, so that node is gone.
    #[inline]
    pub fn push(&mut self, node: DirtyNode) -> bool {
        match self.index.occupant(&self.vec, node.id, |n| n.id) {
            Some(position) if self.vec[position].id == node.id => false,
            Some(position) => {
                self.vec[position] = node;
                true
            }
            None => {
                self.index.insert(node.id, self.vec.len());
                self.vec.push(node);
                true
            }
        }
    }

    /// Returns `true` if the set contains the given id.
    #[inline]
    pub fn contains(&self, id: &RenderId) -> bool {
        self.index.find(&self.vec, *id, |n| n.id).is_some()
    }

    /// Returns the number of entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.vec.len()
    }

    /// Returns `true` if the set is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    /// Sorts the vector by depth (shallow-first).
    #[inline]
    pub fn sort_shallow_first(&mut self) {
        self.vec.sort_unstable_by_key(|n| n.depth);
        self.index.reindex(self.vec.iter().map(|n| n.id));
    }

    /// Sorts the vector by depth (deep-first).
    #[inline]
    pub fn sort_deep_first(&mut self) {
        self.vec
            .sort_unstable_by_key(|n| std::cmp::Reverse(n.depth));
        self.index.reindex(self.vec.iter().map(|n| n.id));
    }

    /// Drains the vector, clearing the index with it.
    pub fn drain(&mut self) -> std::vec::Drain<'_, DirtyNode> {
        self.index.remove_all(self.vec.iter().map(|n| n.id));
        self.vec.drain(..)
    }

    /// Evicts entries whose id is in `removed`.
    #[inline]
    pub fn evict(&mut self, removed: &FxHashSet<RenderId>) {
        self.retain(|d| !removed.contains(&d.id));
    }

    /// Clears the set. The vector and the index retain capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.index.remove_all(self.vec.iter().map(|n| n.id));
        self.vec.clear();
    }

    /// Appends all entries from `other` into `self`, clearing `other`.
    /// Duplicates in `other` that already exist in `self` are skipped.
    pub fn append(&mut self, other: &mut Self) {
        for node in other.drain() {
            self.push(node);
        }
    }

    /// Returns a slice of the entries (for iteration).
    #[inline]
    pub fn as_slice(&self) -> &[DirtyNode] {
        &self.vec
    }

    /// Returns an iterator over the entries.
    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, DirtyNode> {
        self.vec.iter()
    }

    /// Retains only entries matching the predicate.
    ///
    /// A panic in `f` leaves every entry queued and findable, in another
    /// order (see [`SlotIndex::retain`]).
    pub fn retain(&mut self, f: impl FnMut(&DirtyNode) -> bool) {
        self.index.retain(&mut self.vec, |n| n.id, f);
    }
}

impl<'a> IntoIterator for &'a DirtySet {
    type Item = &'a DirtyNode;
    type IntoIter = std::slice::Iter<'a, DirtyNode>;

    fn into_iter(self) -> Self::IntoIter {
        self.vec.iter()
    }
}

// ============================================================================
// PaintQueue — the paint queue carries its own reason
// ============================================================================

/// Why a repaint boundary is queued for the paint phase.
///
/// The paint queue is the record that survives a pass: `run_paint`'s error arm
/// returns before `clear_paint_queue`, so a frame that fails partway leaves the
/// queue naming the work while the walk has already cleared the node flags it
/// touched. Anything that must outlive a pass therefore lives HERE, in the
/// entry, rather than in a node flag or a side map that has to be kept in step
/// with one.
///
/// The two variants are ordered: a repaint subsumes an update, so
/// `PaintQueue::enqueue` upgrades and never downgrades. That makes the
/// "update then paint" / "paint then update" / "mark after a failed pass"
/// orderings all reduce to the same entry, instead of each needing its own
/// guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintKind {
    /// Repaint the boundary's subtree.
    Repaint,
    /// Reuse the boundary's retained output and rebuild only the effect layers
    /// of the listed nodes, which are the ones that asked.
    LayerUpdate(SmallVec<[RenderId; 2]>),
}

/// One entry in the paint queue.
///
/// Deliberately NOT `DirtyNode` with an extra field: that type serves the
/// layout, compositing and semantics queues too, where `PaintKind` would be
/// meaningless — and it is two words, which a `SmallVec` payload would break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintEntry {
    /// The queued render object.
    pub id: RenderId,
    /// Its depth, for the deepest-first flush order.
    pub depth: usize,
    /// Why it is queued.
    pub kind: PaintKind,
}

/// The paint phase's dirty queue: ordered entries plus a slot → position index.
///
/// The index is what lets `enqueue` UPGRADE an existing entry in place rather
/// than push a second one for the same boundary, which is the whole reason this
/// is not a `DirtySet`. A lookup confirms the entry at the stored position
/// carries the asked id, so an index left stale by a reorder answers "not
/// queued" rather than handing back a neighbour's kind.
#[derive(Debug, Default)]
pub struct PaintQueue {
    entries: Vec<PaintEntry>,
    index: SlotIndex<RenderId>,
}

impl PaintQueue {
    /// Creates an empty queue.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `id`, or raises an existing entry's kind. Returns `true` when a
    /// NEW entry was pushed.
    ///
    /// The return value is the caller's cue to fire a frame request, and an
    /// UPGRADE deliberately returns `false`: the boundary was already queued,
    /// so a frame is already scheduled and a second wake would be spurious.
    /// This is not a "nothing happened" signal — the entry may have changed
    /// kind. Read it as "did the queue gain an entry", nothing more.
    ///
    /// Never downgrades: an entry already queued for a repaint stays one, and a
    /// `LayerUpdate` arriving for it is dropped rather than weakening it. An
    /// update arriving for an existing `LayerUpdate` appends its requester.
    pub fn enqueue(&mut self, id: RenderId, depth: usize, kind: PaintKind) -> bool {
        let occupant = self.index.occupant(&self.entries, id, |e| e.id);
        if let Some(position) = occupant.filter(|&p| self.entries[p].id != id) {
            // The tree reused the slot: the queued node is gone.
            self.entries[position] = PaintEntry { id, depth, kind };
            return true;
        }
        if let Some(position) = occupant {
            let existing = &mut self.entries[position].kind;
            match (&mut *existing, kind) {
                // A repaint subsumes any update, in either arrival order, and
                // discards the requesters with it: repainting rebuilds every
                // layer from live properties anyway.
                (PaintKind::Repaint, _) => {}
                (PaintKind::LayerUpdate(_), PaintKind::Repaint) => {
                    *existing = PaintKind::Repaint;
                }
                (PaintKind::LayerUpdate(targets), PaintKind::LayerUpdate(more)) => {
                    for target in more {
                        if !targets.contains(&target) {
                            targets.push(target);
                        }
                    }
                }
            }
            return false;
        }
        self.index.insert(id, self.entries.len());
        self.entries.push(PaintEntry { id, depth, kind });
        true
    }

    /// Why `id` is queued, or `None` when it is not.
    #[inline]
    pub fn kind_of(&self, id: RenderId) -> Option<&PaintKind> {
        self.position(id).map(|i| &self.entries[i].kind)
    }

    /// Whether `id` is queued at all.
    #[inline]
    pub fn contains(&self, id: &RenderId) -> bool {
        self.position(*id).is_some()
    }

    /// Number of queued entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the queue is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries, in queue order.
    #[inline]
    pub fn as_slice(&self) -> &[PaintEntry] {
        &self.entries
    }

    /// Sorts deepest-first and rebuilds the index.
    ///
    /// Every reordering or removal moves the positions the index holds, so
    /// each one rewrites them; both are already O(n) in the queue's length.
    pub fn sort_deep_first(&mut self) {
        self.entries
            .sort_unstable_by_key(|e| std::cmp::Reverse(e.depth));
        self.index.reindex(self.entries.iter().map(|e| e.id));
    }

    /// Drops entries whose id is in `removed`.
    pub fn evict(&mut self, removed: &FxHashSet<RenderId>) {
        self.retain(|e| !removed.contains(&e.id));
    }

    /// Drops entries matching `remove_ids`.
    pub fn retain_not_in(&mut self, remove_ids: &FxHashSet<RenderId>) {
        self.retain(|e| !remove_ids.contains(&e.id));
    }

    /// Clears the queue, retaining capacity. Costs the entries it held.
    #[inline]
    pub fn clear(&mut self) {
        self.index.remove_all(self.entries.iter().map(|e| e.id));
        self.entries.clear();
    }

    /// Moves every entry of `other` into `self`, clearing `other`.
    ///
    /// Collisions UPGRADE rather than being skipped, unlike `DirtySet::append`:
    /// the mid-phase queue and the main queue can each hold an entry for the
    /// same boundary with different kinds, and dropping the incoming one would
    /// lose a repaint that arrived mid-paint.
    pub fn append(&mut self, other: &mut Self) {
        other.index.remove_all(other.entries.iter().map(|e| e.id));
        for entry in other.entries.drain(..) {
            self.enqueue(entry.id, entry.depth, entry.kind);
        }
    }

    /// The kind queued for `id`, cloned — a test-only read.
    ///
    /// Production code never asks: the paint pass reads the whole slice once
    /// and derives its dispositions from it, so a per-id accessor would be a
    /// second way to answer a question that already has one.
    #[cfg(test)]
    pub fn kind_for_test(&self, id: RenderId) -> Option<PaintKind> {
        self.kind_of(id).cloned()
    }

    #[inline]
    fn position(&self, id: RenderId) -> Option<usize> {
        self.index.find(&self.entries, id, |e| e.id)
    }

    fn retain(&mut self, keep: impl FnMut(&PaintEntry) -> bool) {
        self.index.retain(&mut self.entries, |e| e.id, keep);
    }
}

// ============================================================================
// DirtySets
// ============================================================================

/// Co-located dirty sets for the pipeline phases that produce them.
///
/// Each phase's set uses a `Vec` + [`SlotIndex`] pair for O(1) dedup
/// on `push` and ordered iteration at flush time. The vectors are
/// non-shrinking on purpose -- once they grow to the peak working set,
/// they keep that capacity for the next frame.
#[derive(Debug, Default)]
pub struct DirtySets {
    /// Nodes needing layout (sorted shallow-first during flush).
    pub needs_layout: DirtySet,

    /// Nodes needing compositing-bits update (sorted shallow-first during flush).
    pub needs_compositing: DirtySet,

    /// Nodes needing paint (sorted deep-first during flush), each carrying
    /// why it is queued — see [`PaintKind`].
    pub needs_paint: PaintQueue,

    /// Nodes needing semantics update (sorted shallow-first during flush).
    pub needs_semantics: DirtySet,

    /// Nodes whose subtree's semantics only moved: a transform or a
    /// translation changed, no configuration did (sorted shallow-first during
    /// flush). An entry also in [`Self::needs_semantics`] is covered by it.
    pub needs_semantics_geometry: DirtySet,
}

impl DirtySets {
    /// Creates an empty `DirtySets`. Every set is empty.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Evicts every entry whose id is in `removed` from every
    /// set.
    ///
    /// The dispose half of node removal: a freed slot's set entries
    /// must die WITH the node, or the next phase walks ids whose
    /// generation no longer resolves (the residue scan would warn on
    /// every removal).
    pub fn evict(&mut self, removed: &FxHashSet<RenderId>) {
        self.needs_layout.evict(removed);
        self.needs_compositing.evict(removed);
        self.needs_paint.evict(removed);
        self.needs_semantics.evict(removed);
        self.needs_semantics_geometry.evict(removed);
    }

    /// Returns the total number of dirty entries across every set.
    #[inline]
    pub fn total(&self) -> usize {
        self.needs_layout.len()
            + self.needs_paint.len()
            + self.needs_compositing.len()
            + self.needs_semantics.len()
            + self.needs_semantics_geometry.len()
    }

    /// Returns `true` when any phase has at least one dirty entry.
    #[inline]
    pub fn any(&self) -> bool {
        self.total() > 0
    }

    /// Clears every dirty set. Vectors retain their capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.needs_layout.clear();
        self.needs_paint.clear();
        self.needs_compositing.clear();
        self.needs_semantics.clear();
        self.needs_semantics_geometry.clear();
    }
}

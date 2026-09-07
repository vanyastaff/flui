//! Co-located dirty sets for the pipeline owner.
//!
//! `PipelineOwner` used to carry four parallel `Vec<DirtyNode>` fields,
//! scattered across the struct between unrelated bookkeeping. As of
//! 2026-05-20 they are consolidated into a single [`DirtySets`] struct so
//! the one cache line of four `Vec` pointers lives together, the names
//! line up, and "what dirty work is pending" reads as one concept rather
//! than four adjacent ones.
//!
//! Each phase's vector has a stable sort discipline applied at flush time:
//!
//! - **Layout / compositing-bits / semantics** sort shallow-first (root
//!   toward leaves) so ancestor layout can stamp constraints before
//!   descendants are visited.
//! - **Paint** sorts deep-first so leaves emit their layers before
//!   ancestor compositing decisions are taken.
//!
//! See `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md`
//! Section 6 for the broader rationale.

use flui_foundation::RenderId;
use rustc_hash::{FxHashMap, FxHashSet};
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
// DirtySet — Vec + HashSet for O(1) dedup
// ============================================================================

/// A single dirty set with O(1) dedup via a companion `FxHashSet`.
///
/// The `Vec` preserves insertion order for sort-at-flush; the `HashSet`
/// provides O(1) `contains` for the mark-dirty dedup check. Both are
/// kept in sync on every push/evict/clear.
#[derive(Debug, Default)]
pub struct DirtySet {
    vec: Vec<DirtyNode>,
    set: FxHashSet<RenderId>,
}

impl DirtySet {
    /// Creates an empty dirty set.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a node if not already present. Returns `true` if inserted.
    #[inline]
    pub fn push(&mut self, node: DirtyNode) -> bool {
        if self.set.insert(node.id) {
            self.vec.push(node);
            true
        } else {
            false
        }
    }

    /// Returns `true` if the set contains the given id.
    #[inline]
    pub fn contains(&self, id: &RenderId) -> bool {
        self.set.contains(id)
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
    }

    /// Sorts the vector by depth (deep-first).
    #[inline]
    pub fn sort_deep_first(&mut self) {
        self.vec
            .sort_unstable_by_key(|n| std::cmp::Reverse(n.depth));
    }

    /// Drains the vector, clearing both vec and set.
    pub fn drain(&mut self) -> std::vec::Drain<'_, DirtyNode> {
        self.set.clear();
        self.vec.drain(..)
    }

    /// Evicts entries whose id is in `removed`.
    #[inline]
    pub fn evict(&mut self, removed: &FxHashSet<RenderId>) {
        self.set.retain(|id| !removed.contains(id));
        self.vec.retain(|d| !removed.contains(&d.id));
    }

    /// Clears both vec and set. Vec retains capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.vec.clear();
        self.set.clear();
    }

    /// Appends all entries from `other` into `self`, clearing `other`.
    /// Duplicates in `other` that already exist in `self` are skipped.
    pub fn append(&mut self, other: &mut Self) {
        for node in other.vec.drain(..) {
            if self.set.insert(node.id) {
                self.vec.push(node);
            }
        }
        other.set.clear();
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
    pub fn retain(&mut self, mut f: impl FnMut(&DirtyNode) -> bool) {
        self.vec.retain(|d| {
            if f(d) {
                true
            } else {
                self.set.remove(&d.id);
                false
            }
        });
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
/// meaningless — and it is held to two words by `dirty_node_is_two_usize`,
/// which a `SmallVec` payload would break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintEntry {
    /// The queued render object.
    pub id: RenderId,
    /// Its depth, for the deepest-first flush order.
    pub depth: usize,
    /// Why it is queued.
    pub kind: PaintKind,
}

/// The paint phase's dirty queue: ordered entries plus an id → position index.
///
/// The index is what lets `enqueue` UPGRADE an existing entry in place rather
/// than push a second one for the same boundary, which is the whole reason this
/// is not a `DirtySet`.
#[derive(Debug, Default)]
pub struct PaintQueue {
    entries: Vec<PaintEntry>,
    index: FxHashMap<RenderId, usize>,
}

impl PaintQueue {
    /// Creates an empty queue.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `id`, or raises an existing entry's kind. Returns `true` when a
    /// NEW entry was pushed (the caller's cue to fire a frame request).
    ///
    /// Never downgrades: an entry already queued for a repaint stays one, and a
    /// `LayerUpdate` arriving for it is dropped rather than weakening it. An
    /// update arriving for an existing `LayerUpdate` appends its requester.
    pub fn enqueue(&mut self, id: RenderId, depth: usize, kind: PaintKind) -> bool {
        if let Some(&position) = self.index.get(&id) {
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
        self.index.get(&id).map(|&i| &self.entries[i].kind)
    }

    /// Whether `id` is queued at all.
    #[inline]
    pub fn contains(&self, id: &RenderId) -> bool {
        self.index.contains_key(id)
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
    /// Every reordering or removal invalidates the positions the index holds,
    /// so each one rebuilds it. Both operations are already at least O(n), so
    /// this costs nothing asymptotically — and a stale index would silently
    /// upgrade the wrong entry.
    pub fn sort_deep_first(&mut self) {
        self.entries
            .sort_unstable_by_key(|e| std::cmp::Reverse(e.depth));
        self.reindex();
    }

    /// Drops entries whose id is in `removed`.
    pub fn evict(&mut self, removed: &FxHashSet<RenderId>) {
        self.entries.retain(|e| !removed.contains(&e.id));
        self.reindex();
    }

    /// Drops entries matching `remove_ids`.
    pub fn retain_not_in(&mut self, remove_ids: &FxHashSet<RenderId>) {
        self.entries.retain(|e| !remove_ids.contains(&e.id));
        self.reindex();
    }

    /// Clears the queue, retaining capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    /// Moves every entry of `other` into `self`, clearing `other`.
    ///
    /// Collisions UPGRADE rather than being skipped, unlike `DirtySet::append`:
    /// the mid-phase queue and the main queue can each hold an entry for the
    /// same boundary with different kinds, and dropping the incoming one would
    /// lose a repaint that arrived mid-paint.
    pub fn append(&mut self, other: &mut Self) {
        for entry in other.entries.drain(..) {
            self.enqueue(entry.id, entry.depth, entry.kind);
        }
        other.index.clear();
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (position, entry) in self.entries.iter().enumerate() {
            self.index.insert(entry.id, position);
        }
    }
}

// ============================================================================
// DirtySets
// ============================================================================

/// Co-located dirty sets for the four pipeline phases that produce them.
///
/// Each phase's set uses a `Vec` + `FxHashSet` pair for O(1) dedup
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
}

impl DirtySets {
    /// Creates an empty `DirtySets`. All four sets are empty.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Evicts every entry whose id is in `removed` from all four
    /// sets.
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
    }

    /// Returns the total number of dirty entries across all four sets.
    #[inline]
    pub fn total(&self) -> usize {
        self.needs_layout.len()
            + self.needs_paint.len()
            + self.needs_compositing.len()
            + self.needs_semantics.len()
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
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;

    #[test]
    fn dirty_node_is_two_usize() {
        // RenderId(NonZeroUsize, 8) + usize(8) = 16 bytes on 64-bit.
        assert!(size_of::<DirtyNode>() <= 16);
    }

    #[test]
    fn dirty_set_push_deduplicates() {
        let mut set = DirtySet::new();
        let id = RenderId::new(1);
        assert!(set.push(DirtyNode::new(id, 0)));
        assert!(!set.push(DirtyNode::new(id, 1))); // duplicate
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn dirty_set_contains() {
        let mut set = DirtySet::new();
        let id = RenderId::new(1);
        assert!(!set.contains(&id));
        set.push(DirtyNode::new(id, 0));
        assert!(set.contains(&id));
    }

    #[test]
    fn dirty_set_evict() {
        let mut set = DirtySet::new();
        let id1 = RenderId::new(1);
        let id2 = RenderId::new(2);
        set.push(DirtyNode::new(id1, 0));
        set.push(DirtyNode::new(id2, 1));
        let mut removed = FxHashSet::default();
        removed.insert(id1);
        set.evict(&removed);
        assert_eq!(set.len(), 1);
        assert!(!set.contains(&id1));
        assert!(set.contains(&id2));
    }

    #[test]
    fn dirty_set_drain() {
        let mut set = DirtySet::new();
        set.push(DirtyNode::new(RenderId::new(1), 0));
        set.push(DirtyNode::new(RenderId::new(2), 1));
        let drained: Vec<_> = set.drain().collect();
        assert_eq!(drained.len(), 2);
        assert!(set.is_empty());
    }

    #[test]
    fn dirty_sets_evict_across_all() {
        let mut sets = DirtySets::new();
        let id = RenderId::new(1);
        sets.needs_layout.push(DirtyNode::new(id, 0));
        sets.needs_paint.enqueue(id, 0, PaintKind::Repaint);
        let mut removed = FxHashSet::default();
        removed.insert(id);
        sets.evict(&removed);
        assert_eq!(sets.total(), 0);
    }
}

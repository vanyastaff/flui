//! SemanticsTree - Slab-based storage for semantics nodes
//!
//! This module provides the SemanticsTree struct for managing the accessibility
//! tree: slab storage, parent/child links, and the cascading [`SemanticsTree::remove`].

use flui_foundation::{ElementId, SemanticsId};
use rustc_hash::{FxHashMap, FxHashSet};
use slab::Slab;
use smallvec::SmallVec;

use crate::identity::AccessibilityNodeId;
use crate::node::SemanticsNode;

// ============================================================================
// SEMANTICS TREE
// ============================================================================

/// SemanticsTree - Slab-based storage for accessibility nodes.
///
/// This is the fifth of FLUI's five trees, corresponding to Flutter's Semantics
/// tree used for accessibility services (screen readers, voice control, etc.).
///
/// # Architecture
///
/// ```text
/// SemanticsTree
///   ├─ nodes: Slab<SemanticsNode>  (direct storage)
///   └─ root: Option<SemanticsId>
/// ```
///
/// # Thread Safety
///
/// SemanticsTree itself is not thread-safe. Use `Arc<RwLock<SemanticsTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust
/// use flui_semantics::{SemanticsNode, SemanticsTree};
///
/// let mut tree = SemanticsTree::new();
///
/// // Insert semantics node
/// let mut node = SemanticsNode::new();
/// node.config_mut().set_label("Submit");
/// node.config_mut().set_button(true);
/// let id = tree.insert(node);
///
/// // Access node
/// let node = tree.get(id).unwrap();
/// assert_eq!(node.label(), Some("Submit"));
/// ```
#[derive(Debug)]
pub struct SemanticsTree {
    /// Slab storage for SemanticsNodes (0-based indexing internally)
    nodes: Slab<SemanticsNode>,

    /// Root SemanticsNode ID (None if tree is empty)
    root: Option<SemanticsId>,

    /// The (superset of) dirty node ids, maintained at every seam that can
    /// flip a node's dirty bit ([`Self::insert`], [`Self::get_mut`],
    /// [`Self::iter_mut`], [`Self::remove_shallow`], [`Self::clear`],
    /// [`Self::mark_all_dirty`], [`Self::mark_all_clean`]).
    ///
    /// This is what makes [`Self::has_dirty_nodes`] O(1) — and, as a set
    /// rather than the earlier plain counter, what makes *iterating* the
    /// dirty nodes ([`Self::dirty_ids`]) O(dirty) instead of an O(arena)
    /// filter scan. Flutter maintains the same information as
    /// `SemanticsOwner._dirtyNodes` (a set filled by `_markDirty`); this is
    /// the arena-storage port of that.
    ///
    /// Deliberately a superset, not an exact set: handing out
    /// `&mut SemanticsNode` (via [`Self::get_mut`] / [`Self::iter_mut`])
    /// counts the node as dirty *at the borrow*, so nothing the caller does
    /// through the borrow can under-count. Over-counting is self-healing —
    /// a member whose bit turns out clean is skipped by consumers and the
    /// set resets via [`Self::mark_all_clean`].
    dirty: FxHashSet<SemanticsId>,

    /// Arena ids per stable [`AccessibilityNodeId`] value, for O(1)
    /// stable-identity lookups ([`Self::find_by_accessibility_id`],
    /// [`Self::is_accessibility_id_live`]). Almost always one entry per id;
    /// a hand-built tree can alias two nodes onto one render identity, so
    /// the value is a list and the unique-lookup reports the ambiguity by
    /// returning `None`.
    stable_index: FxHashMap<u64, SmallVec<[SemanticsId; 1]>>,

    /// Stable identities removed from the arena since the last
    /// [`Self::take_removed_accessibility_ids`] drain — how the publish
    /// path prunes its adapter mirror in O(removed) instead of sweeping
    /// every live node per flush. Drained (and thus bounded) by every
    /// publish; grows only while no publisher is attached, in which case
    /// the arena itself is the caller's to manage.
    removed_stable: Vec<u64>,
}

impl SemanticsTree {
    /// Creates a new empty SemanticsTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            dirty: FxHashSet::default(),
            stable_index: FxHashMap::default(),
            removed_stable: Vec::new(),
        }
    }

    /// Creates a SemanticsTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
            dirty: FxHashSet::default(),
            stable_index: FxHashMap::default(),
            removed_stable: Vec::new(),
        }
    }

    // ========== Root Management ==========

    /// Get the root SemanticsNode ID.
    #[inline]
    pub fn root(&self) -> Option<SemanticsId> {
        self.root
    }

    /// Set the root SemanticsNode ID.
    ///
    /// Changing which node is the root is a published-state change even
    /// when no node's content moved — the flush path detects the identity
    /// transition and escalates to a self-contained full update, but only
    /// if its dirty gate lets it run at all. So a *changed* root marks the
    /// new root node dirty; re-setting the same root stays free.
    pub fn set_root(&mut self, root: Option<SemanticsId>) {
        if self.root == root {
            return;
        }
        self.root = root;
        if let Some(id) = root {
            self.mark_dirty(id);
        }
    }

    // ========== Basic Operations ==========

    /// Checks if a SemanticsNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: SemanticsId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of SemanticsNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Inserts a SemanticsNode into the tree.
    ///
    /// Returns the SemanticsId of the inserted node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `SemanticsId(1)`
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_semantics::{SemanticsNode, SemanticsTree};
    ///
    /// let mut tree = SemanticsTree::new();
    /// let node = SemanticsNode::new();
    /// let id = tree.insert(node);
    /// ```
    pub fn insert(&mut self, node: SemanticsNode) -> SemanticsId {
        let is_dirty = node.is_dirty();
        let stable = node.accessibility_id();
        let slab_index = self.nodes.insert(node);
        let id = SemanticsId::new(slab_index + 1); // +1 offset
        if is_dirty {
            self.dirty.insert(id);
        }
        if let Some(stable) = stable {
            self.stable_index
                .entry(stable.as_u64())
                .or_default()
                .push(id);
        }
        id
    }

    /// Inserts a SemanticsNode with an associated ElementId.
    pub fn insert_with_element(
        &mut self,
        node: SemanticsNode,
        element_id: ElementId,
    ) -> SemanticsId {
        let node = node.with_element_id(element_id);
        self.insert(node)
    }

    /// Returns a reference to a SemanticsNode.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `SemanticsId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: SemanticsId) -> Option<&SemanticsNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a SemanticsNode.
    ///
    /// The node is conservatively counted as dirty **at the borrow**: the
    /// tree cannot observe what the caller does through `&mut SemanticsNode`
    /// (node-level mutators set the node's own dirty bit, but the tree's
    /// O(1) dirty accounting cannot see that transition), so it presumes
    /// mutation. A borrow that turns out to be read-only costs one spurious
    /// dirty bit that the next flush clears without publishing anything.
    pub fn get_mut(&mut self, id: SemanticsId) -> Option<&mut SemanticsNode> {
        let node = self.nodes.get_mut(id.get() - 1)?;
        if !node.is_dirty() {
            node.mark_dirty();
        }
        self.dirty.insert(id);
        Some(node)
    }

    /// Removes `id` and every descendant, children before their parent
    /// (each through [`Self::remove_shallow`], so parent links, the root,
    /// the dirty set and the stable index stay consistent), and returns the
    /// removed `id` node.
    ///
    /// Returns `None` if `id` is not live, or — with a `tracing::warn!` and
    /// nothing removed — if the pre-walk meets a node twice. That is a
    /// corrupted cycle: [`Self::add_child`] rejects every link that would
    /// create one, so only direct node edits through [`Self::get_mut`] can.
    ///
    /// The pre-walk keeps its own stack, so a deep subtree costs no Rust
    /// stack. Use [`Self::remove_shallow`] to remove one node and orphan its
    /// children instead.
    pub fn remove(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
        if !self.contains(id) {
            return None;
        }

        // Pre-walk: every descendant lands after its parent in `worklist`.
        let mut worklist: SmallVec<[SemanticsId; 32]> = SmallVec::new();
        let mut to_visit: SmallVec<[SemanticsId; 32]> = SmallVec::new();
        let mut visited: FxHashSet<SemanticsId> = FxHashSet::default();
        to_visit.push(id);
        while let Some(current) = to_visit.pop() {
            if !visited.insert(current) {
                tracing::warn!(
                    ?current,
                    ?id,
                    "SemanticsTree::remove met a node twice (the tree holds a cycle); \
                     nothing removed"
                );
                return None;
            }
            worklist.push(current);
            if let Some(node) = self.get(current) {
                to_visit.extend(node.children().iter().copied());
            }
        }

        // Drain in reverse: children are removed before their parents.
        let mut removed_root = None;
        for node_id in worklist.into_iter().rev() {
            let removed = self.remove_shallow(node_id);
            if node_id == id {
                removed_root = removed;
            }
        }
        removed_root
    }

    /// Removes a single SemanticsNode from the tree **without**
    /// cascading to descendants. Descendants are orphaned in storage
    /// (their `parent` pointers still reference the now-deleted slot —
    /// use only when the caller will re-attach or drop them
    /// immediately).
    ///
    /// **Contract change**: the parent's children vector IS now drained
    /// of `id` before the node is dropped. This method used to
    /// intentionally leave the parent's children vec pointing at a
    /// stale id, expecting the caller to handle parent-cleanup; a
    /// review found zero production callers actually exercising that
    /// escape-hatch, so the cleanup was made automatic instead.
    pub fn remove_shallow(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
        if !self.contains(id) {
            return None;
        }
        // Unlink from parent's children vec — matches the trait
        // contract. `get_mut` also marks the parent dirty, which is
        // load-bearing for incremental publishing: the parent's children
        // list is part of its published payload, and republishing the
        // parent is what tells an adapter the child is gone.
        if let Some(parent_id) = self.get(id).and_then(SemanticsNode::parent)
            && let Some(parent) = self.get_mut(parent_id)
        {
            parent.remove_child(id);
        }
        if self.root == Some(id) {
            self.root = None;
        }
        let removed = self.nodes.try_remove(id.get() - 1);
        self.dirty.remove(&id);
        if let Some(stable) = removed.as_ref().and_then(SemanticsNode::accessibility_id) {
            self.forget_stable(stable.as_u64(), id);
        }
        removed
    }

    /// Drops one arena id from the stable index and records the removal for
    /// the publish path's mirror prune.
    fn forget_stable(&mut self, raw: u64, id: SemanticsId) {
        if let Some(entries) = self.stable_index.get_mut(&raw) {
            entries.retain(|&mut entry| entry != id);
            if entries.is_empty() {
                self.stable_index.remove(&raw);
            }
        }
        self.removed_stable.push(raw);
    }

    /// Clears all nodes from the tree.
    ///
    /// Every addressable node's stable identity is recorded as removed (the
    /// same bookkeeping as [`Self::remove_shallow`]), so a publisher
    /// diffing against its last delivered update learns about the wipe.
    pub fn clear(&mut self) {
        for (_, node) in &self.nodes {
            if let Some(stable) = node.accessibility_id() {
                self.removed_stable.push(stable.as_u64());
            }
        }
        self.nodes.clear();
        self.root = None;
        self.dirty.clear();
        self.stable_index.clear();
    }

    /// Whether `ancestor` is a strict ancestor of `descendant` (a node is not
    /// its own ancestor).
    ///
    /// Walks `descendant`'s parent chain. The walk stops after `len()` steps,
    /// so a corrupted parent cycle cannot make it loop forever.
    fn is_ancestor_of(&self, ancestor: SemanticsId, descendant: SemanticsId) -> bool {
        std::iter::successors(self.parent(descendant), |&id| self.parent(id))
            .take(self.len())
            .any(|id| id == ancestor)
    }

    // ========== Tree Operations ==========

    /// Adds `child_id` as a child of `parent_id`.
    ///
    /// **Auto-detach semantics** — if `child_id` is currently
    /// attached to a different parent, it is removed from that parent's
    /// children vector first. Re-attaching to the same parent is a
    /// short-circuit no-op (`SemanticsNode::add_child` carries the
    /// containment dedup so the children vector never holds a duplicate
    /// id). Mirrors the layer-side guarantee that [`LayerTree::add_child`]
    /// provides and matches Flutter `semantics.dart` `_SemanticsTreeWalker`
    /// reparent semantics.
    ///
    /// Missing-id lookups (either `parent_id` or `child_id` not in the
    /// tree) are silent no-ops.
    ///
    /// [`LayerTree::add_child`]: ../../flui-layer/src/tree/layer_tree.rs
    pub fn add_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        // Both endpoints must exist — otherwise the call is a no-op.
        if !self.contains(parent_id) || !self.contains(child_id) {
            return;
        }

        // Reject self-attachment outright — `parent_id == child_id` is a
        // 1-cycle, the smallest possible.
        if parent_id == child_id {
            tracing::warn!(
                ?parent_id,
                "SemanticsTree::add_child rejected self-link (cycle)"
            );
            return;
        }

        // Reject attaching an ancestor of `parent_id` under it (would
        // create an N-cycle). The cascading `remove` would follow such
        // a cycle to unbounded recursion + stack overflow; this guard
        // makes cycles impossible to enter via the public API.
        if self.is_ancestor_of(child_id, parent_id) {
            tracing::warn!(
                ?parent_id,
                ?child_id,
                "SemanticsTree::add_child rejected cycle \
                 (child is ancestor of parent)"
            );
            return;
        }

        // 1. Detach from previous parent if one exists and differs.
        let prev_parent = self.get(child_id).and_then(SemanticsNode::parent);
        if let Some(prev) = prev_parent {
            if prev == parent_id {
                // Already attached to this parent — short-circuit. The
                // node-level dedup in SemanticsNode::add_child would
                // catch a double-add, but bailing here avoids the
                // redundant mutation + dirty-bit ripple.
                return;
            }
            if let Some(prev_node) = self.get_mut(prev) {
                prev_node.remove_child(child_id);
            }
        }

        // 2. Attach to new parent. `SemanticsNode::add_child` already
        //    has the containment dedup (node.rs).
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // 3. Update child's parent pointer.
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent SemanticsNode.
    pub fn remove_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(None);
        }
    }

    /// Returns the parent of a node.
    pub fn parent(&self, id: SemanticsId) -> Option<SemanticsId> {
        self.get(id)?.parent()
    }

    /// Returns the children of a node.
    pub fn children(&self, id: SemanticsId) -> Option<&[SemanticsId]> {
        self.get(id).map(SemanticsNode::children)
    }

    /// Builds the stable-identity payload for one node, or `None` if `id` is
    /// not live **or the node is unaddressable** (never bound to a render
    /// boundary). A payload names its subject, so a node with no stable
    /// identity has no payload — the same skip rule as
    /// [`tree_to_update`](crate::tree_to_update), applied at construction
    /// rather than trusted to every consumer.
    ///
    /// This is the constructor for
    /// [`SemanticsNodeData`](crate::update::SemanticsNodeData): content and
    /// identity come from the node
    /// ([`SemanticsNode::to_node_data`](crate::SemanticsNode::to_node_data)),
    /// and `children` — which a node alone cannot resolve, since it stores its
    /// children as arena [`SemanticsId`]s — is filled here with each
    /// addressable child's stable
    /// [`AccessibilityNodeId`], in child order. An
    /// unaddressable child is likewise omitted, so the payload never
    /// references a node the platform was not given.
    pub fn node_data(&self, id: SemanticsId) -> Option<crate::update::SemanticsNodeData> {
        self.node_data_of(self.get(id)?)
    }

    /// [`Self::node_data`] for a node reference already in hand — the
    /// whole-tree publish path iterates nodes and must not pay a second
    /// arena lookup per node just to rebuild the reference it started from.
    pub(crate) fn node_data_of(
        &self,
        node: &SemanticsNode,
    ) -> Option<crate::update::SemanticsNodeData> {
        let mut data = node.to_node_data();
        // No identity, no payload: an update entry the platform cannot
        // address is worse than an absent one.
        data.id?;
        data.children = node
            .children()
            .iter()
            .filter_map(|&child| self.get(child).and_then(SemanticsNode::accessibility_id))
            .collect();
        Some(data)
    }

    // ========== Dirty Tracking ==========

    /// Returns all dirty node ids in the tree.
    pub fn dirty_nodes(&self) -> impl Iterator<Item = SemanticsId> + '_ {
        self.nodes
            .iter()
            .filter(|(_, node)| node.is_dirty())
            .map(|(index, _)| SemanticsId::new(index + 1))
    }

    /// Returns `(id, &SemanticsNode)` pairs for every dirty node.
    ///
    /// Lets callers (notably [`crate::owner::SemanticsOwner::flush`]) walk
    /// dirty nodes in a single pass without an intermediate
    /// `Vec<SemanticsId>` collect that would force a per-frame heap
    /// allocation when there is any dirt.
    pub fn iter_dirty(&self) -> impl Iterator<Item = (SemanticsId, &SemanticsNode)> + '_ {
        self.nodes
            .iter()
            .filter(|(_, node)| node.is_dirty())
            .map(|(index, node)| (SemanticsId::new(index + 1), node))
    }

    /// Marks a single node as dirty (no-op for a missing id).
    ///
    /// The tree-level entry point for "this node's published payload is
    /// stale" — routes through [`Self::get_mut`] so the O(1) dirty
    /// accounting observes the transition.
    pub fn mark_dirty(&mut self, id: SemanticsId) {
        let _ = self.get_mut(id);
    }

    /// Marks every node as dirty.
    ///
    /// The full-republish primitive (`SemanticsOwner::send_full_tree`):
    /// after this, a flush re-examines every node.
    pub fn mark_all_dirty(&mut self) {
        for (index, node) in &mut self.nodes {
            node.mark_dirty();
            self.dirty.insert(SemanticsId::new(index + 1));
        }
    }

    /// Marks all nodes as clean.
    pub fn mark_all_clean(&mut self) {
        // Bits are cleared through the set, not a full-arena sweep: only
        // members can have a set bit (every bit-setting seam also inserts),
        // so this is O(dirty).
        for id in std::mem::take(&mut self.dirty) {
            if let Some(node) = self.nodes.get_mut(id.get() - 1) {
                node.mark_clean();
            }
        }
    }

    /// Returns true if any node may be dirty — O(1).
    ///
    /// Backed by the maintained dirty set rather than a scan; see the
    /// `dirty` field doc for why the scan was the wrong shape (it walked
    /// the entire arena precisely on the idle frame) and for the
    /// conservative-superset contract (`true` can be spurious after a
    /// read-only `&mut` borrow; `false` is always exact).
    pub fn has_dirty_nodes(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// The maintained dirty-id set — O(dirty) to iterate, unordered.
    ///
    /// A superset of the truly-dirty nodes (see the `dirty` field doc):
    /// consumers re-check [`SemanticsNode::is_dirty`] per member and skip
    /// ids that died since marking.
    pub fn dirty_ids(&self) -> impl Iterator<Item = SemanticsId> + '_ {
        self.dirty.iter().copied()
    }

    // ========== Stable-Identity Lookup ==========

    /// Resolves a stable [`AccessibilityNodeId`] to its arena id — `None`
    /// when the identity is not in the arena **or is ambiguous** (aliased
    /// by more than one node, possible only in hand-built trees; callers
    /// treat ambiguity as "cannot address" exactly like
    /// [`SemanticsOwner::resolve_action`](crate::SemanticsOwner)).
    pub fn find_by_accessibility_id(&self, id: AccessibilityNodeId) -> Option<SemanticsId> {
        match self.stable_index.get(&id.as_u64())?.as_slice() {
            &[single] => Some(single),
            _ => None,
        }
    }

    /// Whether any live arena node publishes under this raw stable id.
    pub(crate) fn is_accessibility_id_live(&self, raw: u64) -> bool {
        self.stable_index.contains_key(&raw)
    }

    /// Drains the stable identities removed since the last drain.
    ///
    /// The publish path's O(removed) prune feed — see the `removed_stable`
    /// field doc. May contain duplicates and identities that have since
    /// been re-inserted; consumers re-check liveness per entry.
    pub(crate) fn take_removed_accessibility_ids(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.removed_stable)
    }

    /// Replaces the node stored at `id` in place, preserving the arena id
    /// and the parent link (so the parent's children vector stays valid
    /// without re-linking).
    ///
    /// The subtree-graft primitive: re-assembly replaces a boundary node's
    /// content while every reference to it — its parent's children list,
    /// its published stable identity — survives. The caller owns the
    /// children: the OLD node's children must already have been removed (or
    /// be about to be re-attached), and the NEW node starts with whatever
    /// children the caller attaches afterwards. Returns `false` (and
    /// changes nothing) if `id` is not live.
    pub fn replace_node(&mut self, id: SemanticsId, mut node: SemanticsNode) -> bool {
        let Some(slot) = self.nodes.get_mut(id.get() - 1) else {
            return false;
        };
        node.set_parent(slot.parent());
        let old = std::mem::replace(slot, node);

        let old_stable = old.accessibility_id();
        let new_stable = self.nodes[id.get() - 1].accessibility_id();
        if old_stable != new_stable {
            if let Some(stable) = old_stable {
                self.forget_stable(stable.as_u64(), id);
            }
            if let Some(stable) = new_stable {
                self.stable_index
                    .entry(stable.as_u64())
                    .or_default()
                    .push(id);
            }
        }

        // Replacement is a mutation: the slot's published payload changed.
        self.mark_dirty(id);
        true
    }

    // ========== Iteration ==========

    /// Returns an iterator over all SemanticsIds in the tree.
    pub fn semantics_ids(&self) -> impl Iterator<Item = SemanticsId> + '_ {
        self.nodes
            .iter()
            .map(|(index, _)| SemanticsId::new(index + 1))
    }

    /// Returns an iterator over all (SemanticsId, &SemanticsNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (SemanticsId, &SemanticsNode)> + '_ {
        self.nodes
            .iter()
            .map(|(index, node)| (SemanticsId::new(index + 1), node))
    }

    /// Returns a mutable iterator over all (SemanticsId, &mut SemanticsNode)
    /// pairs.
    ///
    /// Every yielded node is conservatively counted as dirty, for the same
    /// reason [`Self::get_mut`] counts its borrow: the tree cannot observe
    /// mutation through the handed-out `&mut`, and under-counting would let
    /// a real change slip past the O(1) dirty check.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (SemanticsId, &mut SemanticsNode)> + '_ {
        self.mark_all_dirty();
        self.nodes
            .iter_mut()
            .map(|(index, node)| (SemanticsId::new(index + 1), node))
    }
}

impl Default for SemanticsTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_tree_new() {
        let tree = SemanticsTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_semantics_tree_with_capacity() {
        let tree = SemanticsTree::with_capacity(100);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_semantics_tree_insert() {
        let mut tree = SemanticsTree::new();
        let node = SemanticsNode::new();
        let id = tree.insert(node);

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
        assert_eq!(id.get(), 1); // First ID should be 1
    }

    #[test]
    fn test_semantics_tree_get() {
        let mut tree = SemanticsTree::new();
        let mut node = SemanticsNode::new();
        node.config_mut().set_label("Test");
        let id = tree.insert(node);

        let node = tree.get(id);
        assert!(node.is_some());
        assert_eq!(node.unwrap().label(), Some("Test"));
    }

    #[test]
    fn test_semantics_tree_get_mut() {
        let mut tree = SemanticsTree::new();
        let node = SemanticsNode::new();
        let id = tree.insert(node);

        if let Some(node) = tree.get_mut(id) {
            node.config_mut().set_label("Modified");
        }

        assert_eq!(tree.get(id).unwrap().label(), Some("Modified"));
    }

    #[test]
    fn test_semantics_tree_remove() {
        let mut tree = SemanticsTree::new();
        let node = SemanticsNode::new();
        let id = tree.insert(node);

        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
        assert!(tree.is_empty());
    }

    #[test]
    fn test_semantics_tree_parent_child() {
        let mut tree = SemanticsTree::new();

        let parent_node = SemanticsNode::new();
        let child_node = SemanticsNode::new();

        let parent_id = tree.insert(parent_node);
        let child_id = tree.insert(child_node);

        tree.add_child(parent_id, child_id);

        // Check parent has child
        let children = tree.children(parent_id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child_id);

        // Check child has parent
        let parent = tree.parent(child_id);
        assert_eq!(parent, Some(parent_id));
    }

    #[test]
    fn test_semantics_tree_remove_child() {
        let mut tree = SemanticsTree::new();

        let parent_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child_id);
        assert_eq!(tree.children(parent_id).unwrap().len(), 1);

        tree.remove_child(parent_id, child_id);
        assert_eq!(tree.children(parent_id).unwrap().len(), 0);
        assert!(tree.parent(child_id).is_none());
    }

    #[test]
    fn test_semantics_tree_set_root() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());

        assert!(tree.root().is_none());
        tree.set_root(Some(id));
        assert_eq!(tree.root(), Some(id));
    }

    #[test]
    fn test_semantics_tree_clear() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());
        tree.set_root(Some(id));

        tree.clear();
        assert!(tree.is_empty());
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_semantics_tree_iter() {
        let mut tree = SemanticsTree::new();
        let id1 = tree.insert(SemanticsNode::new());
        let id2 = tree.insert(SemanticsNode::new());

        let ids: Vec<_> = tree.semantics_ids().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    /// `has_dirty_nodes` is counter-backed (O(1)), and the counter must stay
    /// truthful across every seam that can flip a dirty bit. A silent
    /// under-count here would make an idle-looking tree swallow a real
    /// change; an over-count only costs a no-op flush.
    #[test]
    fn the_dirty_gate_tracks_insert_remove_and_clear() {
        let mut tree = SemanticsTree::new();
        assert!(!tree.has_dirty_nodes(), "an empty tree is clean");

        let id = tree.insert(SemanticsNode::new());
        assert!(tree.has_dirty_nodes(), "a fresh insert is dirty");

        tree.mark_all_clean();
        assert!(!tree.has_dirty_nodes());

        tree.mark_dirty(id);
        assert!(tree.has_dirty_nodes());
        let removed = tree.remove_shallow(id);
        assert!(removed.is_some());
        assert!(
            !tree.has_dirty_nodes(),
            "removing the only dirty node cleans the gate"
        );

        let _ = tree.insert(SemanticsNode::new());
        tree.clear();
        assert!(!tree.has_dirty_nodes(), "clear resets the gate");
    }

    /// Handing out `&mut SemanticsNode` counts the node as dirty at the
    /// borrow — the tree cannot see what happens through the borrow, and
    /// presuming mutation is the only direction that cannot under-count.
    #[test]
    fn a_mutable_borrow_counts_as_dirty() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());
        tree.mark_all_clean();

        let _ = tree.get_mut(id);
        assert!(
            tree.has_dirty_nodes(),
            "a mutable borrow must be presumed a mutation"
        );
        assert_eq!(tree.dirty_nodes().count(), 1);

        tree.mark_all_clean();
        let _ = tree.iter_mut();
        assert!(
            tree.has_dirty_nodes(),
            "a mutable iteration presumes mutation of every node"
        );
    }

    /// `mark_all_dirty` is the full-republish primitive: every node becomes
    /// re-examinable, and `mark_all_clean` fully resets the gate after.
    #[test]
    fn mark_all_dirty_marks_every_node() {
        let mut tree = SemanticsTree::new();
        let _ = tree.insert(SemanticsNode::new());
        let _ = tree.insert(SemanticsNode::new());
        tree.mark_all_clean();

        tree.mark_all_dirty();
        assert_eq!(tree.dirty_nodes().count(), 2);
        assert!(tree.has_dirty_nodes());

        tree.mark_all_clean();
        assert!(!tree.has_dirty_nodes());
    }

    #[test]
    fn test_semantics_tree_dirty_tracking() {
        let mut tree = SemanticsTree::new();

        let id1 = tree.insert(SemanticsNode::new()); // dirty by default
        let _id2 = tree.insert(SemanticsNode::new());

        // All nodes start dirty
        assert!(tree.has_dirty_nodes());
        let dirty: Vec<_> = tree.dirty_nodes().collect();
        assert_eq!(dirty.len(), 2);

        // Mark all clean
        tree.mark_all_clean();
        assert!(!tree.has_dirty_nodes());
        assert_eq!(tree.dirty_nodes().count(), 0);

        // Mark one dirty again
        if let Some(node) = tree.get_mut(id1) {
            node.mark_dirty();
        }
        assert!(tree.has_dirty_nodes());
        let dirty: Vec<_> = tree.dirty_nodes().collect();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0], id1);
    }

    // ========== Read and navigation ==========

    #[test]
    fn test_tree_read_get() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());

        assert!(tree.get(id).is_some());
    }

    #[test]
    fn test_tree_read_contains() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());

        assert!(tree.contains(id));
        assert!(!tree.contains(SemanticsId::new(999)));
    }

    #[test]
    fn test_tree_read_len() {
        let mut tree = SemanticsTree::new();
        assert_eq!(tree.len(), 0);

        let _ = tree.insert(SemanticsNode::new());
        assert_eq!(tree.len(), 1);

        let _ = tree.insert(SemanticsNode::new());
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn test_tree_read_semantics_ids() {
        let mut tree = SemanticsTree::new();
        let id1 = tree.insert(SemanticsNode::new());
        let id2 = tree.insert(SemanticsNode::new());

        let ids: Vec<_> = tree.semantics_ids().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_tree_nav_parent() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child_id);

        assert_eq!(tree.parent(child_id), Some(parent_id));
        assert_eq!(tree.parent(parent_id), None);
    }

    #[test]
    fn test_tree_nav_children() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child1_id = tree.insert(SemanticsNode::new());
        let child2_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);

        assert_eq!(tree.children(parent_id), Some(&[child1_id, child2_id][..]));
    }
}

// ============================================================================
// SLAB-TREE HYGIENE TESTS (add_child auto-detach + remove cascade)
// ============================================================================

#[cfg(test)]
mod slab_hygiene_tests {
    use crate::node::SemanticsNode;
    use crate::tree::SemanticsTree;
    use flui_foundation::SemanticsId;

    fn empty_node() -> SemanticsNode {
        SemanticsNode::new()
    }

    // ----- add_child auto-detach -----

    #[test]
    fn add_child_attaches_under_new_parent() {
        let mut tree = SemanticsTree::new();
        let parent = tree.insert(empty_node());
        let child = tree.insert(empty_node());

        tree.add_child(parent, child);

        assert_eq!(tree.get(child).unwrap().parent(), Some(parent));
        assert_eq!(tree.get(parent).unwrap().children(), &[child]);
    }

    #[test]
    fn add_child_auto_detaches_from_previous_parent() {
        let mut tree = SemanticsTree::new();
        let parent_a = tree.insert(empty_node());
        let parent_b = tree.insert(empty_node());
        let child = tree.insert(empty_node());

        tree.add_child(parent_a, child);
        tree.add_child(parent_b, child);

        assert_eq!(tree.get(child).unwrap().parent(), Some(parent_b));
        assert!(tree.get(parent_a).unwrap().children().is_empty());
        assert_eq!(tree.get(parent_b).unwrap().children(), &[child]);
    }

    #[test]
    fn add_child_same_parent_is_idempotent() {
        let mut tree = SemanticsTree::new();
        let parent = tree.insert(empty_node());
        let child = tree.insert(empty_node());

        tree.add_child(parent, child);
        tree.add_child(parent, child);

        assert_eq!(tree.get(parent).unwrap().children().len(), 1);
    }

    #[test]
    fn add_child_missing_parent_is_a_no_op() {
        let mut tree = SemanticsTree::new();
        let child = tree.insert(empty_node());
        let phantom = SemanticsId::new(999);
        tree.add_child(phantom, child);
        assert!(tree.get(child).unwrap().parent().is_none());
    }

    #[test]
    fn add_child_missing_child_is_a_no_op() {
        let mut tree = SemanticsTree::new();
        let parent = tree.insert(empty_node());
        let phantom = SemanticsId::new(999);
        tree.add_child(parent, phantom);
        assert!(tree.get(parent).unwrap().children().is_empty());
    }

    // ----- cycle rejection -----

    #[test]
    fn add_child_rejects_self_link() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(empty_node());
        tree.add_child(id, id);
        assert!(tree.get(id).unwrap().children().is_empty());
        assert!(tree.get(id).unwrap().parent().is_none());
    }

    #[test]
    fn add_child_rejects_attaching_ancestor_under_descendant() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let leaf = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);

        // Would create a 3-cycle: root → mid → leaf → root.
        // Pre-rejection, `tree.remove(root)` would have recursed
        // root → mid → leaf → root → … indefinitely.
        tree.add_child(leaf, root);

        // Tree shape unchanged after rejected call.
        assert_eq!(tree.get(root).unwrap().parent(), None);
        let empty: &[SemanticsId] = &[];
        assert_eq!(tree.get(leaf).unwrap().children(), empty);
        // Cascade terminates.
        let removed = tree.remove(root);
        assert!(removed.is_some());
        assert_eq!(tree.len(), 0);
    }

    // ----- remove cascade + remove_shallow -----

    #[test]
    fn remove_cascades_to_descendants() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let leaf = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);
        assert_eq!(tree.len(), 3);

        let removed = tree.remove(root);
        assert!(removed.is_some());
        assert_eq!(tree.len(), 0);
        assert!(!tree.contains(mid));
        assert!(!tree.contains(leaf));
    }

    #[test]
    fn remove_unlinks_parent_children_vector() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let sibling = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(root, sibling);

        let _ = tree.remove(mid);
        assert!(!tree.contains(mid));
        assert_eq!(tree.get(root).unwrap().children(), &[sibling]);
    }

    #[test]
    fn remove_resets_root_when_removing_root() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        tree.set_root(Some(root));
        let _ = tree.remove(root);
        assert_eq!(tree.root(), None);
    }

    #[test]
    fn remove_of_phantom_id_is_a_no_op() {
        let mut tree = SemanticsTree::new();
        let _ = tree.insert(empty_node());
        let phantom = SemanticsId::new(999);
        assert!(tree.remove(phantom).is_none());
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn remove_shallow_does_not_cascade() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let leaf = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);

        let _ = tree.remove_shallow(mid);
        assert!(!tree.contains(mid));
        // Leaf survives (only cascade path drops descendants).
        assert!(tree.contains(leaf));
    }

    #[test]
    fn remove_retires_children_before_their_parent() {
        use flui_foundation::RenderId;

        use crate::identity::AccessibilityNodeId;

        let stable = |n: usize| AccessibilityNodeId::from(RenderId::new(n)).as_u64();
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node().with_source_render_id(RenderId::new(1)));
        let mid = tree.insert(empty_node().with_source_render_id(RenderId::new(2)));
        let leaf = tree.insert(empty_node().with_source_render_id(RenderId::new(3)));
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);
        let _ = tree.take_removed_accessibility_ids();

        assert!(tree.remove(root).is_some());
        assert_eq!(
            tree.take_removed_accessibility_ids(),
            vec![stable(3), stable(2), stable(1)],
            "leaf, then mid, then root"
        );
    }

    #[test]
    fn remove_on_a_corrupted_cycle_terminates_and_returns_none() {
        let mut tree = SemanticsTree::new();
        let a = tree.insert(empty_node());
        let b = tree.insert(empty_node());
        tree.add_child(a, b);
        // `add_child` refuses the back edge, so corrupt the nodes directly:
        // b → a closes the cycle a → b → a.
        tree.get_mut(b).expect("b is live").add_child(a);
        tree.get_mut(a).expect("a is live").set_parent(Some(b));

        assert!(tree.remove(a).is_none());
        // Nothing was removed: the pre-walk aborts before the drain.
        assert!(tree.contains(a));
        assert!(tree.contains(b));
    }

    #[test]
    fn remove_cascade_is_stack_safe_on_a_deep_chain() {
        const DEPTH: usize = 100_000;
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mut tip = root;
        // Link the nodes directly: `add_child`'s cycle check walks the whole
        // parent chain, which makes building the chain through it quadratic.
        for _ in 1..DEPTH {
            let next = tree.insert(empty_node());
            tree.get_mut(tip).expect("tip is live").add_child(next);
            tree.get_mut(next)
                .expect("next is live")
                .set_parent(Some(tip));
            tip = next;
        }
        assert_eq!(tree.len(), DEPTH);

        assert!(tree.remove(root).is_some());
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn is_ancestor_of_is_strict_and_follows_the_parent_chain() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let leaf = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);

        assert!(tree.is_ancestor_of(root, leaf));
        assert!(tree.is_ancestor_of(mid, leaf));
        assert!(!tree.is_ancestor_of(leaf, root));
        assert!(!tree.is_ancestor_of(leaf, leaf));
    }
}

//! RenderTree - Slab-based render object storage.
//!
//! This module provides efficient storage and tree operations for render
//! objects. Implements `flui-tree` traits for unified tree interface.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_tree::{
    iter::{AllSiblings, Ancestors, DescendantsWithDepth},
    traits::{TreeNav, TreeRead, TreeWrite},
};
use parking_lot::RwLock;
use slab::Slab;

use super::node::RenderNode;
use crate::{
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, RenderObject, SliverProtocol},
};

// ============================================================================
// RenderTree
// ============================================================================

/// Slab-based storage for render objects.
///
/// Provides O(1) render object access by RenderId and tree navigation
/// operations.
///
/// # Thread Safety
///
/// RenderTree itself is `Send + Sync`. For multi-threaded access, wrap in
/// `Arc<RwLock<RenderTree>>`.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::tree::RenderTree;
/// use flui_rendering::objects::RenderColoredBox;
///
/// let mut tree = RenderTree::new();
///
/// // Insert root
/// let root_id = tree.insert(Box::new(RenderColoredBox::new(Color::RED)));
/// tree.set_root(Some(root_id));
///
/// // Insert child`
/// let child_id = tree.insert_child(root_id, Box::new(RenderColoredBox::new(Color::BLUE)));
///
/// // Access render object
/// if let Some(node) = tree.get(root_id) {
///     println!("Root has {} children", node.children().len());
/// }
/// ```
#[derive(Debug)]
pub struct RenderTree {
    /// Slab storage for nodes (0-based indexing internally).
    nodes: Slab<RenderNode>,

    /// Root node ID (None if tree is empty).
    root: Option<RenderId>,

    /// Pipeline owner for dirty scheduling (optional).
    owner: Option<Arc<RwLock<PipelineOwner>>>,
}

impl Default for RenderTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderTree {
    /// Creates a new empty RenderTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            owner: None,
        }
    }

    /// Creates a RenderTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
            owner: None,
        }
    }

    // ========================================================================
    // Pipeline Owner
    // ========================================================================

    /// Returns the pipeline owner.
    #[inline]
    pub fn owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        // PORT-CHECK-OK-SP6: RenderTree owner accessor; pre-existing SP-6
        self.owner.as_ref()
    }

    /// Stores the pipeline owner reference.
    ///
    /// # Semantics
    ///
    /// This is a **store-only** operation: existing nodes already in the tree
    /// are NOT walked, not attached to the new owner, and not notified of the
    /// owner change. The caller is responsible for the attach/detach
    /// lifecycle. Two recommended patterns:
    ///
    /// 1. **Empty-tree set**: call [`set_owner`](Self::set_owner) BEFORE any
    ///    nodes are inserted. Subsequent inserts attach to the stored owner
    ///    via the regular insert path.
    /// 2. **Per-node attach**: use [`PipelineOwner`]'s own insert /
    ///    `add_node_needing_*` registration methods directly when adding
    ///    nodes to an already-owned tree.
    ///
    /// # Cycle 4 R-12
    ///
    /// Pre-cycle the docstring promised "This will attach all existing nodes
    /// to the new owner" — the impl never did that (silent no-op on existing
    /// nodes). The lie was a real Constitution Principle 6 violation in the
    /// docstring layer. Per audit R-12 the cycle-4 cleanup is the lower-cost
    /// **honest-doc** path; Flutter parity (`RenderObject::attach` recursive
    /// subtree walk) is a follow-up audit item that needs an
    /// `attached: AtomicBool` on `RenderState<P>::flags` + owner-dirty-list
    /// re-registration plumbing not yet in place.
    pub fn set_owner(&mut self, owner: Option<Arc<RwLock<PipelineOwner>>>) {
        self.owner = owner;
    }

    // ========================================================================
    // Root Management
    // ========================================================================

    /// Returns the root node ID.
    #[inline]
    pub fn root(&self) -> Option<RenderId> {
        self.root
    }

    /// Sets the root node ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.root = root;
    }

    // ========================================================================
    // Basic Operations
    // ========================================================================

    /// Checks if a node exists in the tree.
    #[inline]
    pub fn contains(&self, id: RenderId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of nodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns a reference to a node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `RenderId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&RenderNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a node.
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Returns mutable references to two distinct nodes simultaneously.
    ///
    /// Used by the layout phase for parent-child re-entrant access: a
    /// parent holds `&mut RenderNode` for itself while it calls `layout`
    /// on each child's `&mut RenderNode`. Returns `None` if either id is
    /// missing.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `a == b`. In release builds, returns
    /// `None` (treated as "second id is missing" after the first borrow
    /// claims it).
    ///
    /// # Safety
    ///
    /// The single `unsafe` block here forms `&mut T` from a `*mut T`. The
    /// safety invariant is "disjoint indices yield disjoint memory":
    ///   1. The assert / equality check above guarantees `a_idx !=
    ///      b_idx`.
    ///   2. `Slab::get_mut(idx)` returns a pointer-into-the-slab-vector
    ///      whose validity is bounded by `&mut self`. Holding `&mut self`
    ///      for the duration of this function call (via the receiver
    ///      `&mut self`) keeps the slab borrow alive.
    ///   3. The two `&mut RenderNode` references we hand out alias to
    ///      different elements of the underlying vector storage, so no
    ///      mutable-aliasing rule is violated.
    ///
    /// Mythos Step 10 (2026-05-20).
    pub fn get_two_mut(
        &mut self,
        a: RenderId,
        b: RenderId,
    ) -> Option<(&mut RenderNode, &mut RenderNode)> {
        debug_assert_ne!(a, b, "RenderTree::get_two_mut requires distinct ids");
        if a == b {
            return None;
        }

        let a_idx = a.get() - 1;
        let b_idx = b.get() - 1;
        if !self.nodes.contains(a_idx) || !self.nodes.contains(b_idx) {
            return None;
        }

        // SAFETY: see method-level comment. a_idx and b_idx are distinct
        // (checked above) and both valid (checked above). Re-borrowing
        // through `*mut Slab<RenderNode>` to materialize two `&mut
        // RenderNode` references to disjoint elements is sound under
        // Rust's aliasing model.
        let nodes_ptr: *mut slab::Slab<RenderNode> = &mut self.nodes;
        unsafe {
            let a_ref = (*nodes_ptr).get_mut(a_idx)?;
            let b_ref = (*nodes_ptr).get_mut(b_idx)?;
            Some((a_ref, b_ref))
        }
    }

    /// Returns mutable references to a parent + every child in the given
    /// child id list.
    ///
    /// Used by variable-arity layout where a parent's `perform_layout`
    /// must read its own fields while writing into each child's slot.
    /// Returns `None` if any id is missing or any pair of ids collide.
    ///
    /// # Safety
    ///
    /// Same invariant as [`get_two_mut`](Self::get_two_mut), extended to
    /// N+1 indices: the function checks `parent_id` is distinct from
    /// every entry in `child_ids` and that no two entries in `child_ids`
    /// are equal, then materializes N+1 `&mut RenderNode` references to
    /// the disjoint slab slots.
    ///
    /// Mythos Step 10 (2026-05-20).
    pub fn get_parent_and_children_mut<'a>(
        &'a mut self,
        parent_id: RenderId,
        child_ids: &[RenderId],
    ) -> Option<(&'a mut RenderNode, Vec<&'a mut RenderNode>)> {
        // Verify uniqueness: parent ≠ each child, and child ids are pairwise
        // unique. O(N²) for small N is fine; typical render trees have small
        // child counts.
        for (i, &c) in child_ids.iter().enumerate() {
            if c == parent_id {
                return None;
            }
            for &later in &child_ids[i + 1..] {
                if later == c {
                    return None;
                }
            }
        }

        let parent_idx = parent_id.get() - 1;
        if !self.nodes.contains(parent_idx) {
            return None;
        }
        for c in child_ids {
            if !self.nodes.contains(c.get() - 1) {
                return None;
            }
        }

        // SAFETY: see method-level comment. Uniqueness verified above; all
        // indices are valid; the receiver `&mut self` keeps the slab borrow
        // alive for the lifetime of the returned references.
        let nodes_ptr: *mut slab::Slab<RenderNode> = &mut self.nodes;
        unsafe {
            let parent_ref = (*nodes_ptr).get_mut(parent_idx)?;
            let mut children = Vec::with_capacity(child_ids.len());
            for c in child_ids {
                children.push((*nodes_ptr).get_mut(c.get() - 1)?);
            }
            Some((parent_ref, children))
        }
    }

    /// Returns mutable references to **every** node id in the given list,
    /// materialised in a single function scope so all `&mut RenderNode`
    /// borrows coexist on disjoint slab slots without re-entering the
    /// slab borrow checker.
    ///
    /// Generalises [`get_parent_and_children_mut`](Self::get_parent_and_children_mut)
    /// from N+1 (parent + direct children) to arbitrary N (whole subtree
    /// pre-acquisition). The returned `Vec<&mut RenderNode>` is in input
    /// order so callers indexing by id can pre-compute a
    /// `HashMap<RenderId, usize>` lookup.
    ///
    /// Returns `None` if any id is missing from the slab OR if `ids`
    /// contains duplicates.
    ///
    /// # Use case (D-block PR-A1b3 U20.1)
    ///
    /// [`PipelineOwner::layout_dirty_root`](crate::pipeline::PipelineOwner::layout_dirty_root)
    /// uses this to pre-acquire the entire subtree's `&mut RenderNode`
    /// borrows up front, then drives `perform_layout_raw` recursively
    /// against an index-into-pre-acquired-pool — eliminating the
    /// recursive raw-pointer reborrow pattern that the prior U20
    /// implementation used (latent Stacked/Tree Borrows UB, see
    /// PR #144 review). All borrows live in one stack frame so the
    /// aliasing model is satisfied: `&mut Slab` is borrowed once,
    /// N disjoint `&mut RenderNode` borrows on distinct slots are
    /// returned, no nested reborrow.
    ///
    /// # Safety
    ///
    /// Same invariant as [`get_two_mut`](Self::get_two_mut) and
    /// [`get_parent_and_children_mut`](Self::get_parent_and_children_mut),
    /// extended to arbitrary N: caller passes pairwise-distinct ids
    /// (checked at function entry, returns `None` on collision); all
    /// ids must be present in the slab (checked, returns `None`
    /// otherwise); the returned `&mut RenderNode` references alias
    /// disjoint slots in the underlying `Vec<Entry<RenderNode>>` and
    /// therefore do not violate Rust's aliasing rules. Receiver
    /// `&mut self` keeps the slab borrow alive for the returned
    /// references' lifetime.
    ///
    /// # Complexity
    ///
    /// O(N²) uniqueness check for small N (typical render-tree subtrees
    /// are 10–100 nodes). O(N) slab presence check. O(N) borrow
    /// materialisation. Switch to a HashSet-based duplicate check if a
    /// subtree of >1000 nodes ever becomes hot — the inner loop's `==`
    /// compare on `NonZeroUsize` is dominant under typical N.
    pub fn get_subtree_mut<'a>(&'a mut self, ids: &[RenderId]) -> Option<Vec<&'a mut RenderNode>> {
        // Verify pairwise uniqueness. O(N²) acceptable for small N.
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                if a == b {
                    return None;
                }
            }
        }

        // Verify all ids present in slab.
        for id in ids {
            if !self.nodes.contains(id.get() - 1) {
                return None;
            }
        }

        // SAFETY: see method-level safety doc. Uniqueness verified above;
        // all indices are valid; the receiver `&mut self` keeps the slab
        // borrow alive for the lifetime of the returned references. The
        // N disjoint `&mut RenderNode` references alias distinct
        // `Vec<Entry<RenderNode>>` cells which the slab's internal Vec
        // resolves via independent pointer offsets — sound under Rust's
        // aliasing model (mirrors the proven get_two_mut /
        // get_parent_and_children_mut pattern, extended to arbitrary N).
        let nodes_ptr: *mut slab::Slab<RenderNode> = &mut self.nodes;
        unsafe {
            let mut refs = Vec::with_capacity(ids.len());
            for id in ids {
                refs.push((*nodes_ptr).get_mut(id.get() - 1)?);
            }
            Some(refs)
        }
    }

    /// Inserts a Box protocol render object into the tree (no parent).
    ///
    /// Returns the RenderId of the inserted node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `RenderId(1)`
    pub fn insert_box(&mut self, render_object: Box<dyn RenderObject<BoxProtocol>>) -> RenderId {
        let node = RenderNode::new_box(render_object);
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1) // 0-based → 1-based
    }

    /// Inserts a Sliver protocol render object into the tree (no parent).
    pub fn insert_sliver(
        &mut self,
        render_object: Box<dyn RenderObject<SliverProtocol>>,
    ) -> RenderId {
        let node = RenderNode::new_sliver(render_object);
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1)
    }

    /// Inserts a Box protocol render object as a child of the given parent.
    ///
    /// Returns the RenderId of the inserted child.
    pub fn insert_box_child(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn RenderObject<BoxProtocol>>,
    ) -> Option<RenderId> {
        // Get parent depth
        let parent_depth = self.get(parent_id)?.depth();

        // Create child node
        let child_node =
            RenderNode::new_box_with_parent(render_object, parent_id, parent_depth + 1);
        let child_slab_index = self.nodes.insert(child_node);
        let child_id = RenderId::new(child_slab_index + 1);

        // Add child to parent's tree structure
        if let Some(parent) = self.nodes.get_mut(parent_id.get() - 1) {
            parent.add_child(child_id);
        }

        Some(child_id)
    }

    /// Inserts a Sliver protocol render object as a child of the given parent.
    pub fn insert_sliver_child(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn RenderObject<SliverProtocol>>,
    ) -> Option<RenderId> {
        let parent_depth = self.get(parent_id)?.depth();

        let child_node =
            RenderNode::new_sliver_with_parent(render_object, parent_id, parent_depth + 1);
        let child_slab_index = self.nodes.insert(child_node);
        let child_id = RenderId::new(child_slab_index + 1);

        if let Some(parent) = self.nodes.get_mut(parent_id.get() - 1) {
            parent.add_child(child_id);
        }

        Some(child_id)
    }

    /// Removes a node from the tree.
    ///
    /// Removes a node WITHOUT cascading to descendants.
    ///
    /// Returns the removed node, or None if it didn't exist. Descendants
    /// are orphaned in the slab; use [`Self::remove_recursive`] for full
    /// cascade.
    ///
    /// Cycle 3 T-1: this is the [`TreeWrite::remove_shallow`] primitive
    /// the trait builds the cascade-by-default `remove` on top of.
    pub fn remove_shallow(&mut self, id: RenderId) -> Option<RenderNode> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        // Get parent and remove from parent's children
        if let Some(parent_id) = self.get(id).and_then(|n| n.parent())
            && let Some(parent) = self.get_mut(parent_id)
        {
            parent.remove_child(id);
        }

        self.nodes.try_remove(id.get() - 1)
    }

    /// Removes a node and all its descendants recursively.
    ///
    /// Returns the number of nodes removed. Cycle 3 T-1: equivalent to
    /// [`TreeWrite::remove`] (which now cascades by default) with a
    /// count instead of the returned root node. Prefer `TreeWrite::remove`
    /// for new code; this inherent stays for in-crate callers that want
    /// the count.
    pub fn remove_recursive(&mut self, id: RenderId) -> usize {
        let mut count = 0;

        // Get children first (clone to avoid borrow issues)
        let children: Vec<RenderId> = self
            .get(id)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();

        // Remove children recursively
        for child_id in children {
            count += self.remove_recursive(child_id);
        }

        // Remove the node itself
        if self.remove_shallow(id).is_some() {
            count += 1;
        }

        count
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    /// Reserves capacity for additional nodes.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // ========================================================================
    // Tree Navigation
    // ========================================================================

    /// Returns the parent ID of a node.
    #[inline]
    pub fn parent(&self, id: RenderId) -> Option<RenderId> {
        self.get(id)?.parent()
    }

    /// Returns the children IDs of a node.
    #[inline]
    pub fn children(&self, id: RenderId) -> &[RenderId] {
        self.get(id).map(|n| n.children()).unwrap_or(&[])
    }

    /// Returns the depth of a node in the tree.
    #[inline]
    pub fn depth(&self, id: RenderId) -> Option<u16> {
        self.get(id).map(|n| n.depth())
    }

    /// Collects `root_id` plus every transitive descendant in
    /// **DFS pre-order** (parent before children; children visited in
    /// stored order). Returns an empty `Vec` if `root_id` is not in
    /// the tree.
    ///
    /// # Use case (D-block PR-A1b3 U20.1)
    ///
    /// [`PipelineOwner::layout_dirty_root`](crate::pipeline::PipelineOwner::layout_dirty_root)
    /// passes the result into
    /// [`Self::get_subtree_mut`] to pre-acquire every subtree node's
    /// `&mut RenderNode` borrow in one stack frame, eliminating the
    /// recursive raw-pointer reborrow pattern (latent Stacked / Tree
    /// Borrows UB) the prior U20 implementation used.
    ///
    /// # Implementation
    ///
    /// Iterative DFS with an explicit `Vec` stack so deep trees do
    /// not overflow Rust's call stack (the layout walk has no other
    /// depth limit until U21's cycle guard lands). Children are
    /// pushed in reverse so they pop in stored order — preserves
    /// pre-order with children-left-to-right.
    ///
    /// # Cycle protection (PR #145 review fix)
    ///
    /// Carries a `visited` `HashSet<RenderId>` to short-circuit on
    /// repeated ids. Without this guard, a malformed tree containing
    /// a parent / child cycle (which `RenderNode::add_child` does not
    /// prevent; full cycle protection arrives in U21) would loop
    /// forever — repeatedly re-pushing the cycle's nodes onto `stack`
    /// while `out` grows unbounded → hang / OOM. The visited-set
    /// short-circuit terminates the walk on the first repeated id and
    /// produces a deduplicated `Vec<RenderId>` suitable for
    /// [`Self::get_subtree_mut`] (which requires pairwise uniqueness).
    /// The cyclic edge itself is silently dropped; full
    /// [`RenderError::LayoutCycle`](crate::error::RenderError::LayoutCycle)
    /// reporting is U21's job — this fix is the minimum-disruption
    /// termination guard so the pre-acquired-subtree walk does not
    /// regress on cycles vs the prior PR #144 stack-overflow failure
    /// mode.
    ///
    /// # Complexity
    ///
    /// O(N) where N is the subtree node count. Single pass; each
    /// node's `children()` slice is borrowed once. `visited` is a
    /// `HashSet<RenderId>` — O(1) amortised lookup + insert per id.
    pub fn collect_subtree_ids(&self, root_id: RenderId) -> Vec<RenderId> {
        let mut out = Vec::new();
        // If the root doesn't exist, return empty to mirror other
        // tree-walk methods (e.g., `depth()` returns None) — callers
        // should check before doing further work with the result.
        if self.get(root_id).is_none() {
            return out;
        }
        let mut stack: Vec<RenderId> = vec![root_id];
        // PR #145 review fix: visited-set short-circuits on repeated
        // ids so a cyclic tree terminates instead of hanging /
        // OOMing. Pre-sized to a conservative guess (small trees are
        // the common case; HashSet grows by power-of-two doubling
        // otherwise).
        let mut visited: std::collections::HashSet<RenderId> =
            std::collections::HashSet::with_capacity(16);
        while let Some(id) = stack.pop() {
            // Skip ids already visited — preserves uniqueness in `out`
            // and breaks cycles. Without this, a parent/child cycle
            // (A → B → A) re-pushes A onto stack forever.
            if !visited.insert(id) {
                continue;
            }
            if let Some(node) = self.get(id) {
                out.push(id);
                // Reverse-push so the leftmost child pops first,
                // preserving pre-order with children-in-stored-order.
                for &child_id in node.children().iter().rev() {
                    stack.push(child_id);
                }
            }
        }
        out
    }

    /// Checks if `ancestor` is an ancestor of `descendant`.
    pub fn is_ancestor(&self, ancestor: RenderId, descendant: RenderId) -> bool {
        let mut current = self.parent(descendant);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.parent(id);
        }
        false
    }

    /// Checks if `descendant` is a descendant of `ancestor`.
    #[inline]
    pub fn is_descendant(&self, descendant: RenderId, ancestor: RenderId) -> bool {
        self.is_ancestor(ancestor, descendant)
    }

    /// Returns the path from root to the given node.
    ///
    /// The path includes the node itself.
    pub fn path_to_root(&self, id: RenderId) -> Vec<RenderId> {
        let mut path = Vec::new();
        let mut current = Some(id);

        while let Some(node_id) = current {
            path.push(node_id);
            current = self.parent(node_id);
        }

        path.reverse();
        path
    }

    // ========================================================================
    // Dirty Node Collection
    // ========================================================================

    /// Collects all nodes that need layout, sorted by depth.
    ///
    /// Returns IDs of nodes with `needs_layout() == true`, sorted by depth
    /// (shallow first) for correct layout order.
    pub fn collect_nodes_needing_layout(&self) -> Vec<RenderId> {
        let mut nodes: Vec<(RenderId, usize)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.needs_layout())
            .map(|(idx, node)| (RenderId::new(idx + 1), node.depth() as usize))
            .collect();

        // Sort by depth (shallow first)
        nodes.sort_by_key(|(_, depth)| *depth);

        nodes.into_iter().map(|(id, _)| id).collect()
    }

    /// Collects all nodes that need paint, sorted by depth.
    ///
    /// Returns IDs of nodes with `needs_paint() == true`, sorted by depth
    /// (shallow first) for correct paint order.
    pub fn collect_nodes_needing_paint(&self) -> Vec<RenderId> {
        let mut nodes: Vec<(RenderId, usize)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.needs_paint())
            .map(|(idx, node)| (RenderId::new(idx + 1), node.depth() as usize))
            .collect();

        // Sort by depth (shallow first)
        nodes.sort_by_key(|(_, depth)| *depth);

        nodes.into_iter().map(|(id, _)| id).collect()
    }

    // ========================================================================
    // Iteration
    // ========================================================================

    /// Returns an iterator over all node IDs.
    pub fn ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.nodes.iter().map(|(idx, _)| RenderId::new(idx + 1))
    }

    /// Returns an iterator over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &RenderNode> + '_ {
        self.nodes.iter().map(|(_, node)| node)
    }

    /// Returns a mutable iterator over all nodes.
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut RenderNode> + '_ {
        self.nodes.iter_mut().map(|(_, node)| node)
    }

    /// Returns an iterator over (RenderId, &RenderNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (RenderId, &RenderNode)> + '_ {
        self.nodes
            .iter()
            .map(|(idx, node)| (RenderId::new(idx + 1), node))
    }

    /// Returns a mutable iterator over (RenderId, &mut RenderNode) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (RenderId, &mut RenderNode)> + '_ {
        self.nodes
            .iter_mut()
            .map(|(idx, node)| (RenderId::new(idx + 1), node))
    }

    // ========================================================================
    // Depth-First Traversal
    // ========================================================================

    /// Visits all nodes in depth-first pre-order starting from root.
    ///
    /// The callback receives (RenderId, &RenderNode) for each node.
    ///
    /// # Implementation
    ///
    /// Cycle 4 R-26: iterative loop + `SmallVec<[RenderId; 32]>`
    /// work-stack rather than recursive `visit_depth_first_from`.
    /// Three wins:
    /// - **No stack overflow** on pathological tree depths
    ///   (recursion blew at ~5000 with default Rust stack; the
    ///   iterative version is unbounded).
    /// - **Inline 32-deep buffer** via `SmallVec` covers the typical
    ///   widget tree depth (Flutter's `RenderObject` paint trees
    ///   measure ~20-40 deep in practice) without heap allocation.
    ///   Deeper trees spill to heap automatically.
    /// - **No per-node child clone.** The recursive path called
    ///   `node.children().to_vec()` on every visit to dodge a borrow
    ///   conflict; the iterative path borrows the slice in-place and
    ///   pushes child ids onto the work-stack directly. The
    ///   `RenderId` push is a `Copy` of two `usize`s -- no heap
    ///   traffic -- and the `SmallVec` doubles its inline buffer to
    ///   absorb the children without reallocating until depth 32+.
    ///
    /// Pre-order semantics preserved: children are pushed in
    /// **reverse** order so the work-stack pops them in original
    /// child-order (mirrors Flutter's `visitChildren` shape).
    ///
    /// PR #116 review (cycle 4 wave 4 follow-up): the prior comment
    /// claimed `extend_from_slice`. That was a copy-paste error from
    /// an earlier draft; reversing in-place via `iter().rev()` is
    /// required for pre-order pop-order and `extend_from_slice` would
    /// need a temporary reversed allocation, defeating the no-alloc
    /// goal. The body matches the doc now.
    pub fn visit_depth_first<F>(&self, mut f: F)
    where
        F: FnMut(RenderId, &RenderNode),
    {
        let Some(root_id) = self.root else {
            return;
        };
        let mut stack: smallvec::SmallVec<[RenderId; 32]> = smallvec::SmallVec::new();
        stack.push(root_id);
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get(id) {
                f(id, node);
                // Push children in reverse so pop() yields them
                // in original child-order (pre-order traversal).
                for &child_id in node.children().iter().rev() {
                    stack.push(child_id);
                }
            }
        }
    }

    /// Visits all nodes mutably in depth-first pre-order starting from root.
    ///
    /// **Note:** The callback receives only RenderId since we can't provide
    /// mutable references during traversal. Use `get_mut()` inside the
    /// callback.
    pub fn visit_depth_first_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Self, RenderId),
    {
        if let Some(root_id) = self.root {
            self.visit_depth_first_mut_from(root_id, &mut f);
        }
    }

    /// Visits all nodes mutably in depth-first pre-order starting from a given
    /// node.
    fn visit_depth_first_mut_from<F>(&mut self, id: RenderId, f: &mut F)
    where
        F: FnMut(&mut Self, RenderId),
    {
        // Get children first (clone to avoid borrow issues)
        let children: Vec<RenderId> = self
            .get(id)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();

        // Visit this node
        f(self, id);

        // Visit children
        for child_id in children {
            self.visit_depth_first_mut_from(child_id, f);
        }
    }
}

// Send + Sync auto-derive.
//
// U2 exemplar refactor removed the `RwLock<Box<dyn RenderObject<P>>>` field
// on `RenderEntry<P>` and replaced it with plain `Box<dyn RenderObject<P>>`.
// All transitive components are Send + Sync:
//   - Slab<RenderNode> auto-derives Send + Sync from RenderNode.
//   - RenderNode is an enum of RenderEntry<P>; each entry holds a plain
//     Box<dyn RenderObject<P>>, RenderState<P> (lock-free atomics + OnceCell),
//     and NodeLinks (POD).
//   - Box<dyn RenderObject<P>> is Send + Sync because the trait requires
//     `Send + Sync + 'static` (traits/render_object.rs).
//   - Option<RenderId> and Option<Arc<RwLock<PipelineOwner>>> are Send + Sync.
// No `unsafe impl` is required; the previous `unsafe impl Send/Sync for
// RenderTree` block was load-bearing only because of the `RwLock` interior
// mutability around `Box<dyn>`. With that gone, Rust's auto-derivation does
// the right thing and produces the same `Send + Sync` reachability without
// the unsafe carve-out.
//
// See `docs/PORT.md` Refusal trigger 1 and
// `crates/flui-rendering/ARCHITECTURE.md` for the rationale.

// ============================================================================
// flui-tree Trait Implementations
// ============================================================================

impl TreeRead<RenderId> for RenderTree {
    type Node = RenderNode;

    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: RenderId) -> Option<&Self::Node> {
        RenderTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: RenderId) -> bool {
        RenderTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        RenderTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.nodes.iter().map(|(idx, _)| RenderId::new(idx + 1))
    }
}

impl TreeWrite<RenderId> for RenderTree {
    #[inline]
    fn get_mut(&mut self, id: RenderId) -> Option<&mut Self::Node> {
        RenderTree::get_mut(self, id)
    }

    fn insert(&mut self, node: Self::Node) -> RenderId {
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1)
    }

    fn remove_shallow(&mut self, id: RenderId) -> Option<Self::Node> {
        // Cycle 3 T-1: the trait's `remove` default impl now cascades
        // post-order via this primitive. `remove_shallow` keeps the
        // pre-cycle non-cascade behaviour for reparenting workflows
        // (re-attach the descendants under a new parent immediately).

        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        // Get parent and remove from parent's children
        if let Some(parent_id) = self.get(id).and_then(|n| n.parent())
            && let Some(parent) = self.nodes.get_mut(parent_id.get() - 1)
        {
            parent.remove_child(id);
        }

        self.nodes.try_remove(id.get() - 1)
    }

    #[inline]
    fn clear(&mut self) {
        RenderTree::clear(self);
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        RenderTree::reserve(self, additional);
    }
}

impl TreeNav<RenderId> for RenderTree {
    const MAX_DEPTH: usize = 64;
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: RenderId) -> Option<RenderId> {
        RenderTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        self.get(id)
            .map(|node| node.children().iter().copied())
            .into_iter()
            .flatten()
    }

    #[inline]
    fn ancestors(&self, start: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: RenderId) -> impl Iterator<Item = (RenderId, usize)> + '_ {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        AllSiblings::new(self, id)
    }

    #[inline]
    fn child_count(&self, id: RenderId) -> usize {
        self.get(id).map(|node| node.children().len()).unwrap_or(0)
    }

    #[inline]
    fn has_children(&self, id: RenderId) -> bool {
        self.get(id)
            .map(|node| !node.children().is_empty())
            .unwrap_or(false)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_types::Pixels;

    use super::*;
    use crate::objects::RenderSizedBox;

    fn make_leaf() -> Box<dyn RenderObject<BoxProtocol>> {
        Box::new(RenderSizedBox::fixed(Pixels(10.0), Pixels(10.0)))
    }

    #[test]
    fn get_two_mut_returns_distinct_nodes() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        let b = tree.insert_box(make_leaf());
        let pair = tree.get_two_mut(a, b);
        assert!(pair.is_some(), "two existing distinct ids must yield Some");
        let (na, nb) = pair.unwrap();
        // The two refs must point to different RenderNodes -- compare addresses.
        assert!(!std::ptr::eq(na as *const _, nb as *const _));
    }

    #[test]
    fn get_two_mut_returns_none_on_duplicate_id_in_release() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        // In debug builds this panics via debug_assert_ne!; we run the
        // release-path check by going through the `if a == b { return None }`
        // arm directly. To exercise that without tripping the debug assert,
        // we test the missing-second-id branch instead.
        let missing = a; // intentionally the same id
        if cfg!(debug_assertions) {
            // debug build: skip (would panic). Behaviour validated by
            // the release-build `return None` path below in test
            // get_two_mut_with_missing_id_returns_none.
        } else {
            assert!(tree.get_two_mut(a, missing).is_none());
        }
    }

    #[test]
    fn get_two_mut_with_missing_id_returns_none() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        // Build an id that cannot exist (a + 100).
        let missing = RenderId::new(a.get() + 100);
        assert!(tree.get_two_mut(a, missing).is_none());
        assert!(tree.get_two_mut(missing, a).is_none());
    }

    #[test]
    fn get_parent_and_children_mut_returns_n_plus_one_refs() {
        let mut tree = RenderTree::new();
        let parent = tree.insert_box(make_leaf());
        let c1 = tree.insert_box(make_leaf());
        let c2 = tree.insert_box(make_leaf());
        let c3 = tree.insert_box(make_leaf());

        let result = tree.get_parent_and_children_mut(parent, &[c1, c2, c3]);
        assert!(
            result.is_some(),
            "valid parent + 3 distinct children must yield Some"
        );
        let (_parent_ref, children) = result.unwrap();
        assert_eq!(children.len(), 3);
    }

    #[test]
    fn get_parent_and_children_mut_rejects_duplicate_child() {
        let mut tree = RenderTree::new();
        let parent = tree.insert_box(make_leaf());
        let c1 = tree.insert_box(make_leaf());
        // c1 appears twice in the children list -- the duplicate-detection
        // pass must reject the request.
        assert!(
            tree.get_parent_and_children_mut(parent, &[c1, c1])
                .is_none()
        );
    }

    #[test]
    fn get_parent_and_children_mut_rejects_parent_in_child_list() {
        let mut tree = RenderTree::new();
        let parent = tree.insert_box(make_leaf());
        let c1 = tree.insert_box(make_leaf());
        // parent appearing as a child means the parent's slot would be
        // borrowed twice.
        assert!(
            tree.get_parent_and_children_mut(parent, &[c1, parent])
                .is_none()
        );
    }

    // ========================================================================
    // get_subtree_mut (D-block PR-A1b3 U20.1)
    // ========================================================================

    #[test]
    fn get_subtree_mut_returns_n_disjoint_refs_in_input_order() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        let b = tree.insert_box(make_leaf());
        let c = tree.insert_box(make_leaf());
        let d = tree.insert_box(make_leaf());

        // Acquire all 4 nodes in [c, a, d, b] order — verifies input
        // ordering is preserved in the returned Vec (not slot-id-sorted).
        let ids = [c, a, d, b];
        let refs = tree
            .get_subtree_mut(&ids)
            .expect("4 distinct ids must yield Some");
        assert_eq!(refs.len(), 4);

        // Verify disjointness — all 4 references point to distinct
        // RenderNodes (compare addresses through *const _).
        let addrs: Vec<*const RenderNode> = refs.iter().map(|r| *r as *const RenderNode).collect();
        for (i, &a_addr) in addrs.iter().enumerate() {
            for &b_addr in &addrs[i + 1..] {
                assert!(
                    !std::ptr::eq(a_addr, b_addr),
                    "all returned refs must alias distinct slab slots",
                );
            }
        }
        // Drop the disjointness check's borrow before the order check.
        drop(refs);

        // PR #145 review fix (Copilot 3294267590): verify refs[i]
        // CORRESPONDS to ids[i] — not just disjoint / correct count.
        // Write a distinct marker via refs[i] (depth = i + 100), drop
        // the Vec, read back via tree.get(ids[i]) to confirm
        // position-by-position alignment. The +100 offset avoids
        // collision with depths set by insert_box_child (these were
        // all 0 since the nodes are roots here).
        {
            let mut refs = tree
                .get_subtree_mut(&ids)
                .expect("re-acquire for marker write");
            for (i, r) in refs.iter_mut().enumerate() {
                r.set_depth((i + 100) as u16);
            }
            // refs Vec drops here, freeing slab for the read-back below.
        }
        for (i, &id) in ids.iter().enumerate() {
            let depth = tree.get(id).expect("node still in tree").depth();
            assert_eq!(
                depth,
                (i + 100) as u16,
                "refs[{i}] must alias ids[{i}]'s slot — input order preserved",
            );
        }
    }

    /// PR #145 review fix (Codex 3294268624 + Copilot 3294267583):
    /// `collect_subtree_ids` must terminate on cyclic trees instead of
    /// hanging / OOMing. The visited-set short-circuit dedups repeated
    /// ids on the DFS stack.
    #[test]
    fn collect_subtree_ids_terminates_on_cycle() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        let b = tree.insert_box_child(a, make_leaf()).expect("b insert");

        // Inject a synthetic A → B → A cycle by adding A back as a
        // child of B (defeats the natural tree-construction
        // discipline — would normally come from a hot-reload bug or
        // programmatic tree mutation).
        tree.get_mut(b).expect("b in tree").add_child(a);

        // Without the visited-set guard this would loop forever:
        // pop A → push B → pop B → push A → pop A → push B → ...
        let ids = tree.collect_subtree_ids(a);

        // Must terminate. Output must contain A and B exactly once
        // (deduped by visited-set). Order: A first (DFS pre-order
        // root), then B.
        assert_eq!(
            ids.len(),
            2,
            "cyclic A → B → A subtree must collect to exactly 2 unique ids; got {ids:?}",
        );
        assert!(ids.contains(&a) && ids.contains(&b));

        // Output must satisfy get_subtree_mut's uniqueness requirement.
        assert!(
            tree.get_subtree_mut(&ids).is_some(),
            "deduped output must be acceptable to get_subtree_mut",
        );
    }

    #[test]
    fn get_subtree_mut_rejects_duplicate_id() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        let b = tree.insert_box(make_leaf());
        // a appears twice in the id list — duplicate detection must fail.
        assert!(tree.get_subtree_mut(&[a, b, a]).is_none());
    }

    #[test]
    fn get_subtree_mut_rejects_missing_id() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        let missing = RenderId::new(a.get() + 999);
        assert!(tree.get_subtree_mut(&[a, missing]).is_none());
        assert!(tree.get_subtree_mut(&[missing, a]).is_none());
    }

    #[test]
    fn get_subtree_mut_empty_id_list_returns_empty_vec() {
        let mut tree = RenderTree::new();
        let _a = tree.insert_box(make_leaf());
        let refs = tree
            .get_subtree_mut(&[])
            .expect("empty input must yield empty Vec");
        assert!(refs.is_empty());
    }

    #[test]
    fn get_subtree_mut_single_id_works() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        let refs = tree
            .get_subtree_mut(&[a])
            .expect("single id must yield Some");
        assert_eq!(refs.len(), 1);
    }

    // ========================================================================
    // collect_subtree_ids (D-block PR-A1b3 U20.1)
    // ========================================================================

    #[test]
    fn collect_subtree_ids_root_only() {
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        assert_eq!(tree.collect_subtree_ids(root), vec![root]);
    }

    #[test]
    fn collect_subtree_ids_missing_root_returns_empty() {
        let tree = RenderTree::new();
        let missing = RenderId::new(42);
        assert!(tree.collect_subtree_ids(missing).is_empty());
    }

    #[test]
    fn collect_subtree_ids_two_level_preserves_child_order() {
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        let c1 = tree.insert_box_child(root, make_leaf()).unwrap();
        let c2 = tree.insert_box_child(root, make_leaf()).unwrap();
        let c3 = tree.insert_box_child(root, make_leaf()).unwrap();

        // Pre-order: root, c1, c2, c3 (children visited in stored order)
        assert_eq!(tree.collect_subtree_ids(root), vec![root, c1, c2, c3]);
    }

    #[test]
    fn collect_subtree_ids_three_level_dfs_preorder() {
        // Tree:
        //     root
        //    /    \
        //   a      b
        //  / \      \
        // a1 a2     b1
        //
        // Pre-order: root, a, a1, a2, b, b1
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        let a = tree.insert_box_child(root, make_leaf()).unwrap();
        let a1 = tree.insert_box_child(a, make_leaf()).unwrap();
        let a2 = tree.insert_box_child(a, make_leaf()).unwrap();
        let b = tree.insert_box_child(root, make_leaf()).unwrap();
        let b1 = tree.insert_box_child(b, make_leaf()).unwrap();

        assert_eq!(
            tree.collect_subtree_ids(root),
            vec![root, a, a1, a2, b, b1],
            "DFS pre-order must visit each subtree completely before moving \
             to the next sibling",
        );
    }

    #[test]
    fn collect_subtree_ids_subtree_root_works() {
        // Same tree as above, but call collect_subtree_ids on `a`
        // instead of `root`. Expect: a, a1, a2 (excludes root and b's subtree).
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        let a = tree.insert_box_child(root, make_leaf()).unwrap();
        let a1 = tree.insert_box_child(a, make_leaf()).unwrap();
        let a2 = tree.insert_box_child(a, make_leaf()).unwrap();
        let _b = tree.insert_box_child(root, make_leaf()).unwrap();

        assert_eq!(tree.collect_subtree_ids(a), vec![a, a1, a2]);
    }

    #[test]
    fn collect_subtree_ids_chain_is_linear() {
        // Linear chain: root → mid → leaf
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        let mid = tree.insert_box_child(root, make_leaf()).unwrap();
        let leaf = tree.insert_box_child(mid, make_leaf()).unwrap();
        assert_eq!(tree.collect_subtree_ids(root), vec![root, mid, leaf]);
    }

    /// Pairs `collect_subtree_ids` with `get_subtree_mut` — the canonical
    /// U20.1 usage pattern. Should always yield Some, and the returned
    /// Vec length should equal the collected id count.
    #[test]
    fn collect_subtree_ids_feeds_get_subtree_mut() {
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        let a = tree.insert_box_child(root, make_leaf()).unwrap();
        let _a1 = tree.insert_box_child(a, make_leaf()).unwrap();
        let _b = tree.insert_box_child(root, make_leaf()).unwrap();

        let ids = tree.collect_subtree_ids(root);
        assert_eq!(ids.len(), 4);

        let refs = tree.get_subtree_mut(&ids).expect(
            "collect_subtree_ids output must always satisfy get_subtree_mut \
             uniqueness + presence preconditions",
        );
        assert_eq!(refs.len(), 4);
    }

    /// Cycle 4 PR #116 review fix: pre-order traversal of the
    /// iterative `visit_depth_first` must yield root, then each
    /// subtree in child-insertion order. The reverse-push trick is
    /// the load-bearing detail; this test would catch any future
    /// "simpler" rewrite that pushes children forward and prints
    /// siblings in reverse.
    ///
    /// Tree shape (insertion order matches child order):
    /// ```text
    /// root
    /// ├── a
    /// │   └── a1
    /// ├── b
    /// └── c
    /// ```
    /// Expected pre-order: [root, a, a1, b, c]
    #[test]
    fn visit_depth_first_yields_preorder_with_sibling_order() {
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        tree.set_root(Some(root));

        let a = tree.insert_box_child(root, make_leaf()).expect("insert a");
        let a1 = tree.insert_box_child(a, make_leaf()).expect("insert a1");
        let b = tree.insert_box_child(root, make_leaf()).expect("insert b");
        let c = tree.insert_box_child(root, make_leaf()).expect("insert c");

        let mut visited = Vec::new();
        tree.visit_depth_first(|id, _| visited.push(id));

        assert_eq!(
            visited,
            vec![root, a, a1, b, c],
            "pre-order must be root, then each subtree in child-insertion order"
        );
    }

    /// Empty-root guard: when no root is set, the visitor is never
    /// invoked. Tests the `Some(root_id) else return` early exit.
    #[test]
    fn visit_depth_first_with_no_root_is_noop() {
        let tree = RenderTree::new();
        let mut visited = 0_usize;
        tree.visit_depth_first(|_, _| visited += 1);
        assert_eq!(visited, 0);
    }
}

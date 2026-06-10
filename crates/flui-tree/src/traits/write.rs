//! Mutable tree operations trait.
//!
//! This module provides the [`TreeWrite`] trait for modifying
//! tree structure (insert, remove, reparent).

use flui_foundation::Identifier;

use super::TreeRead;
use crate::depth::INLINE_TREE_DEPTH;
use crate::error::{TreeError, TreeResult};

/// Mutable access to tree nodes and structure.
///
/// This trait extends [`TreeRead`] with operations that modify
/// the tree structure. It provides both low-level node access
/// and higher-level tree operations.
///
/// # Generic Parameter
///
/// The `I` parameter specifies the ID type used for node identification,
/// matching the same type used in [`TreeRead<I>`].
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync`. Mutable operations
/// require exclusive access (`&mut self`).
///
/// # Error Handling
///
/// Operations that can fail return [`TreeResult`]. Common errors:
/// - `NotFound` - Node doesn't exist
/// - `CycleDetected` - Reparenting would create a cycle
/// - `AlreadyExists` - Node already in tree
pub trait TreeWrite<I: Identifier>: TreeRead<I> {
    /// Returns a mutable reference to the node with the given ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the node
    ///
    /// # Returns
    ///
    /// `Some(&mut Node)` if the node exists, `None` otherwise.
    fn get_mut(&mut self, id: I) -> Option<&mut Self::Node>;

    /// Inserts a new node into the tree.
    ///
    /// The node is inserted without a parent (as a potential root).
    /// Use [`TreeWriteNav::set_parent`] to establish parent-child
    /// relationships.
    ///
    /// # Arguments
    ///
    /// * `node` - The node to insert
    ///
    /// # Returns
    ///
    /// The unique ID assigned to the new node.
    ///
    /// # Performance
    ///
    /// Should be O(1) amortized for slab-based implementations.
    fn insert(&mut self, node: Self::Node) -> I;

    /// Removes a node **without** cascading to descendants.
    ///
    /// This is the primitive that [`remove`](Self::remove) (cascade-by-
    /// default) builds on. Children are orphaned in storage; the caller
    /// is responsible for re-attaching or otherwise dealing with them.
    ///
    /// Use this directly for re-parenting workflows where the subtree
    /// is immediately re-attached to a new parent. For one-shot removal
    /// (the common case), call [`remove`](Self::remove) instead.
    ///
    /// Implementations MUST:
    /// 1. Unlink the node from its parent's children list (if any).
    /// 2. Update root tracking if `id` is the root.
    /// 3. Remove the node from storage and return it.
    /// 4. Leave descendants (and their parent pointers to `id`) intact.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the node to remove.
    ///
    /// # Returns
    ///
    /// `Some(node)` if the node existed and was removed, `None`
    /// otherwise.
    fn remove_shallow(&mut self, id: I) -> Option<Self::Node>;

    /// Removes a node **and all its descendants** (cascade-by-default).
    ///
    /// Cycle 2 PR #100 hoisted this contract to `LayerTree::remove` and
    /// `SemanticsTree::remove` per-impl. Cycle 3 T-1 lifts it to the
    /// trait so every adopter (current `RenderTree`, future
    /// `ElementTree`, `ViewTree`, and the two cycle-2 adopters) inherits
    /// the cascade as the default contract. Pre-cycle the trait
    /// codified non-cascade as the default — i.e. orphans-in-storage —
    /// which the audit T-1 finding flagged as a footgun.
    ///
    /// This default delegates to [`try_remove`](Self::try_remove),
    /// which walks the subtree post-order **iteratively** with
    /// cycle-detection. On a corrupted (cyclic) slab `try_remove`
    /// returns `Err(TreeError::CycleDetected)`; `remove` maps that to
    /// `None` + a `tracing::warn!` (callers needing to distinguish a
    /// cycle from "not found" should call `try_remove` directly).
    ///
    /// The post-order drain guarantees each `remove_shallow` call sees
    /// children disposing before their parents — the engine listeners
    /// and lifecycle hooks (`LayerNode::Drop` from PR #100 U8) rely on
    /// it.
    ///
    /// PR #103 followup: the original draft of this default did a
    /// recursive `self.remove(child_id)` call per child, which
    /// consumed one stack frame per tree depth level and risked a
    /// stack overflow on tall trees (large generated view trees or
    /// nested scroll views can exceed the default thread stack).
    /// The iterative shape uses heap allocation for the worklist and
    /// keeps stack usage constant regardless of depth.
    ///
    /// Implementations MAY override for efficiency (e.g. a `Vec`-backed
    /// arena that wants to free a contiguous range in one go), but
    /// MUST preserve the post-order cascade semantics.
    ///
    /// # Arguments
    ///
    /// * `id` - The root of the subtree to remove.
    ///
    /// # Returns
    ///
    /// `Some(root_node)` if `id` existed (the removed root node) or
    /// `None` if it did not.
    fn remove(&mut self, id: I) -> Option<Self::Node>
    where
        Self: super::TreeNav<I> + Sized,
    {
        match self.try_remove(id) {
            Ok(node) => node,
            Err(e @ TreeError::CycleDetected(_)) => {
                tracing::warn!(
                    error = ?e,
                    "TreeWrite::remove encountered a cycle; returning None. \
                     Use try_remove() to handle this case explicitly."
                );
                None
            }
            Err(e) => {
                tracing::error!(error = ?e, "TreeWrite::remove encountered an unexpected error");
                None
            }
        }
    }

    /// Removes a node **and all its descendants** with cycle-detection.
    ///
    /// This is the semantic-carrying sibling of [`remove`](Self::remove):
    /// it returns `Err(TreeError::CycleDetected)` if a corrupted cycle is
    /// found during the cascade walk rather than hanging or OOM-ing on an
    /// infinite traversal. [`remove`](Self::remove) delegates here and
    /// maps the `Err` to `None` + `tracing::warn!`.
    ///
    /// Callers that need to distinguish a genuine "node not found"
    /// (`Ok(None)`) from a cycle error (`Err(CycleDetected)`) should call
    /// `try_remove` directly.
    ///
    /// # Cycle detection
    ///
    /// Uses a `HashSet<I>` visited set for O(1) per-node detection
    /// (O(N) space). The `I: Hash + Eq` bound is already supplied by the
    /// [`Identifier`] supertrait, so no extra bound is required here.
    /// Under the normal public API (`add_child`, `set_parent`) cycle
    /// creation is rejected at insertion time; this guard is
    /// defense-in-depth against a corrupted slab.
    ///
    /// # Worklist allocation
    ///
    /// Both the to-visit stack and the collected worklist use
    /// `SmallVec<[I; INLINE_TREE_DEPTH]>` (inline = 32 entries) to avoid
    /// heap allocation for typical shallow subtrees; deeper subtrees
    /// spill to the heap (closes audit F24, replacing the prior
    /// heap-allocated vector worklist).
    ///
    /// # Post-order drain
    ///
    /// The worklist is drained in reverse so each `remove_shallow` call
    /// sees children disposing before their parents — the engine
    /// listeners and lifecycle hooks rely on this post-order guarantee.
    ///
    /// # Errors
    ///
    /// Returns [`TreeError::CycleDetected`] if a node is visited twice
    /// during the pre-walk (i.e. the slab contains a cycle).
    fn try_remove(&mut self, id: I) -> Result<Option<Self::Node>, TreeError>
    where
        Self: super::TreeNav<I> + Sized,
    {
        use std::collections::HashSet;

        use smallvec::SmallVec;

        if !self.contains(id) {
            return Ok(None);
        }

        let mut worklist: SmallVec<[I; INLINE_TREE_DEPTH]> = SmallVec::new();
        let mut to_visit: SmallVec<[I; INLINE_TREE_DEPTH]> = SmallVec::new();
        let mut visited: HashSet<I> = HashSet::new();

        to_visit.push(id);
        while let Some(current) = to_visit.pop() {
            if !visited.insert(current) {
                // `current` was already visited: the slab contains a
                // cycle. Abort rather than loop forever.
                tracing::warn!(
                    node_id = current.get(),
                    "cycle detected in cascade removal; aborting traversal"
                );
                return Err(TreeError::cycle_detected(current.get()));
            }
            worklist.push(current);
            // Push children for later processing. Since this is a
            // pre-walk feeding a post-order drain (reversed before
            // disposal), child order doesn't matter — what matters is
            // that every descendant appears *after* its parent in
            // `worklist`, which the depth-first push guarantees.
            for child_id in self.children(current) {
                to_visit.push(child_id);
            }
        }

        // Post-order drain: reverse so leaves dispose before their
        // parents, then the root last. Capture the root node from its
        // `remove_shallow` to return as the trait's contract.
        let mut root_node: Option<Self::Node> = None;
        for node_id in worklist.into_iter().rev() {
            let removed = self.remove_shallow(node_id);
            if node_id == id {
                root_node = removed;
            }
        }
        Ok(root_node)
    }

    /// Removes `id` and counts the resulting cascade size.
    ///
    /// Convenience wrapper around [`remove`](Self::remove) for callers
    /// that want an explicit count (e.g. devtools "X nodes removed"
    /// messaging).
    ///
    /// # Default Implementation
    ///
    /// Walks `descendants(id)` to compute the count (the iterator yields
    /// `id` itself plus all transitive descendants), then calls
    /// [`remove`](Self::remove). Implementations with a denser storage
    /// representation may override.
    fn remove_subtree(&mut self, id: I) -> usize
    where
        Self: super::TreeNav<I> + Sized,
    {
        // Pre-walk to capture the count without holding the descendant
        // iterator across the mutable cascade. `descendants(id)` is
        // inclusive — it yields `id` itself.
        let count = self.descendants(id).count();
        if count == 0 {
            return 0;
        }
        let _ = self.remove(id);
        count
    }

    /// Clears all nodes from the tree.
    ///
    /// After this operation, `len()` returns 0.
    ///
    /// # Default Implementation
    ///
    /// Does nothing - implementations must override this method.
    /// The default cannot be implemented generically due to borrow
    /// checker constraints.
    fn clear(&mut self) {
        // Default implementation does nothing.
        // Implementations should override with efficient clearing.
    }

    /// Reserves capacity for at least `additional` more nodes.
    ///
    /// # Default Implementation
    ///
    /// Does nothing. Implementations with pre-allocated storage
    /// should override.
    #[inline]
    fn reserve(&mut self, additional: usize) {
        let _ = additional;
    }
}

/// Extended tree write operations requiring navigation.
///
/// This trait provides operations that need both write access and
/// navigation capabilities.
pub trait TreeWriteNav<I: Identifier>: TreeWrite<I> + super::TreeNav<I> {
    /// Sets the parent of a node.
    ///
    /// # Arguments
    ///
    /// * `child` - The node to reparent
    /// * `new_parent` - The new parent, or `None` to make it a root
    ///
    /// # Errors
    ///
    /// - `NotFound` - Child or parent doesn't exist
    /// - `CycleDetected` - New parent is a descendant of child
    ///
    /// # Note
    ///
    /// This method has no default implementation because it requires
    /// access to the internal node structure. Implementations must
    /// provide their own version that:
    /// 1. Validates child and `new_parent` exist
    /// 2. Checks for cycles (`new_parent` must not be a descendant of child)
    /// 3. Updates old parent's children list
    /// 4. Updates new parent's children list
    /// 5. Updates child's parent reference
    fn set_parent(&mut self, child: I, new_parent: Option<I>) -> TreeResult<I>;

    /// Adds a child to a parent node.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent node
    /// * `child` - The child to add
    ///
    /// # Errors
    ///
    /// - `NotFound` - Parent or child doesn't exist
    /// - `CycleDetected` - Child is an ancestor of parent
    #[inline]
    fn add_child(&mut self, parent: I, child: I) -> TreeResult<I> {
        self.set_parent(child, Some(parent))
    }

    /// Removes a child from its parent.
    ///
    /// The child becomes a root node (no parent).
    ///
    /// # Arguments
    ///
    /// * `child` - The child to detach
    ///
    /// # Errors
    ///
    /// - `NotFound` - Child doesn't exist
    #[inline]
    fn detach(&mut self, child: I) -> TreeResult<I> {
        self.set_parent(child, None)
    }

    /// Moves all children from one parent to another.
    ///
    /// # Arguments
    ///
    /// * `from` - The source parent
    /// * `to` - The destination parent
    ///
    /// # Errors
    ///
    /// - `NotFound` - Source or destination doesn't exist
    /// - `CycleDetected` - Would create a cycle
    fn move_children(&mut self, from: I, to: I) -> TreeResult<()>
    where
        Self: Sized,
    {
        if !self.contains(from) {
            return Err(TreeError::not_found(from.get()));
        }
        if !self.contains(to) {
            return Err(TreeError::not_found(to.get()));
        }

        // Check for cycles: 'to' can't be a descendant of 'from'
        if self.is_ancestor_of(from, to) {
            return Err(TreeError::cycle_detected(to.get()));
        }

        // Collect children first (to avoid borrow issues)
        let children: Vec<_> = self.children(from).collect();

        // Move each child
        for child in children {
            self.set_parent(child, Some(to))?;
        }

        Ok(())
    }

    /// Inserts a node as a child of the given parent.
    ///
    /// Convenience method combining `insert` and `set_parent`.
    ///
    /// # Arguments
    ///
    /// * `node` - The node to insert
    /// * `parent` - The parent node, or `None` for root
    ///
    /// # Returns
    ///
    /// The ID of the newly inserted node.
    ///
    /// # Errors
    ///
    /// - `NotFound` - Parent doesn't exist
    fn insert_child(&mut self, node: Self::Node, parent: Option<I>) -> TreeResult<I> {
        let id = self.insert(node);

        if let Some(parent_id) = parent {
            if !self.contains(parent_id) {
                // Rollback the just-inserted node. `remove_shallow` is
                // safe here because the node is freshly inserted and
                // has no children yet — cascade vs shallow is moot.
                self.remove_shallow(id);
                return Err(TreeError::not_found(parent_id.get()));
            }
            self.set_parent(id, Some(parent_id))?;
        }

        Ok(id)
    }
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

impl<I: Identifier, T: TreeWrite<I> + ?Sized> TreeWrite<I> for &mut T {
    #[inline]
    fn get_mut(&mut self, id: I) -> Option<&mut Self::Node> {
        (**self).get_mut(id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> I {
        (**self).insert(node)
    }

    #[inline]
    fn remove_shallow(&mut self, id: I) -> Option<Self::Node> {
        (**self).remove_shallow(id)
    }

    #[inline]
    fn clear(&mut self) {
        (**self).clear();
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        (**self).reserve(additional);
    }
}

impl<I: Identifier, T: TreeWrite<I> + ?Sized> TreeWrite<I> for Box<T> {
    #[inline]
    fn get_mut(&mut self, id: I) -> Option<&mut Self::Node> {
        (**self).get_mut(id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> I {
        (**self).insert(node)
    }

    #[inline]
    fn remove_shallow(&mut self, id: I) -> Option<Self::Node> {
        (**self).remove_shallow(id)
    }

    #[inline]
    fn clear(&mut self) {
        (**self).clear();
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        (**self).reserve(additional);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_foundation::ViewId;

    use super::*;
    use crate::{
        iter::{Ancestors, DescendantsWithDepth},
        traits::{TreeNav, TreeRead},
    };

    // Test implementation
    #[derive(Debug, Default)]
    struct TestNode {
        value: i32,
        parent: Option<ViewId>,
        children: Vec<ViewId>,
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        /// Test-only escape hatch: injects a child edge into the slab
        /// **without** the public-API cycle check in `set_parent`.
        ///
        /// Used by `cascade_cycle_detection` to corrupt the tree into a
        /// cyclic state that the cascade walk must defend against. Sets
        /// the child's parent pointer and appends it to the parent's
        /// child list directly.
        fn corrupt_add_child(&mut self, parent: ViewId, child: ViewId) {
            if let Some(Some(parent_node)) = self.nodes.get_mut(parent.get() - 1)
                && !parent_node.children.contains(&child)
            {
                parent_node.children.push(child);
            }
            if let Some(Some(child_node)) = self.nodes.get_mut(child.get() - 1) {
                child_node.parent = Some(parent);
            }
        }
    }

    impl TreeRead<ViewId> for TestTree {
        type Node = TestNode;

        fn get(&self, id: ViewId) -> Option<&TestNode> {
            self.nodes.get(id.get() - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }

        fn node_ids(&self) -> impl Iterator<Item = ViewId> + '_ {
            (0..self.nodes.len()).filter_map(|i| {
                if self.nodes[i].is_some() {
                    Some(ViewId::new(i + 1))
                } else {
                    None
                }
            })
        }
    }

    impl TreeNav<ViewId> for TestTree {
        fn parent(&self, id: ViewId) -> Option<ViewId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ViewId) -> impl Iterator<Item = ViewId> + '_ {
            self.get(id)
                .map(|node| node.children.iter().copied())
                .into_iter()
                .flatten()
        }

        fn ancestors(&self, start: ViewId) -> impl Iterator<Item = ViewId> + '_ {
            Ancestors::new(self, start)
        }

        fn descendants(&self, root: ViewId) -> impl Iterator<Item = (ViewId, usize)> + '_ {
            DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ViewId) -> impl Iterator<Item = ViewId> + '_ {
            let parent_id = self.parent(id);
            parent_id
                .into_iter()
                .flat_map(move |pid| self.children(pid).filter(move |&cid| cid != id))
        }
    }

    impl TreeWrite<ViewId> for TestTree {
        fn get_mut(&mut self, id: ViewId) -> Option<&mut TestNode> {
            self.nodes.get_mut(id.get() - 1)?.as_mut()
        }

        fn insert(&mut self, node: TestNode) -> ViewId {
            let id = ViewId::new(self.nodes.len() + 1);
            self.nodes.push(Some(node));
            id
        }

        fn remove_shallow(&mut self, id: ViewId) -> Option<TestNode> {
            let index = id.get() - 1;

            // Remove from parent's children
            if let Some(node) = self.nodes.get(index)?.as_ref()
                && let Some(parent_id) = node.parent
                && let Some(Some(parent)) = self.nodes.get_mut(parent_id.get() - 1)
            {
                parent.children.retain(|&child| child != id);
            }

            self.nodes.get_mut(index)?.take()
        }

        fn clear(&mut self) {
            self.nodes.clear();
        }

        fn reserve(&mut self, additional: usize) {
            self.nodes.reserve(additional);
        }
    }

    impl TreeWriteNav<ViewId> for TestTree {
        fn set_parent(&mut self, child: ViewId, new_parent: Option<ViewId>) -> TreeResult<ViewId> {
            // Check child exists
            if !self.contains(child) {
                return Err(TreeError::not_found(child.get()));
            }

            // Check new parent exists (if provided)
            if let Some(parent_id) = new_parent {
                if !self.contains(parent_id) {
                    return Err(TreeError::not_found(parent_id.get()));
                }

                // Check for cycles: new_parent must not be a descendant of child
                if self.is_ancestor_of(child, parent_id) || parent_id == child {
                    return Err(TreeError::cycle_detected(child.get()));
                }
            }

            // Remove from old parent's children
            if let Some(old_parent) = self.parent(child)
                && let Some(Some(parent_node)) = self.nodes.get_mut(old_parent.get() - 1)
            {
                parent_node.children.retain(|&c| c != child);
            }

            // Update child's parent
            if let Some(Some(child_node)) = self.nodes.get_mut(child.get() - 1) {
                child_node.parent = new_parent;
            }

            // Add to new parent's children
            if let Some(parent_id) = new_parent
                && let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() - 1)
                && !parent_node.children.contains(&child)
            {
                parent_node.children.push(child);
            }

            Ok(child)
        }
    }

    #[test]
    fn test_insert_remove() {
        let mut tree = TestTree::new();

        let id = tree.insert(TestNode {
            value: 42,
            ..Default::default()
        });
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
        assert_eq!(tree.get(id).map(|n| n.value), Some(42));

        let removed = tree.remove(id);
        assert_eq!(removed.map(|n| n.value), Some(42));
        assert_eq!(tree.len(), 0);
        assert!(!tree.contains(id));
    }

    #[test]
    fn test_get_mut() {
        let mut tree = TestTree::new();
        let id = tree.insert(TestNode {
            value: 1,
            ..Default::default()
        });

        tree.get_mut(id).unwrap().value = 99;
        assert_eq!(tree.get(id).map(|n| n.value), Some(99));
    }

    #[test]
    fn test_set_parent() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());

        let _ = tree.set_parent(child, Some(root)).unwrap();

        assert_eq!(tree.parent(child), Some(root));
        let children: Vec<_> = tree.children(root).collect();
        assert_eq!(children, vec![child]);
    }

    #[test]
    fn test_cycle_detection() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());

        let _ = tree.set_parent(child, Some(root)).unwrap();

        // Trying to make root a child of child should fail
        let result = tree.set_parent(root, Some(child));
        assert!(matches!(result, Err(TreeError::CycleDetected(_))));
    }

    #[test]
    fn test_detach() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());

        let _ = tree.set_parent(child, Some(root)).unwrap();
        assert_eq!(tree.parent(child), Some(root));

        let _ = tree.detach(child).unwrap();
        assert_eq!(tree.parent(child), None);
        assert_eq!(tree.children(root).count(), 0);
    }

    #[test]
    fn test_clear() {
        let mut tree = TestTree::new();
        let _ = tree.insert(TestNode::default());
        let _ = tree.insert(TestNode::default());
        let _ = tree.insert(TestNode::default());

        assert_eq!(tree.len(), 3);
        tree.clear();
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_remove_subtree() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child1 = tree.insert(TestNode::default());
        let child2 = tree.insert(TestNode::default());
        let grandchild = tree.insert(TestNode::default());

        let _ = tree.set_parent(child1, Some(root)).unwrap();
        let _ = tree.set_parent(child2, Some(root)).unwrap();
        let _ = tree.set_parent(grandchild, Some(child1)).unwrap();

        assert_eq!(tree.len(), 4);

        // Remove child1 subtree (child1 + grandchild)
        let removed = tree.remove_subtree(child1);
        assert_eq!(removed, 2);
        assert_eq!(tree.len(), 2);
        assert!(!tree.contains(child1));
        assert!(!tree.contains(grandchild));
        assert!(tree.contains(root));
        assert!(tree.contains(child2));
    }

    /// Cycle 3 T-1 regression: `TreeWrite::remove` cascades by default.
    ///
    /// Pre-cycle the trait's `remove` was non-cascade — descendants were
    /// orphaned in storage. Cycle 2 PR #100 fixed this at the impl
    /// level for `LayerTree` and `SemanticsTree`; cycle 3 lifts the
    /// fix to the trait contract so every adopter inherits it.
    #[test]
    fn remove_cascades_by_default() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());
        let grandchild = tree.insert(TestNode::default());
        tree.set_parent(child, Some(root)).unwrap();
        tree.set_parent(grandchild, Some(child)).unwrap();
        assert_eq!(tree.len(), 3);

        // `remove(root)` MUST drop the whole subtree.
        let removed = tree.remove(root);
        assert!(removed.is_some(), "root must have been removed");
        assert_eq!(tree.len(), 0, "every descendant must cascade away");
        assert!(!tree.contains(root));
        assert!(!tree.contains(child));
        assert!(!tree.contains(grandchild));
    }

    /// Cycle 3 T-1 regression: `TreeWrite::remove_shallow` preserves
    /// the pre-cycle non-cascade behaviour. Use for re-parenting
    /// workflows that immediately re-attach the descendants.
    #[test]
    fn remove_shallow_does_not_cascade() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());
        tree.set_parent(child, Some(root)).unwrap();
        assert_eq!(tree.len(), 2);

        let removed = tree.remove_shallow(root);
        assert!(removed.is_some());
        // `child` stays in storage — orphaned but reachable.
        assert!(tree.contains(child));
        assert_eq!(tree.len(), 1);
    }

    /// `remove` of a missing id is a `None` no-op (no panic, no
    /// half-walk).
    #[test]
    fn remove_of_missing_id_is_a_no_op() {
        let mut tree = TestTree::new();
        let _real = tree.insert(TestNode::default());
        let phantom = ViewId::new(999);
        assert!(tree.remove(phantom).is_none());
        assert_eq!(tree.len(), 1);
    }

    /// PR #103 followup (Codex P2): the default cascade is iterative,
    /// not recursive. A linear chain of 10,000 nodes must `remove`
    /// without exhausting the native call stack. Pre-fix the recursive
    /// `self.remove(child_id)` shape consumed one stack frame per
    /// depth level and would stack-overflow on chains longer than
    /// roughly 1k nodes depending on platform default stack size.
    /// F19 + F24: the cascade walk must detect a corrupted cycle and
    /// return `Err(TreeError::CycleDetected)` instead of hanging or
    /// OOM-ing on an infinite traversal. `remove()` must degrade to
    /// `None` (with a `tracing::warn!`) on the same corruption.
    #[test]
    fn cascade_cycle_detection() {
        let mut tree = TestTree::new();
        let a = tree.insert(TestNode::default());
        let b = tree.insert(TestNode::default());
        let c = tree.insert(TestNode::default());
        tree.add_child(a, b).unwrap();
        tree.add_child(b, c).unwrap();

        // Inject cycle: c's child list points back to a (bypasses the
        // public-API cycle check in `set_parent`).
        tree.corrupt_add_child(c, a);

        // `try_remove` must detect the cycle and return Err, not hang/OOM.
        let result = tree.try_remove(a);
        assert!(
            matches!(result, Err(TreeError::CycleDetected(_))),
            "try_remove must detect the cycle and return Err, got {result:?}"
        );

        // `remove()` must return None with tracing::warn! (not panic/hang).
        tree.corrupt_add_child(c, a); // re-inject cycle
        let none_result = tree.remove(a);
        assert!(
            none_result.is_none(),
            "remove() must return None on cycle, not panic"
        );
    }

    /// F19 triangulation: normal subtree removal returns `Ok(Some(root))`.
    #[test]
    fn remove_subtree_no_cycle() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());
        let grandchild = tree.insert(TestNode::default());
        tree.add_child(root, child).unwrap();
        tree.add_child(child, grandchild).unwrap();

        let result = tree.try_remove(root);
        assert!(matches!(result, Ok(Some(_))));
        assert_eq!(tree.len(), 0);
    }

    /// F19 triangulation: leaf removal walks no children.
    #[test]
    fn remove_leaf_node() {
        let mut tree = TestTree::new();
        let leaf = tree.insert(TestNode {
            value: 7,
            ..Default::default()
        });

        let result = tree.try_remove(leaf);
        assert!(matches!(result, Ok(Some(node)) if node.value == 7));
        assert_eq!(tree.len(), 0);
    }

    /// F19 triangulation: removing a missing node is `Ok(None)`.
    #[test]
    fn remove_nonexistent_node() {
        let mut tree = TestTree::new();
        let _real = tree.insert(TestNode::default());
        let phantom = ViewId::new(999);
        assert!(matches!(tree.try_remove(phantom), Ok(None)));
        assert_eq!(tree.len(), 1);
    }

    /// F19 triangulation: `remove_shallow` is unaffected by `try_remove`.
    #[test]
    fn remove_shallow_still_available() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());
        tree.add_child(root, child).unwrap();

        let removed = tree.remove_shallow(root);
        assert!(removed.is_some());
        // child orphaned, still in storage.
        assert!(tree.contains(child));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn remove_cascade_is_stack_safe_on_deep_chain() {
        // 2_000 nodes exceeds typical native-stack frame budgets for
        // recursive descent (Rust's default per-frame size puts the
        // overflow threshold around 1-2k frames depending on
        // platform). The iterative cascade in `TreeWrite::remove`
        // uses heap memory for the worklist instead.
        const DEPTH: usize = 2_000;
        let mut tree = TestTree::new();
        let mut prev = tree.insert(TestNode::default());
        let root = prev;
        for _ in 1..DEPTH {
            let next = tree.insert(TestNode::default());
            tree.set_parent(next, Some(prev)).unwrap();
            prev = next;
        }
        assert_eq!(tree.len(), DEPTH);

        let removed = tree.remove(root);
        assert!(removed.is_some());
        assert_eq!(tree.len(), 0, "{DEPTH}-deep chain must cascade");
    }
}

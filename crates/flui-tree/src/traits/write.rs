//! Mutable tree operations trait.
//!
//! This module provides the [`TreeWrite`] trait for modifying
//! tree structure (insert, remove, reparent).

use flui_foundation::ElementId;

use super::TreeRead;
use crate::error::{TreeError, TreeResult};

/// Mutable access to tree nodes and structure.
///
/// This trait extends [`TreeRead`] with operations that modify
/// the tree structure. It provides both low-level node access
/// and higher-level tree operations.
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
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{TreeWrite, TreeRead};
/// use flui_foundation::ElementId;
///
/// fn add_children<T: TreeWrite>(
///     tree: &mut T,
///     parent: ElementId,
///     count: usize,
/// ) -> Vec<ElementId> {
///     (0..count)
///         .map(|_| {
///             let node = T::Node::default();
///             tree.insert(node)
///         })
///         .collect()
/// }
/// ```
pub trait TreeWrite: TreeRead {
    /// Returns a mutable reference to the node with the given ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the node
    ///
    /// # Returns
    ///
    /// `Some(&mut Node)` if the node exists, `None` otherwise.
    fn get_mut(&mut self, id: ElementId) -> Option<&mut Self::Node>;

    /// Inserts a new node into the tree.
    ///
    /// The node is inserted without a parent (as a potential root).
    /// Use [`Self::set_parent`] to establish parent-child relationships.
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
    fn insert(&mut self, node: Self::Node) -> ElementId;

    /// Removes a node from the tree.
    ///
    /// This removes only the specified node. Children handling
    /// depends on the implementation (may be orphaned or removed).
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the node to remove
    ///
    /// # Returns
    ///
    /// `Some(node)` if the node existed and was removed, `None` otherwise.
    ///
    /// # Note
    ///
    /// Implementations should update parent's children list when
    /// removing a node.
    fn remove(&mut self, id: ElementId) -> Option<Self::Node>;

    /// Removes a node and all its descendants.
    ///
    /// # Arguments
    ///
    /// * `id` - The root of the subtree to remove
    ///
    /// # Returns
    ///
    /// The number of nodes removed.
    ///
    /// # Default Implementation
    ///
    /// Collects descendants and removes them in reverse order.
    /// Implementations may provide more efficient versions.
    fn remove_subtree(&mut self, id: ElementId) -> usize
    where
        Self: super::TreeNav + Sized,
    {
        // Collect all descendants first (to avoid borrow issues)
        let to_remove: Vec<_> = self.descendants(id).collect();
        let count = to_remove.len();

        // Remove in reverse order (children before parents)
        for node_id in to_remove.into_iter().rev() {
            self.remove(node_id);
        }

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
pub trait TreeWriteNav: TreeWrite + super::TreeNav {
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
    /// # Default Implementation
    ///
    /// Validates no cycle, updates old parent's children, updates
    /// new parent's children, and sets child's parent.
    fn set_parent(&mut self, child: ElementId, new_parent: Option<ElementId>) -> TreeResult<()>;

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
    fn add_child(&mut self, parent: ElementId, child: ElementId) -> TreeResult<()> {
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
    fn detach(&mut self, child: ElementId) -> TreeResult<()> {
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
    fn move_children(&mut self, from: ElementId, to: ElementId) -> TreeResult<()>
    where
        Self: Sized,
    {
        if !self.contains(from) {
            return Err(TreeError::not_found(from));
        }
        if !self.contains(to) {
            return Err(TreeError::not_found(to));
        }

        // Check for cycles: 'to' can't be a descendant of 'from'
        if self.is_descendant(to, from) {
            return Err(TreeError::cycle_detected(to));
        }

        // Collect children first (to avoid borrow issues)
        let children: Vec<_> = self.children(from).to_vec();

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
    fn insert_child(
        &mut self,
        node: Self::Node,
        parent: Option<ElementId>,
    ) -> TreeResult<ElementId> {
        let id = self.insert(node);

        if let Some(parent_id) = parent {
            if !self.contains(parent_id) {
                self.remove(id);
                return Err(TreeError::not_found(parent_id));
            }
            self.set_parent(id, Some(parent_id))?;
        }

        Ok(id)
    }
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

impl<T: TreeWrite + ?Sized> TreeWrite for &mut T {
    #[inline]
    fn get_mut(&mut self, id: ElementId) -> Option<&mut Self::Node> {
        (**self).get_mut(id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> ElementId {
        (**self).insert(node)
    }

    #[inline]
    fn remove(&mut self, id: ElementId) -> Option<Self::Node> {
        (**self).remove(id)
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

impl<T: TreeWrite + ?Sized> TreeWrite for Box<T> {
    #[inline]
    fn get_mut(&mut self, id: ElementId) -> Option<&mut Self::Node> {
        (**self).get_mut(id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> ElementId {
        (**self).insert(node)
    }

    #[inline]
    fn remove(&mut self, id: ElementId) -> Option<Self::Node> {
        (**self).remove(id)
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
    use super::*;
    use crate::traits::{TreeNav, TreeRead};
    use flui_foundation::Slot;

    // Test implementation
    #[derive(Default)]
    struct TestNode {
        value: i32,
        parent: Option<ElementId>,
        children: Vec<ElementId>,
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }
    }

    impl TreeRead for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() as usize - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }

        fn node_ids(&self) -> Option<Box<dyn Iterator<Item = ElementId> + '_>> {
            Some(Box::new(self.nodes.iter().enumerate().filter_map(
                |(i, n)| n.as_ref().map(|_| ElementId::new(i + 1)),
            )))
        }
    }

    impl TreeNav for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> &[ElementId] {
            self.get(id).map(|n| n.children.as_slice()).unwrap_or(&[])
        }

        fn slot(&self, _id: ElementId) -> Option<Slot> {
            None
        }
    }

    impl TreeWrite for TestTree {
        fn get_mut(&mut self, id: ElementId) -> Option<&mut TestNode> {
            self.nodes.get_mut(id.get() as usize - 1)?.as_mut()
        }

        fn insert(&mut self, node: TestNode) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            self.nodes.push(Some(node));
            id
        }

        fn remove(&mut self, id: ElementId) -> Option<TestNode> {
            let index = id.get() as usize - 1;

            // Remove from parent's children
            if let Some(node) = self.nodes.get(index)?.as_ref() {
                if let Some(parent_id) = node.parent {
                    if let Some(Some(parent)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                        parent.children.retain(|&child| child != id);
                    }
                }
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

    impl TreeWriteNav for TestTree {
        fn set_parent(
            &mut self,
            child: ElementId,
            new_parent: Option<ElementId>,
        ) -> TreeResult<()> {
            // Check child exists
            if !self.contains(child) {
                return Err(TreeError::not_found(child));
            }

            // Check new parent exists (if provided)
            if let Some(parent_id) = new_parent {
                if !self.contains(parent_id) {
                    return Err(TreeError::not_found(parent_id));
                }

                // Check for cycles
                if self.is_descendant(parent_id, child) || parent_id == child {
                    return Err(TreeError::cycle_detected(child));
                }
            }

            // Remove from old parent's children
            if let Some(old_parent) = self.parent(child) {
                if let Some(Some(parent_node)) = self.nodes.get_mut(old_parent.get() as usize - 1) {
                    parent_node.children.retain(|&c| c != child);
                }
            }

            // Update child's parent
            if let Some(Some(child_node)) = self.nodes.get_mut(child.get() as usize - 1) {
                child_node.parent = new_parent;
            }

            // Add to new parent's children
            if let Some(parent_id) = new_parent {
                if let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    if !parent_node.children.contains(&child) {
                        parent_node.children.push(child);
                    }
                }
            }

            Ok(())
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

        tree.set_parent(child, Some(root)).unwrap();

        assert_eq!(tree.parent(child), Some(root));
        assert_eq!(tree.children(root), &[child]);
    }

    #[test]
    fn test_cycle_detection() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());

        tree.set_parent(child, Some(root)).unwrap();

        // Trying to make root a child of child should fail
        let result = tree.set_parent(root, Some(child));
        assert!(matches!(result, Err(TreeError::CycleDetected(_))));
    }

    #[test]
    fn test_detach() {
        let mut tree = TestTree::new();
        let root = tree.insert(TestNode::default());
        let child = tree.insert(TestNode::default());

        tree.set_parent(child, Some(root)).unwrap();
        assert_eq!(tree.parent(child), Some(root));

        tree.detach(child).unwrap();
        assert_eq!(tree.parent(child), None);
        assert!(tree.children(root).is_empty());
    }

    #[test]
    fn test_clear() {
        let mut tree = TestTree::new();
        tree.insert(TestNode::default());
        tree.insert(TestNode::default());
        tree.insert(TestNode::default());

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

        tree.set_parent(child1, Some(root)).unwrap();
        tree.set_parent(child2, Some(root)).unwrap();
        tree.set_parent(grandchild, Some(child1)).unwrap();

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
}

//! Mutable tree operations trait.
//!
//! This module provides the [`TreeWrite`] trait for modifying
//! tree structure (insert, remove, reparent).

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
    fn get_mut(&mut self, id: Self::Id) -> Option<&mut Self::Node>;

    /// Inserts a new node into the tree.
    ///
    /// The node is inserted without a parent (as a potential root).
    /// Use [`TreeWriteNav::set_parent`] to establish parent-child relationships.
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
    fn insert(&mut self, node: Self::Node) -> Self::Id;

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
    fn remove(&mut self, id: Self::Id) -> Option<Self::Node>;

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
    fn remove_subtree(&mut self, id: Self::Id) -> usize
    where
        Self: super::TreeNav + Sized,
    {
        // Collect all descendants first (to avoid borrow issues)
        // descendants() returns (Id, depth) tuples
        let to_remove: Vec<_> = self.descendants(id).map(|(id, _depth)| id).collect();
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
    /// # Note
    ///
    /// This method has no default implementation because it requires
    /// access to the internal node structure. Implementations must
    /// provide their own version that:
    /// 1. Validates child and new_parent exist
    /// 2. Checks for cycles (new_parent must not be a descendant of child)
    /// 3. Updates old parent's children list
    /// 4. Updates new parent's children list
    /// 5. Updates child's parent reference
    fn set_parent(
        &mut self,
        child: Self::Id,
        new_parent: Option<Self::Id>,
    ) -> TreeResult<Self::Id, Self::Id>;

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
    fn add_child(&mut self, parent: Self::Id, child: Self::Id) -> TreeResult<Self::Id, Self::Id> {
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
    fn detach(&mut self, child: Self::Id) -> TreeResult<Self::Id, Self::Id> {
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
    fn move_children(&mut self, from: Self::Id, to: Self::Id) -> TreeResult<(), Self::Id>
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
        if self.is_ancestor_of(from, to) {
            return Err(TreeError::cycle_detected(to));
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
    fn insert_child(
        &mut self,
        node: Self::Node,
        parent: Option<Self::Id>,
    ) -> TreeResult<Self::Id, Self::Id> {
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
    fn get_mut(&mut self, id: Self::Id) -> Option<&mut Self::Node> {
        (**self).get_mut(id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> Self::Id {
        (**self).insert(node)
    }

    #[inline]
    fn remove(&mut self, id: Self::Id) -> Option<Self::Node> {
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
    fn get_mut(&mut self, id: Self::Id) -> Option<&mut Self::Node> {
        (**self).get_mut(id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> Self::Id {
        (**self).insert(node)
    }

    #[inline]
    fn remove(&mut self, id: Self::Id) -> Option<Self::Node> {
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
    use crate::iter::{Ancestors, DescendantsWithDepth};
    use crate::traits::{TreeNav, TreeRead};
    use flui_foundation::ElementId;

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

    impl crate::traits::sealed::TreeReadSealed for TestTree {}
    impl crate::traits::sealed::TreeNavSealed for TestTree {}

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }
    }

    impl TreeRead for TestTree {
        type Id = ElementId;
        type Node = TestNode;
        type NodeIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }

        fn node_ids(&self) -> Self::NodeIter<'_> {
            Box::new((0..self.nodes.len()).filter_map(|i| {
                if self.nodes[i].is_some() {
                    Some(ElementId::new(i + 1))
                } else {
                    None
                }
            }))
        }
    }

    impl TreeNav for TestTree {
        type ChildrenIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;
        type AncestorsIter<'a> = Ancestors<'a, Self>;
        type DescendantsIter<'a> = DescendantsWithDepth<'a, Self>;
        type SiblingsIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;

        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
            if let Some(node) = self.get(id) {
                Box::new(node.children.iter().copied())
            } else {
                Box::new(std::iter::empty())
            }
        }

        fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
            Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
            DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
            if let Some(parent_id) = self.parent(id) {
                Box::new(
                    self.children(parent_id)
                        .filter(move |&child_id| child_id != id),
                )
            } else {
                Box::new(std::iter::empty())
            }
        }
    }

    impl TreeWrite for TestTree {
        fn get_mut(&mut self, id: ElementId) -> Option<&mut TestNode> {
            self.nodes.get_mut(id.get() - 1)?.as_mut()
        }

        fn insert(&mut self, node: TestNode) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            self.nodes.push(Some(node));
            id
        }

        fn remove(&mut self, id: ElementId) -> Option<TestNode> {
            let index = id.get() - 1;

            // Remove from parent's children
            if let Some(node) = self.nodes.get(index)?.as_ref() {
                if let Some(parent_id) = node.parent {
                    if let Some(Some(parent)) = self.nodes.get_mut(parent_id.get() - 1) {
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
        ) -> TreeResult<ElementId, ElementId> {
            // Check child exists
            if !self.contains(child) {
                return Err(TreeError::not_found(child));
            }

            // Check new parent exists (if provided)
            if let Some(parent_id) = new_parent {
                if !self.contains(parent_id) {
                    return Err(TreeError::not_found(parent_id));
                }

                // Check for cycles: new_parent must not be a descendant of child
                if self.is_ancestor_of(child, parent_id) || parent_id == child {
                    return Err(TreeError::cycle_detected(child));
                }
            }

            // Remove from old parent's children
            if let Some(old_parent) = self.parent(child) {
                if let Some(Some(parent_node)) = self.nodes.get_mut(old_parent.get() - 1) {
                    parent_node.children.retain(|&c| c != child);
                }
            }

            // Update child's parent
            if let Some(Some(child_node)) = self.nodes.get_mut(child.get() - 1) {
                child_node.parent = new_parent;
            }

            // Add to new parent's children
            if let Some(parent_id) = new_parent {
                if let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() - 1) {
                    if !parent_node.children.contains(&child) {
                        parent_node.children.push(child);
                    }
                }
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
}

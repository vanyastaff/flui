//! Descendant iterators.

use crate::traits::TreeNav;
use smallvec::SmallVec;

/// Stack type for descendants iterator.
///
/// Uses inline storage for up to 32 elements to avoid heap allocation
/// for typical shallow UI trees.
type DescendantStack<Id> = SmallVec<[Id; 32]>;

/// Iterator over descendants of a node (pre-order depth-first).
///
/// Yields elements starting with the root, then recursively visiting
/// children before siblings (depth-first, pre-order).
///
/// # Example
///
/// ```rust,ignore
/// // For tree: root -> [child1, child2 -> grandchild]
/// let descendants: Vec<_> = tree.descendants(root).collect();
/// assert_eq!(descendants, vec![root, child1, child2, grandchild]);
/// ```
///
/// # Performance
///
/// Uses `SmallVec` with inline storage for 32 elements. This avoids heap
/// allocation for typical UI trees where traversal depth rarely exceeds 32.
#[derive(Debug)]
pub struct Descendants<'a, T: TreeNav> {
    tree: &'a T,
    stack: DescendantStack<T::Id>,
}

impl<'a, T: TreeNav> Descendants<'a, T> {
    /// Creates a new descendants iterator starting from the given root.
    #[inline]
    pub fn new(tree: &'a T, root: T::Id) -> Self {
        let mut stack = DescendantStack::new();
        stack.push(root);

        Self { tree, stack }
    }

    /// Returns the tree reference.
    #[inline]
    pub fn tree(&self) -> &'a T {
        self.tree
    }

    /// Returns the current stack depth.
    #[inline]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}

impl<T: TreeNav> Iterator for Descendants<'_, T> {
    type Item = T::Id;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.stack.pop()?;

        // Check if current exists
        if !self.tree.contains(current) {
            return self.next(); // Skip and try next
        }

        // Push children in reverse order (so first child is processed first)
        let children: SmallVec<[T::Id; 8]> = self.tree.children(current).collect();
        for child in children.into_iter().rev() {
            self.stack.push(child);
        }

        Some(current)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        // At least what's on the stack
        (self.stack.len(), None)
    }
}

impl<T: TreeNav> std::iter::FusedIterator for Descendants<'_, T> {}

/// Stack type for descendants-with-depth iterator.
type DescendantDepthStack<Id> = SmallVec<[(Id, usize); 32]>;

/// Iterator over descendants with their depths.
///
/// Yields `(Id, usize)` tuples where depth is relative to
/// the starting root (root has depth 0).
#[derive(Debug)]
pub struct DescendantsWithDepth<'a, T: TreeNav> {
    tree: &'a T,
    stack: DescendantDepthStack<T::Id>,
}

impl<'a, T: TreeNav> DescendantsWithDepth<'a, T> {
    /// Creates a new descendants-with-depth iterator.
    #[inline]
    pub fn new(tree: &'a T, root: T::Id) -> Self {
        let mut stack = DescendantDepthStack::new();
        stack.push((root, 0));

        Self { tree, stack }
    }
}

impl<T: TreeNav> Iterator for DescendantsWithDepth<'_, T> {
    type Item = (T::Id, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let (current, depth) = self.stack.pop()?;

        if !self.tree.contains(current) {
            return self.next();
        }

        let children: SmallVec<[T::Id; 8]> = self.tree.children(current).collect();
        for child in children.into_iter().rev() {
            self.stack.push((child, depth + 1));
        }

        Some((current, depth))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.stack.len(), None)
    }
}

impl<T: TreeNav> std::iter::FusedIterator for DescendantsWithDepth<'_, T> {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iter::Ancestors;
    use crate::traits::TreeRead;
    use flui_foundation::ElementId;

    struct TestNode {
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

        fn insert(&mut self, parent: Option<ElementId>) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            self.nodes.push(Some(TestNode {
                parent,
                children: Vec::new(),
            }));

            if let Some(parent_id) = parent {
                if let Some(Some(p)) = self.nodes.get_mut(parent_id.get() - 1) {
                    p.children.push(id);
                }
            }

            id
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

    #[test]
    fn test_descendants_simple() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child1 = tree.insert(Some(root));
        let child2 = tree.insert(Some(root));

        let descendants: Vec<_> = Descendants::new(&tree, root).collect();
        assert_eq!(descendants, vec![root, child1, child2]);
    }

    #[test]
    fn test_descendants_deep() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child1 = tree.insert(Some(root));
        let child2 = tree.insert(Some(root));
        let grandchild = tree.insert(Some(child2));

        let descendants: Vec<_> = Descendants::new(&tree, root).collect();
        assert_eq!(descendants, vec![root, child1, child2, grandchild]);
    }

    #[test]
    fn test_descendants_single() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);

        let descendants: Vec<_> = Descendants::new(&tree, root).collect();
        assert_eq!(descendants, vec![root]);
    }

    #[test]
    fn test_descendants_with_depth() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child = tree.insert(Some(root));
        let grandchild = tree.insert(Some(child));

        let descendants: Vec<_> = DescendantsWithDepth::new(&tree, root).collect();
        assert_eq!(descendants, vec![(root, 0), (child, 1), (grandchild, 2),]);
    }

    #[test]
    fn test_descendants_stack_overflow() {
        // Create a deep tree to test overflow handling
        let mut tree = TestTree::new();
        let mut parent = tree.insert(None);

        for _ in 0..50 {
            parent = tree.insert(Some(parent));
        }

        // Should not panic
        let count = Descendants::new(&tree, ElementId::new(1)).count();
        assert_eq!(count, 51);
    }
}

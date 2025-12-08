//! Descendant iterators.

use crate::traits::TreeNav;
use flui_foundation::Identifier;
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
pub struct Descendants<'a, I: Identifier, T: TreeNav<I>> {
    tree: &'a T,
    stack: DescendantStack<I>,
}

impl<'a, I: Identifier, T: TreeNav<I>> Descendants<'a, I, T> {
    /// Creates a new descendants iterator starting from the given root.
    #[inline]
    pub fn new(tree: &'a T, root: I) -> Self {
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

impl<I: Identifier, T: TreeNav<I>> Iterator for Descendants<'_, I, T> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.stack.pop()?;

        // Check if current exists
        if !self.tree.contains(current) {
            return self.next(); // Skip and try next
        }

        // Push children in reverse order (so first child is processed first)
        let children: SmallVec<[I; 8]> = self.tree.children(current).collect();
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

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for Descendants<'_, I, T> {}

/// Stack type for descendants-with-depth iterator.
type DescendantDepthStack<Id> = SmallVec<[(Id, usize); 32]>;

/// Iterator over descendants with their depths.
///
/// Yields `(Id, usize)` tuples where depth is relative to
/// the starting root (root has depth 0).
#[derive(Debug)]
pub struct DescendantsWithDepth<'a, I: Identifier, T: TreeNav<I>> {
    tree: &'a T,
    stack: DescendantDepthStack<I>,
}

impl<'a, I: Identifier, T: TreeNav<I>> DescendantsWithDepth<'a, I, T> {
    /// Creates a new descendants-with-depth iterator.
    #[inline]
    pub fn new(tree: &'a T, root: I) -> Self {
        let mut stack = DescendantDepthStack::new();
        stack.push((root, 0));

        Self { tree, stack }
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for DescendantsWithDepth<'_, I, T> {
    type Item = (I, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let (current, depth) = self.stack.pop()?;

        if !self.tree.contains(current) {
            return self.next();
        }

        let children: SmallVec<[I; 8]> = self.tree.children(current).collect();
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

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for DescendantsWithDepth<'_, I, T> {}

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

    impl TreeRead<ElementId> for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }

        fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
            (0..self.nodes.len()).filter_map(|i| {
                if self.nodes[i].is_some() {
                    Some(ElementId::new(i + 1))
                } else {
                    None
                }
            })
        }
    }

    impl TreeNav<ElementId> for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            self.get(id)
                .map(|node| node.children.iter().copied())
                .into_iter()
                .flatten()
        }

        fn ancestors(&self, start: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
            DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            let parent_id = self.parent(id);
            parent_id
                .into_iter()
                .flat_map(move |pid| self.children(pid).filter(move |&cid| cid != id))
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

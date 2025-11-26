//! Breadth-first (level-order) iterator.

use crate::traits::TreeNav;
use flui_foundation::ElementId;
use std::collections::VecDeque;

/// Breadth-first (level-order) iterator.
///
/// Visits all nodes at depth 0, then depth 1, then depth 2, etc.
///
/// # Example
///
/// ```rust,ignore
/// // For tree:
/// //     A
/// //    / \
/// //   B   C
/// //  / \
/// // D   E
///
/// let bfs: Vec<_> = BreadthFirstIter::new(&tree, root).collect();
/// assert_eq!(bfs, vec![A, B, C, D, E]);
/// ```
#[derive(Debug)]
pub struct BreadthFirstIter<'a, T: TreeNav> {
    tree: &'a T,
    queue: VecDeque<ElementId>,
}

impl<'a, T: TreeNav> BreadthFirstIter<'a, T> {
    /// Creates a new breadth-first iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut queue = VecDeque::with_capacity(16);

        if tree.contains(root) {
            queue.push_back(root);
        }

        Self { tree, queue }
    }

    /// Returns the tree reference.
    #[inline]
    pub fn tree(&self) -> &'a T {
        self.tree
    }

    /// Returns the number of nodes in the queue.
    #[inline]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

impl<T: TreeNav> Iterator for BreadthFirstIter<'_, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.queue.pop_front()?;

        // Check if current exists
        if !self.tree.contains(current) {
            return self.next();
        }

        // Add children to back of queue
        for &child in self.tree.children(current) {
            self.queue.push_back(child);
        }

        Some(current)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.queue.len(), None)
    }
}

impl<'a, T: TreeNav> std::iter::FusedIterator for BreadthFirstIter<'a, T> {}

/// Breadth-first iterator with depth information.
///
/// Yields `(ElementId, usize)` tuples.
#[derive(Debug)]
pub struct BreadthFirstIterWithDepth<'a, T: TreeNav> {
    tree: &'a T,
    queue: VecDeque<(ElementId, usize)>,
}

impl<'a, T: TreeNav> BreadthFirstIterWithDepth<'a, T> {
    /// Creates a new breadth-first iterator with depth tracking.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut queue = VecDeque::with_capacity(16);

        if tree.contains(root) {
            queue.push_back((root, 0));
        }

        Self { tree, queue }
    }
}

impl<'a, T: TreeNav> Iterator for BreadthFirstIterWithDepth<'a, T> {
    type Item = (ElementId, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let (current, depth) = self.queue.pop_front()?;

        if !self.tree.contains(current) {
            return self.next();
        }

        for &child in self.tree.children(current) {
            self.queue.push_back((child, depth + 1));
        }

        Some((current, depth))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.queue.len(), None)
    }
}

impl<'a, T: TreeNav> std::iter::FusedIterator for BreadthFirstIterWithDepth<'a, T> {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TreeRead;
    use flui_foundation::Slot;

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
                if let Some(Some(p)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    p.children.push(id);
                }
            }

            id
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

    #[test]
    fn test_bfs_simple() {
        let mut tree = TestTree::new();
        let a = tree.insert(None);
        let b = tree.insert(Some(a));
        let c = tree.insert(Some(a));
        let d = tree.insert(Some(b));
        let e = tree.insert(Some(b));

        let result: Vec<_> = BreadthFirstIter::new(&tree, a).collect();
        // Level 0: A
        // Level 1: B, C
        // Level 2: D, E
        assert_eq!(result, vec![a, b, c, d, e]);
    }

    #[test]
    fn test_bfs_with_depth() {
        let mut tree = TestTree::new();
        let a = tree.insert(None);
        let b = tree.insert(Some(a));
        let c = tree.insert(Some(a));
        let d = tree.insert(Some(b));

        let result: Vec<_> = BreadthFirstIterWithDepth::new(&tree, a).collect();
        assert_eq!(result, vec![(a, 0), (b, 1), (c, 1), (d, 2),]);
    }

    #[test]
    fn test_bfs_single() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);

        let result: Vec<_> = BreadthFirstIter::new(&tree, root).collect();
        assert_eq!(result, vec![root]);
    }

    #[test]
    fn test_bfs_empty() {
        let tree = TestTree::new();
        let fake_id = ElementId::new(999);

        let result: Vec<_> = BreadthFirstIter::new(&tree, fake_id).collect();
        assert!(result.is_empty());
    }
}

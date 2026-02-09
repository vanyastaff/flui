//! Breadth-first (level-order) iterator.

use crate::traits::TreeNav;
use flui_foundation::Identifier;
use std::collections::VecDeque;

/// Breadth-first (level-order) iterator.
///
/// Visits all nodes at depth 0, then depth 1, then depth 2, etc.
///
/// # Example
///
/// ```
/// # use flui_tree::{Ancestors, BreadthFirstIter, TreeNav, TreeRead};
/// # use flui_foundation::ElementId;
/// # struct N { parent: Option<ElementId>, children: Vec<ElementId> }
/// # struct T(Vec<Option<N>>);
/// # impl T { fn ins(&mut self, p: Option<ElementId>) -> ElementId {
/// #     let id = ElementId::new(self.0.len()+1);
/// #     self.0.push(Some(N { parent: p, children: vec![] }));
/// #     if let Some(pid) = p { self.0[pid.get()-1].as_mut().unwrap().children.push(id); }
/// #     id
/// # }}
/// # impl TreeRead<ElementId> for T {
/// #     type Node = N;
/// #     fn get(&self, id: ElementId) -> Option<&N> { self.0.get(id.get()-1)?.as_ref() }
/// #     fn len(&self) -> usize { self.0.iter().flatten().count() }
/// #     fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
/// #         (0..self.0.len()).filter_map(|i| if self.0[i].is_some() { Some(ElementId::new(i+1)) } else { None })
/// #     }
/// # }
/// # impl TreeNav<ElementId> for T {
/// #     fn parent(&self, id: ElementId) -> Option<ElementId> { self.get(id)?.parent }
/// #     fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
/// #         self.get(id).into_iter().flat_map(|n| n.children.iter().copied())
/// #     }
/// #     fn ancestors(&self, s: ElementId) -> impl Iterator<Item = ElementId> + '_ { Ancestors::new(self, s) }
/// #     fn descendants(&self, r: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
/// #         flui_tree::DescendantsWithDepth::new(self, r)
/// #     }
/// #     fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
/// #         self.parent(id).into_iter().flat_map(move |p| self.children(p).filter(move |&c| c != id))
/// #     }
/// # }
/// // For tree:
/// //     A
/// //    / \
/// //   B   C
/// //  / \
/// // D   E
/// # let mut tree = T(vec![]);
/// # let a = tree.ins(None);
/// # let b = tree.ins(Some(a));
/// # let c = tree.ins(Some(a));
/// # let d = tree.ins(Some(b));
/// # let e = tree.ins(Some(b));
///
/// let bfs: Vec<_> = BreadthFirstIter::new(&tree, a).collect();
/// assert_eq!(bfs, vec![a, b, c, d, e]);
/// ```
#[derive(Debug)]
pub struct BreadthFirstIter<'a, I: Identifier, T: TreeNav<I>> {
    tree: &'a T,
    queue: VecDeque<I>,
}

impl<'a, I: Identifier, T: TreeNav<I>> BreadthFirstIter<'a, I, T> {
    /// Creates a new breadth-first iterator.
    #[inline]
    pub fn new(tree: &'a T, root: I) -> Self {
        let mut queue = VecDeque::with_capacity(16);

        if tree.contains(root) {
            queue.push_back(root);
        }

        Self { tree, queue }
    }

    /// Returns the tree reference.
    #[inline]
    #[must_use]
    pub fn tree(&self) -> &'a T {
        self.tree
    }

    /// Returns the number of nodes in the queue.
    #[inline]
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for BreadthFirstIter<'_, I, T> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.queue.pop_front()?;

        // Check if current exists
        if !self.tree.contains(current) {
            return self.next();
        }

        // Add children to back of queue
        for child in self.tree.children(current) {
            self.queue.push_back(child);
        }

        Some(current)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.queue.len(), None)
    }
}

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for BreadthFirstIter<'_, I, T> {}

/// Breadth-first iterator with depth information.
///
/// Yields `(Id, usize)` tuples.
#[derive(Debug)]
#[allow(dead_code)]
pub(super) struct BreadthFirstIterWithDepth<'a, I: Identifier, T: TreeNav<I>> {
    tree: &'a T,
    queue: VecDeque<(I, usize)>,
}

impl<'a, I: Identifier, T: TreeNav<I>> BreadthFirstIterWithDepth<'a, I, T> {
    /// Creates a new breadth-first iterator with depth tracking.
    #[inline]
    #[allow(dead_code)]
    pub(super) fn new(tree: &'a T, root: I) -> Self {
        let mut queue = VecDeque::with_capacity(16);

        if tree.contains(root) {
            queue.push_back((root, 0));
        }

        Self { tree, queue }
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for BreadthFirstIterWithDepth<'_, I, T> {
    type Item = (I, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let (current, depth) = self.queue.pop_front()?;

        if !self.tree.contains(current) {
            return self.next();
        }

        for child in self.tree.children(current) {
            self.queue.push_back((child, depth + 1));
        }

        Some((current, depth))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.queue.len(), None)
    }
}

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator
    for BreadthFirstIterWithDepth<'_, I, T>
{
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iter::{Ancestors, DescendantsWithDepth};
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
    #[allow(clippy::many_single_char_names)]
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

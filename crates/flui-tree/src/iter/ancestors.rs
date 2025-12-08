//! Ancestor iterators.

use crate::traits::TreeNav;
use flui_foundation::Identifier;

/// Iterator over ancestors of a node.
///
/// Yields elements from the starting node up to and including the root.
/// The first element yielded is always the starting node itself.
///
/// # Example
///
/// ```rust,ignore
/// // For tree: root -> parent -> child
/// let ancestors: Vec<_> = tree.ancestors(child).collect();
/// assert_eq!(ancestors, vec![child, parent, root]);
/// ```
#[derive(Debug, Clone)]
pub struct Ancestors<'a, I: Identifier, T: TreeNav<I>> {
    tree: &'a T,
    current: Option<I>,
}

impl<'a, I: Identifier, T: TreeNav<I>> Ancestors<'a, I, T> {
    /// Creates a new ancestors iterator starting from the given node.
    #[inline]
    pub fn new(tree: &'a T, start: I) -> Self {
        Self {
            tree,
            current: Some(start),
        }
    }

    /// Returns the tree reference.
    #[inline]
    pub fn tree(&self) -> &'a T {
        self.tree
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for Ancestors<'_, I, T> {
    type Item = I;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;

        // Check if current exists in tree
        if !self.tree.contains(current) {
            self.current = None;
            return None;
        }

        // Move to parent for next iteration
        self.current = self.tree.parent(current);

        Some(current)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        // We know there's at least 1 element if current is Some
        // Upper bound is MAX_DEPTH from the trait (typical tree depth)
        if self.current.is_some() {
            (1, Some(T::MAX_DEPTH))
        } else {
            (0, Some(0))
        }
    }
}

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for Ancestors<'_, I, T> {}

/// Iterator over ancestors with their depths.
///
/// Yields `(Id, usize)` tuples where the depth is relative
/// to the starting node (starting node has depth 0).
///
/// # Example
///
/// ```rust,ignore
/// // For tree: root (depth 2) -> parent (depth 1) -> child (depth 0)
/// let ancestors: Vec<_> = tree.ancestors_with_depth(child).collect();
/// assert_eq!(ancestors, vec![
///     (child, 0),
///     (parent, 1),
///     (root, 2),
/// ]);
/// ```
#[derive(Debug, Clone)]
pub struct AncestorsWithDepth<'a, I: Identifier, T: TreeNav<I>> {
    inner: Ancestors<'a, I, T>,
    depth: usize,
}

impl<'a, I: Identifier, T: TreeNav<I>> AncestorsWithDepth<'a, I, T> {
    /// Creates a new ancestors-with-depth iterator.
    #[inline]
    pub fn new(tree: &'a T, start: I) -> Self {
        Self {
            inner: Ancestors::new(tree, start),
            depth: 0,
        }
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for AncestorsWithDepth<'_, I, T> {
    type Item = (I, usize);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.inner.next()?;
        let current_depth = self.depth;
        self.depth += 1;
        Some((id, current_depth))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for AncestorsWithDepth<'_, I, T> {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iter::DescendantsWithDepth;
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
    fn test_ancestors() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let parent = tree.insert(Some(root));
        let child = tree.insert(Some(parent));

        let ancestors: Vec<_> = Ancestors::new(&tree, child).collect();
        assert_eq!(ancestors, vec![child, parent, root]);
    }

    #[test]
    fn test_ancestors_root() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);

        let ancestors: Vec<_> = Ancestors::new(&tree, root).collect();
        assert_eq!(ancestors, vec![root]);
    }

    #[test]
    fn test_ancestors_nonexistent() {
        let tree = TestTree::new();
        let fake_id = ElementId::new(999);

        let ancestors: Vec<_> = Ancestors::new(&tree, fake_id).collect();
        assert!(ancestors.is_empty());
    }

    #[test]
    fn test_ancestors_with_depth() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let parent = tree.insert(Some(root));
        let child = tree.insert(Some(parent));

        let ancestors: Vec<_> = AncestorsWithDepth::new(&tree, child).collect();
        assert_eq!(ancestors, vec![(child, 0), (parent, 1), (root, 2),]);
    }
}

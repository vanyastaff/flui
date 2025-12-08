//! Configurable depth-first iterator.

use crate::traits::TreeNav;
use flui_foundation::Identifier;

/// Depth-first traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DepthFirstOrder {
    /// Pre-order: visit parent before children.
    ///
    /// ```text
    /// A -> B -> D -> E -> C -> F
    ///     A
    ///    / \
    ///   B   C
    ///  / \   \
    /// D   E   F
    /// ```
    #[default]
    PreOrder,

    /// Post-order: visit children before parent.
    ///
    /// ```text
    /// D -> E -> B -> F -> C -> A
    ///     A
    ///    / \
    ///   B   C
    ///  / \   \
    /// D   E   F
    /// ```
    PostOrder,
}

/// Configurable depth-first iterator.
///
/// Supports both pre-order and post-order traversal.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{DepthFirstIter, DepthFirstOrder};
///
/// // Post-order traversal (children before parents)
/// let post_order: Vec<_> = DepthFirstIter::new(&tree, root, DepthFirstOrder::PostOrder)
///     .collect();
/// ```
#[derive(Debug)]
pub struct DepthFirstIter<'a, I: Identifier, T: TreeNav<I>> {
    tree: &'a T,
    order: DepthFirstOrder,
    // For pre-order: stack of nodes to visit
    // For post-order: stack of (node, visited_children)
    stack: Vec<StackEntry<I>>,
}

#[derive(Debug, Clone)]
struct StackEntry<I> {
    id: I,
    /// For post-order: index of next child to process
    child_index: usize,
}

impl<'a, I: Identifier, T: TreeNav<I>> DepthFirstIter<'a, I, T> {
    /// Creates a new depth-first iterator.
    #[inline]
    pub fn new(tree: &'a T, root: I, order: DepthFirstOrder) -> Self {
        let mut stack = Vec::with_capacity(16);

        if tree.contains(root) {
            stack.push(StackEntry {
                id: root,
                child_index: 0,
            });
        }

        Self { tree, order, stack }
    }

    /// Creates a pre-order iterator.
    #[inline]
    pub fn pre_order(tree: &'a T, root: I) -> Self {
        Self::new(tree, root, DepthFirstOrder::PreOrder)
    }

    /// Creates a post-order iterator.
    #[inline]
    pub fn post_order(tree: &'a T, root: I) -> Self {
        Self::new(tree, root, DepthFirstOrder::PostOrder)
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for DepthFirstIter<'_, I, T> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        match self.order {
            DepthFirstOrder::PreOrder => self.next_pre_order(),
            DepthFirstOrder::PostOrder => self.next_post_order(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.stack.len(), None)
    }
}

impl<I: Identifier, T: TreeNav<I>> DepthFirstIter<'_, I, T> {
    fn next_pre_order(&mut self) -> Option<I> {
        let entry = self.stack.pop()?;
        let current = entry.id;

        // Push children in reverse order
        let children: Vec<_> = self.tree.children(current).collect();
        for child in children.into_iter().rev() {
            self.stack.push(StackEntry {
                id: child,
                child_index: 0,
            });
        }

        Some(current)
    }

    fn next_post_order(&mut self) -> Option<I> {
        loop {
            let entry = self.stack.last_mut()?;
            let children: Vec<_> = self.tree.children(entry.id).collect();

            if entry.child_index < children.len() {
                // More children to process
                let child = children[entry.child_index];
                entry.child_index += 1;

                self.stack.push(StackEntry {
                    id: child,
                    child_index: 0,
                });
            } else {
                // All children processed, return this node
                let entry = self.stack.pop()?;
                return Some(entry.id);
            }
        }
    }
}

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for DepthFirstIter<'_, I, T> {}

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

    impl TreeRead<ElementId> for TestTree {
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

    impl TreeNav<ElementId> for TestTree {
        type ChildrenIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;
        type AncestorsIter<'a> = Ancestors<'a, ElementId, Self>;
        type DescendantsIter<'a> = DescendantsWithDepth<'a, ElementId, Self>;
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

    // Build test tree:
    //     A(1)
    //    / \
    //   B(2) C(3)
    //  / \    \
    // D(4) E(5) F(6)
    #[allow(clippy::many_single_char_names)]
    fn build_test_tree() -> (TestTree, [ElementId; 6]) {
        let mut tree = TestTree::new();
        let a = tree.insert(None);
        let b = tree.insert(Some(a));
        let c = tree.insert(Some(a));
        let d = tree.insert(Some(b));
        let e = tree.insert(Some(b));
        let f = tree.insert(Some(c));

        (tree, [a, b, c, d, e, f])
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn test_pre_order() {
        let (tree, [a, b, c, d, e, f]) = build_test_tree();

        let result: Vec<_> = DepthFirstIter::pre_order(&tree, a).collect();
        assert_eq!(result, vec![a, b, d, e, c, f]);
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn test_post_order() {
        let (tree, [a, b, c, d, e, f]) = build_test_tree();

        let result: Vec<_> = DepthFirstIter::post_order(&tree, a).collect();
        assert_eq!(result, vec![d, e, b, f, c, a]);
    }

    #[test]
    fn test_single_node() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);

        let pre: Vec<_> = DepthFirstIter::pre_order(&tree, root).collect();
        let post: Vec<_> = DepthFirstIter::post_order(&tree, root).collect();

        assert_eq!(pre, vec![root]);
        assert_eq!(post, vec![root]);
    }

    #[test]
    fn test_empty() {
        let tree = TestTree::new();
        let fake_id = ElementId::new(999);

        let result: Vec<_> = DepthFirstIter::pre_order(&tree, fake_id).collect();
        assert!(result.is_empty());
    }
}

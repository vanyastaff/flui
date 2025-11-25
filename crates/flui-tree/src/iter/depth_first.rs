//! Configurable depth-first iterator.

use crate::traits::TreeNav;
use flui_foundation::ElementId;

/// Depth-first traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

impl Default for DepthFirstOrder {
    fn default() -> Self {
        Self::PreOrder
    }
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
pub struct DepthFirstIter<'a, T: TreeNav> {
    tree: &'a T,
    order: DepthFirstOrder,
    // For pre-order: stack of nodes to visit
    // For post-order: stack of (node, visited_children)
    stack: Vec<StackEntry>,
}

#[derive(Debug, Clone)]
struct StackEntry {
    id: ElementId,
    /// For post-order: index of next child to process
    child_index: usize,
    /// For post-order: whether we've visited this node yet
    visited: bool,
}

impl<'a, T: TreeNav> DepthFirstIter<'a, T> {
    /// Creates a new depth-first iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId, order: DepthFirstOrder) -> Self {
        let mut stack = Vec::with_capacity(16);

        if tree.contains(root) {
            stack.push(StackEntry {
                id: root,
                child_index: 0,
                visited: false,
            });
        }

        Self { tree, order, stack }
    }

    /// Creates a pre-order iterator.
    #[inline]
    pub fn pre_order(tree: &'a T, root: ElementId) -> Self {
        Self::new(tree, root, DepthFirstOrder::PreOrder)
    }

    /// Creates a post-order iterator.
    #[inline]
    pub fn post_order(tree: &'a T, root: ElementId) -> Self {
        Self::new(tree, root, DepthFirstOrder::PostOrder)
    }
}

impl<'a, T: TreeNav> Iterator for DepthFirstIter<'a, T> {
    type Item = ElementId;

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

impl<'a, T: TreeNav> DepthFirstIter<'a, T> {
    fn next_pre_order(&mut self) -> Option<ElementId> {
        let entry = self.stack.pop()?;
        let current = entry.id;

        // Push children in reverse order
        let children = self.tree.children(current);
        for &child in children.iter().rev() {
            self.stack.push(StackEntry {
                id: child,
                child_index: 0,
                visited: false,
            });
        }

        Some(current)
    }

    fn next_post_order(&mut self) -> Option<ElementId> {
        loop {
            let entry = self.stack.last_mut()?;
            let children = self.tree.children(entry.id);

            if entry.child_index < children.len() {
                // More children to process
                let child = children[entry.child_index];
                entry.child_index += 1;

                self.stack.push(StackEntry {
                    id: child,
                    child_index: 0,
                    visited: false,
                });
            } else {
                // All children processed, return this node
                let entry = self.stack.pop()?;
                return Some(entry.id);
            }
        }
    }
}

impl<'a, T: TreeNav> std::iter::FusedIterator for DepthFirstIter<'a, T> {}

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

    // Build test tree:
    //     A(1)
    //    / \
    //   B(2) C(3)
    //  / \    \
    // D(4) E(5) F(6)
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
    fn test_pre_order() {
        let (tree, [a, b, c, d, e, f]) = build_test_tree();

        let result: Vec<_> = DepthFirstIter::pre_order(&tree, a).collect();
        assert_eq!(result, vec![a, b, d, e, c, f]);
    }

    #[test]
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

//! Descendant iterators.

use crate::traits::TreeNav;
use flui_foundation::ElementId;

/// Stack storage optimization.
/// Uses inline array for shallow trees, heap for deep ones.
const INLINE_STACK_SIZE: usize = 32;

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
/// Uses a small inline stack for trees up to 32 levels deep, falling
/// back to heap allocation for deeper trees.
#[derive(Debug)]
pub struct Descendants<'a, T: TreeNav> {
    tree: &'a T,
    stack: DescendantStack,
}

impl<'a, T: TreeNav> Descendants<'a, T> {
    /// Creates a new descendants iterator starting from the given root.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
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

impl<'a, T: TreeNav> Iterator for Descendants<'a, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.stack.pop()?;

        // Check if current exists
        if !self.tree.contains(current) {
            return self.next(); // Skip and try next
        }

        // Push children in reverse order (so first child is processed first)
        let children = self.tree.children(current);
        for &child in children.iter().rev() {
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

impl<'a, T: TreeNav> std::iter::FusedIterator for Descendants<'a, T> {}

/// Iterator over descendants with their depths.
///
/// Yields `(ElementId, usize)` tuples where depth is relative to
/// the starting root (root has depth 0).
#[derive(Debug)]
pub struct DescendantsWithDepth<'a, T: TreeNav> {
    tree: &'a T,
    stack: DescendantStackWithDepth,
}

impl<'a, T: TreeNav> DescendantsWithDepth<'a, T> {
    /// Creates a new descendants-with-depth iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut stack = DescendantStackWithDepth::new();
        stack.push((root, 0));

        Self { tree, stack }
    }
}

impl<'a, T: TreeNav> Iterator for DescendantsWithDepth<'a, T> {
    type Item = (ElementId, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let (current, depth) = self.stack.pop()?;

        if !self.tree.contains(current) {
            return self.next();
        }

        let children = self.tree.children(current);
        for &child in children.iter().rev() {
            self.stack.push((child, depth + 1));
        }

        Some((current, depth))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.stack.len(), None)
    }
}

impl<'a, T: TreeNav> std::iter::FusedIterator for DescendantsWithDepth<'a, T> {}

// ============================================================================
// STACK IMPLEMENTATION
// ============================================================================

/// Stack with inline storage optimization.
#[derive(Debug)]
struct DescendantStack {
    inline: [ElementId; INLINE_STACK_SIZE],
    inline_len: usize,
    overflow: Vec<ElementId>,
}

impl DescendantStack {
    fn new() -> Self {
        Self {
            inline: [ElementId::new(0); INLINE_STACK_SIZE],
            inline_len: 0,
            overflow: Vec::new(),
        }
    }

    fn push(&mut self, id: ElementId) {
        if self.inline_len < INLINE_STACK_SIZE {
            self.inline[self.inline_len] = id;
            self.inline_len += 1;
        } else {
            self.overflow.push(id);
        }
    }

    fn pop(&mut self) -> Option<ElementId> {
        if let Some(id) = self.overflow.pop() {
            Some(id)
        } else if self.inline_len > 0 {
            self.inline_len -= 1;
            Some(self.inline[self.inline_len])
        } else {
            None
        }
    }

    fn len(&self) -> usize {
        self.inline_len + self.overflow.len()
    }
}

/// Stack with depth tracking.
#[derive(Debug)]
struct DescendantStackWithDepth {
    inline: [(ElementId, usize); INLINE_STACK_SIZE],
    inline_len: usize,
    overflow: Vec<(ElementId, usize)>,
}

impl DescendantStackWithDepth {
    fn new() -> Self {
        Self {
            inline: [(ElementId::new(0), 0); INLINE_STACK_SIZE],
            inline_len: 0,
            overflow: Vec::new(),
        }
    }

    fn push(&mut self, item: (ElementId, usize)) {
        if self.inline_len < INLINE_STACK_SIZE {
            self.inline[self.inline_len] = item;
            self.inline_len += 1;
        } else {
            self.overflow.push(item);
        }
    }

    fn pop(&mut self) -> Option<(ElementId, usize)> {
        if let Some(item) = self.overflow.pop() {
            Some(item)
        } else if self.inline_len > 0 {
            self.inline_len -= 1;
            Some(self.inline[self.inline_len])
        } else {
            None
        }
    }

    fn len(&self) -> usize {
        self.inline_len + self.overflow.len()
    }
}

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
            let id = ElementId::new(self.nodes.len() as u64 + 1);
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

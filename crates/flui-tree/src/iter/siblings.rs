//! Sibling iterators.
//!
//! Provides two types of sibling iteration:
//! - [`Siblings`]: Directional iteration (forward or backward from a node)
//! - [`AllSiblings`]: All siblings of a node (excluding self)

use crate::traits::TreeNav;
use flui_foundation::Identifier;

/// Direction for sibling iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SiblingsDirection {
    /// Iterate forward (increasing index).
    #[default]
    Forward,
    /// Iterate backward (decreasing index).
    Backward,
}

/// Iterator over siblings of a node.
///
/// Iterates through siblings in the specified direction, optionally
/// including the starting node.
///
/// # Example
///
/// ```rust,ignore
/// // For parent with children [A, B, C, D]
/// // Starting from B, forward:
/// let siblings: Vec<_> = tree.siblings(b, Forward, false).collect();
/// assert_eq!(siblings, vec![C, D]);
///
/// // Starting from C, backward, including self:
/// let siblings: Vec<_> = tree.siblings(c, Backward, true).collect();
/// assert_eq!(siblings, vec![C, B, A]);
/// ```
#[derive(Debug)]
pub struct Siblings<'a, I: Identifier, T: TreeNav<I>> {
    _tree: &'a T,
    /// Parent's children list (owned)
    children: Vec<I>,
    /// Current index in siblings list
    current_index: Option<usize>,
    /// Direction of iteration
    direction: SiblingsDirection,
    /// Whether we've yielded the first element
    started: bool,
    /// Whether to include the starting node
    include_self: bool,
}

impl<'a, I: Identifier, T: TreeNav<I>> Siblings<'a, I, T> {
    /// Creates a new siblings iterator.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree to iterate over
    /// * `start` - The starting node
    /// * `direction` - Direction to iterate
    /// * `include_self` - Whether to include the starting node
    pub fn new(tree: &'a T, start: I, direction: SiblingsDirection, include_self: bool) -> Self {
        // Get parent and find index
        let (children, current_index) = if let Some(parent) = tree.parent(start) {
            let sibs: Vec<_> = tree.children(parent).collect();
            let idx = sibs.iter().position(|&id| id == start);
            (sibs, idx)
        } else {
            // No parent means no siblings
            (Vec::new(), None)
        };

        Self {
            _tree: tree,
            children,
            current_index,
            direction,
            started: false,
            include_self,
        }
    }

    /// Creates a forward siblings iterator (not including self).
    #[inline]
    pub fn forward(tree: &'a T, start: I) -> Self {
        Self::new(tree, start, SiblingsDirection::Forward, false)
    }

    /// Creates a backward siblings iterator (not including self).
    #[inline]
    pub fn backward(tree: &'a T, start: I) -> Self {
        Self::new(tree, start, SiblingsDirection::Backward, false)
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for Siblings<'_, I, T> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.current_index?;

        if !self.started {
            self.started = true;

            if self.include_self {
                return Some(self.children[idx]);
            }

            // Move to first sibling
            match self.direction {
                SiblingsDirection::Forward => {
                    if idx + 1 < self.children.len() {
                        self.current_index = Some(idx + 1);
                        return Some(self.children[idx + 1]);
                    }
                    self.current_index = None;
                    return None;
                }
                SiblingsDirection::Backward => {
                    if idx > 0 {
                        self.current_index = Some(idx - 1);
                        return Some(self.children[idx - 1]);
                    }
                    self.current_index = None;
                    return None;
                }
            }
        }

        // Continue iteration
        match self.direction {
            SiblingsDirection::Forward => {
                if idx + 1 < self.children.len() {
                    self.current_index = Some(idx + 1);
                    Some(self.children[idx + 1])
                } else {
                    self.current_index = None;
                    None
                }
            }
            SiblingsDirection::Backward => {
                if idx > 0 {
                    self.current_index = Some(idx - 1);
                    Some(self.children[idx - 1])
                } else {
                    self.current_index = None;
                    None
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.current_index {
            Some(idx) => {
                let remaining = match self.direction {
                    SiblingsDirection::Forward => self.children.len().saturating_sub(idx + 1),
                    SiblingsDirection::Backward => idx,
                };
                let extra = usize::from(self.include_self && !self.started);
                (remaining + extra, Some(remaining + extra))
            }
            None => (0, Some(0)),
        }
    }
}

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for Siblings<'_, I, T> {}
impl<I: Identifier, T: TreeNav<I>> std::iter::ExactSizeIterator for Siblings<'_, I, T> {}

// ============================================================================
// ALL SIBLINGS ITERATOR
// ============================================================================

/// Iterator over ALL siblings of a node (excluding self).
///
/// Unlike `Siblings` which iterates in one direction, this iterates
/// through all siblings from first to last, skipping the node itself.
///
/// # Example
///
/// ```rust,ignore
/// // For parent with children [A, B, C, D]
/// // Starting from B:
/// let siblings: Vec<_> = AllSiblings::new(&tree, b).collect();
/// assert_eq!(siblings, vec![A, C, D]);
/// ```
#[derive(Debug)]
pub struct AllSiblings<'a, I: Identifier, T: TreeNav<I>> {
    _tree: &'a T,
    /// Parent's children list (owned)
    children: Vec<I>,
    /// Current index in siblings list
    index: usize,
    /// The node to exclude
    exclude_id: I,
}

impl<'a, I: Identifier, T: TreeNav<I>> AllSiblings<'a, I, T> {
    /// Creates a new iterator over all siblings.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree to iterate over
    /// * `node` - The node whose siblings to iterate (excluded from results)
    pub fn new(tree: &'a T, node: I) -> Self {
        let children = if let Some(parent) = tree.parent(node) {
            tree.children(parent).collect()
        } else {
            Vec::new()
        };

        Self {
            _tree: tree,
            children,
            index: 0,
            exclude_id: node,
        }
    }
}

impl<I: Identifier, T: TreeNav<I>> Iterator for AllSiblings<'_, I, T> {
    type Item = I;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.children.len() {
            let id = self.children[self.index];
            self.index += 1;
            if id != self.exclude_id {
                return Some(id);
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.children.len().saturating_sub(self.index);
        // May need to subtract 1 if exclude_id is still ahead
        let has_exclude_ahead = self.children[self.index..].contains(&self.exclude_id);
        let count = if has_exclude_ahead {
            remaining.saturating_sub(1)
        } else {
            remaining
        };
        (count, Some(count))
    }
}

impl<I: Identifier, T: TreeNav<I>> std::iter::FusedIterator for AllSiblings<'_, I, T> {}

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

    #[test]
    fn test_siblings_forward() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let _a = tree.insert(Some(root));
        let b = tree.insert(Some(root));
        let c = tree.insert(Some(root));
        let d = tree.insert(Some(root));

        // From B, forward, not including self
        let siblings: Vec<_> = Siblings::forward(&tree, b).collect();
        assert_eq!(siblings, vec![c, d]);
    }

    #[test]
    fn test_siblings_backward() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let a = tree.insert(Some(root));
        let b = tree.insert(Some(root));
        let c = tree.insert(Some(root));
        let _d = tree.insert(Some(root));

        // From C, backward, not including self
        let siblings: Vec<_> = Siblings::backward(&tree, c).collect();
        assert_eq!(siblings, vec![b, a]);
    }

    #[test]
    fn test_siblings_include_self() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let _a = tree.insert(Some(root));
        let b = tree.insert(Some(root));
        let c = tree.insert(Some(root));

        // From B, forward, including self
        let siblings: Vec<_> = Siblings::new(&tree, b, SiblingsDirection::Forward, true).collect();
        assert_eq!(siblings, vec![b, c]);
    }

    #[test]
    fn test_siblings_no_siblings() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let only_child = tree.insert(Some(root));

        let siblings: Vec<_> = Siblings::forward(&tree, only_child).collect();
        assert!(siblings.is_empty());
    }

    #[test]
    fn test_siblings_root() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);

        // Root has no parent, so no siblings
        let siblings: Vec<_> = Siblings::forward(&tree, root).collect();
        assert!(siblings.is_empty());
    }

    #[test]
    fn test_siblings_exact_size() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let _a = tree.insert(Some(root));
        let b = tree.insert(Some(root));
        let _c = tree.insert(Some(root));
        let _d = tree.insert(Some(root));

        let siblings = Siblings::forward(&tree, b);
        assert_eq!(siblings.len(), 2); // c, d
    }

    #[test]
    fn test_all_siblings() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let a = tree.insert(Some(root));
        let b = tree.insert(Some(root));
        let c = tree.insert(Some(root));
        let d = tree.insert(Some(root));

        // From B, get all siblings (A, C, D)
        let siblings: Vec<_> = AllSiblings::new(&tree, b).collect();
        assert_eq!(siblings.len(), 3);
        assert_eq!(siblings, vec![a, c, d]);
    }

    #[test]
    fn test_all_siblings_first_child() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let a = tree.insert(Some(root));
        let b = tree.insert(Some(root));
        let c = tree.insert(Some(root));

        // From A (first child), get all siblings (B, C)
        let siblings: Vec<_> = AllSiblings::new(&tree, a).collect();
        assert_eq!(siblings, vec![b, c]);
    }

    #[test]
    fn test_all_siblings_last_child() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let a = tree.insert(Some(root));
        let b = tree.insert(Some(root));
        let c = tree.insert(Some(root));

        // From C (last child), get all siblings (A, B)
        let siblings: Vec<_> = AllSiblings::new(&tree, c).collect();
        assert_eq!(siblings, vec![a, b]);
    }

    #[test]
    fn test_all_siblings_only_child() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let only = tree.insert(Some(root));

        let siblings: Vec<_> = AllSiblings::new(&tree, only).collect();
        assert!(siblings.is_empty());
    }

    #[test]
    fn test_all_siblings_root() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);

        // Root has no parent, so no siblings
        let siblings: Vec<_> = AllSiblings::new(&tree, root).collect();
        assert!(siblings.is_empty());
    }
}

//! Sibling iterators.

use crate::traits::TreeNav;
use flui_foundation::ElementId;

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
pub struct Siblings<'a, T: TreeNav> {
    _tree: &'a T,
    /// Parent's children list
    children: &'a [ElementId],
    /// Current index in siblings list
    current_index: Option<usize>,
    /// Direction of iteration
    direction: SiblingsDirection,
    /// Whether we've yielded the first element
    started: bool,
    /// Whether to include the starting node
    include_self: bool,
}

impl<'a, T: TreeNav> Siblings<'a, T> {
    /// Creates a new siblings iterator.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree to iterate over
    /// * `start` - The starting node
    /// * `direction` - Direction to iterate
    /// * `include_self` - Whether to include the starting node
    pub fn new(
        tree: &'a T,
        start: ElementId,
        direction: SiblingsDirection,
        include_self: bool,
    ) -> Self {
        // Get parent and find index
        let (children, current_index) = if let Some(parent) = tree.parent(start) {
            let sibs = tree.children(parent);
            let idx = sibs.iter().position(|&id| id == start);
            (sibs, idx)
        } else {
            // No parent means no siblings
            (&[][..], None)
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
    pub fn forward(tree: &'a T, start: ElementId) -> Self {
        Self::new(tree, start, SiblingsDirection::Forward, false)
    }

    /// Creates a backward siblings iterator (not including self).
    #[inline]
    pub fn backward(tree: &'a T, start: ElementId) -> Self {
        Self::new(tree, start, SiblingsDirection::Backward, false)
    }
}

impl<T: TreeNav> Iterator for Siblings<'_, T> {
    type Item = ElementId;

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

impl<T: TreeNav> std::iter::FusedIterator for Siblings<'_, T> {}
impl<T: TreeNav> std::iter::ExactSizeIterator for Siblings<'_, T> {}

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
    fn test_siblings_forward() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let a = tree.insert(Some(root));
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
        let d = tree.insert(Some(root));

        // From C, backward, not including self
        let siblings: Vec<_> = Siblings::backward(&tree, c).collect();
        assert_eq!(siblings, vec![b, a]);
    }

    #[test]
    fn test_siblings_include_self() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let a = tree.insert(Some(root));
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
}

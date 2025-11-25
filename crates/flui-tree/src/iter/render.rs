//! Render-specific iterators.
//!
//! These iterators filter to only render elements, skipping
//! non-render elements like StatelessView wrappers.

use crate::traits::RenderTreeAccess;
use flui_foundation::ElementId;

/// Iterator over render ancestors.
///
/// Like [`Ancestors`](super::Ancestors) but only yields elements that
/// are render elements (have a RenderObject).
///
/// # Use Case
///
/// Finding the render parent when the element tree contains non-render
/// elements (Component, Provider wrappers).
///
/// # Example
///
/// ```rust,ignore
/// // Tree: RenderBox -> StatelessWrapper -> RenderFlex
/// // RenderAncestors from RenderFlex yields: [RenderFlex, RenderBox]
/// // (skipping StatelessWrapper)
/// ```
#[derive(Debug)]
pub struct RenderAncestors<'a, T: RenderTreeAccess> {
    tree: &'a T,
    current: Option<ElementId>,
}

impl<'a, T: RenderTreeAccess> RenderAncestors<'a, T> {
    /// Creates a new render ancestors iterator.
    #[inline]
    pub fn new(tree: &'a T, start: ElementId) -> Self {
        Self {
            tree,
            current: Some(start),
        }
    }
}

impl<'a, T: RenderTreeAccess> Iterator for RenderAncestors<'a, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let current = self.current?;

            if !self.tree.contains(current) {
                self.current = None;
                return None;
            }

            // Move to parent for next iteration
            self.current = self.tree.parent(current);

            // Only yield if it's a render element
            if self.tree.is_render_element(current) {
                return Some(current);
            }
            // Otherwise continue to next ancestor
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<'a, T: RenderTreeAccess> std::iter::FusedIterator for RenderAncestors<'a, T> {}

/// Iterator over render descendants.
///
/// Like [`Descendants`](super::Descendants) but only yields elements
/// that are render elements.
///
/// # Use Case
///
/// Collecting all render objects that need layout/paint, skipping
/// wrapper elements.
#[derive(Debug)]
pub struct RenderDescendants<'a, T: RenderTreeAccess> {
    tree: &'a T,
    stack: Vec<ElementId>,
}

impl<'a, T: RenderTreeAccess> RenderDescendants<'a, T> {
    /// Creates a new render descendants iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut stack = Vec::with_capacity(16);

        if tree.contains(root) {
            stack.push(root);
        }

        Self { tree, stack }
    }
}

impl<'a, T: RenderTreeAccess> Iterator for RenderDescendants<'a, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let current = self.stack.pop()?;

            if !self.tree.contains(current) {
                continue;
            }

            // Always push children (even from non-render elements)
            let children = self.tree.children(current);
            for &child in children.iter().rev() {
                self.stack.push(child);
            }

            // Only yield if it's a render element
            if self.tree.is_render_element(current) {
                return Some(current);
            }
            // Otherwise continue to next element
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<'a, T: RenderTreeAccess> std::iter::FusedIterator for RenderDescendants<'a, T> {}

/// Iterator that finds render children of a render element.
///
/// Unlike `RenderDescendants`, this stops at render boundaries.
/// It finds the immediate render children, skipping non-render
/// wrapper elements but not recursing into other render subtrees.
///
/// # Use Case
///
/// During layout, a render parent needs to find its render children
/// to call `performLayout` on them.
#[derive(Debug)]
pub struct RenderChildren<'a, T: RenderTreeAccess> {
    tree: &'a T,
    stack: Vec<ElementId>,
}

impl<'a, T: RenderTreeAccess> RenderChildren<'a, T> {
    /// Creates a new render children iterator.
    #[inline]
    pub fn new(tree: &'a T, parent: ElementId) -> Self {
        let mut stack = Vec::with_capacity(8);

        // Start with direct children
        if tree.contains(parent) {
            for &child in tree.children(parent).iter().rev() {
                stack.push(child);
            }
        }

        Self { tree, stack }
    }
}

impl<'a, T: RenderTreeAccess> Iterator for RenderChildren<'a, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let current = self.stack.pop()?;

            if !self.tree.contains(current) {
                continue;
            }

            if self.tree.is_render_element(current) {
                // Found a render child - don't recurse further
                return Some(current);
            }

            // Non-render element - look at its children
            let children = self.tree.children(current);
            for &child in children.iter().rev() {
                self.stack.push(child);
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.stack.len()))
    }
}

impl<'a, T: RenderTreeAccess> std::iter::FusedIterator for RenderChildren<'a, T> {}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Finds the nearest render ancestor of an element.
///
/// Convenience function wrapping `RenderAncestors`.
#[inline]
pub fn find_render_ancestor<T: RenderTreeAccess>(tree: &T, id: ElementId) -> Option<ElementId> {
    RenderAncestors::new(tree, id).nth(1) // Skip self, get first ancestor
}

/// Finds the render parent of an element.
///
/// Same as `find_render_ancestor` but more semantic.
#[inline]
pub fn render_parent<T: RenderTreeAccess>(tree: &T, id: ElementId) -> Option<ElementId> {
    find_render_ancestor(tree, id)
}

/// Collects all render children of a render element.
#[inline]
pub fn collect_render_children<T: RenderTreeAccess>(tree: &T, parent: ElementId) -> Vec<ElementId> {
    RenderChildren::new(tree, parent).collect()
}

/// Counts render elements in a subtree.
#[inline]
pub fn count_render_elements<T: RenderTreeAccess>(tree: &T, root: ElementId) -> usize {
    RenderDescendants::new(tree, root).count()
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{TreeNav, TreeRead};
    use flui_foundation::Slot;
    use std::any::Any;

    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        is_render: bool,
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(&mut self, parent: Option<ElementId>, is_render: bool) -> ElementId {
            let id = ElementId::new(self.nodes.len() as u64 + 1);
            self.nodes.push(Some(TestNode {
                parent,
                children: Vec::new(),
                is_render,
            }));

            if let Some(parent_id) = parent {
                if let Some(Some(p)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    p.children.push(id);
                }
            }

            id
        }

        fn insert_render(&mut self, parent: Option<ElementId>) -> ElementId {
            self.insert(parent, true)
        }

        fn insert_component(&mut self, parent: Option<ElementId>) -> ElementId {
            self.insert(parent, false)
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

    impl RenderTreeAccess for TestTree {
        fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
            if self.get(id)?.is_render {
                Some(&() as &dyn Any)
            } else {
                None
            }
        }

        fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            None
        }

        fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
            self.render_object(id)
        }

        fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            None
        }
    }

    #[test]
    fn test_render_ancestors() {
        let mut tree = TestTree::new();

        // Build: render1 -> component -> render2
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));

        let ancestors: Vec<_> = RenderAncestors::new(&tree, render2).collect();
        assert_eq!(ancestors, vec![render2, render1]);
    }

    #[test]
    fn test_render_descendants() {
        let mut tree = TestTree::new();

        // Build: render1 -> [component -> render2, render3]
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(render1));

        let descendants: Vec<_> = RenderDescendants::new(&tree, render1).collect();
        assert_eq!(descendants, vec![render1, render2, render3]);
    }

    #[test]
    fn test_render_children() {
        let mut tree = TestTree::new();

        // Build: render1 -> [component -> [render2, render3], render4]
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(component));
        let render4 = tree.insert_render(Some(render1));

        let children: Vec<_> = RenderChildren::new(&tree, render1).collect();
        // Should find render2, render3 (through component), and render4
        assert_eq!(children.len(), 3);
        assert!(children.contains(&render2));
        assert!(children.contains(&render3));
        assert!(children.contains(&render4));
    }

    #[test]
    fn test_find_render_ancestor() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));

        assert_eq!(find_render_ancestor(&tree, render2), Some(render1));
        assert_eq!(find_render_ancestor(&tree, render1), None);
    }

    #[test]
    fn test_count_render_elements() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(render1));

        assert_eq!(count_render_elements(&tree, render1), 3);
    }
}

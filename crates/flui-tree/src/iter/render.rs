//! Render-specific iterators.
//!
//! These iterators filter to only render elements, skipping
//! non-render elements like `StatelessView` wrappers.
//!
//! # Iterator Types
//!
//! - [`RenderAncestors`] - Walk up the tree through render elements
//! - [`RenderDescendants`] - Walk down the tree through all render elements
//! - [`RenderChildren`] - Find immediate render children (stops at render boundaries)
//! - [`RenderChildrenWithIndex`] - Like `RenderChildren` but with indices
//! - [`RenderSiblings`] - Iterate over render siblings
//! - [`RenderSubtree`] - BFS traversal of render subtree with depth info
//! - [`RenderLeaves`] - Find leaf render elements (no render children)
//!
//! # Performance Notes
//!
//! All iterators are zero-allocation during iteration (only initial Vec allocation
//! for stack-based iterators). Use `with_capacity` hints when known.

use crate::traits::RenderTreeAccess;
use flui_foundation::ElementId;

// ============================================================================
// RENDER ANCESTORS ITERATOR
// ============================================================================

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
pub struct RenderAncestors<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    current: Option<ElementId>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderAncestors<'a, T> {
    /// Creates a new render ancestors iterator.
    #[inline]
    pub fn new(tree: &'a T, start: ElementId) -> Self {
        Self {
            tree,
            current: Some(start),
        }
    }

    /// Creates iterator starting from parent (skipping start element).
    #[inline]
    pub fn from_parent(tree: &'a T, start: ElementId) -> Self {
        Self {
            tree,
            current: tree.parent(start),
        }
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderAncestors<'a, T> {
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

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderAncestors<'_, T> {}

// ============================================================================
// RENDER DESCENDANTS ITERATOR
// ============================================================================

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
pub struct RenderDescendants<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    stack: Vec<ElementId>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderDescendants<'a, T> {
    /// Creates a new render descendants iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut stack = Vec::with_capacity(16);

        if tree.contains(root) {
            stack.push(root);
        }

        Self { tree, stack }
    }

    /// Creates with custom capacity hint.
    #[inline]
    pub fn with_capacity(tree: &'a T, root: ElementId, capacity: usize) -> Self {
        let mut stack = Vec::with_capacity(capacity);

        if tree.contains(root) {
            stack.push(root);
        }

        Self { tree, stack }
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderDescendants<'a, T> {
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

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderDescendants<'_, T> {}

// ============================================================================
// RENDER CHILDREN ITERATOR
// ============================================================================

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
pub struct RenderChildren<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    stack: Vec<ElementId>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderChildren<'a, T> {
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

    /// Creates with custom capacity hint.
    #[inline]
    pub fn with_capacity(tree: &'a T, parent: ElementId, capacity: usize) -> Self {
        let mut stack = Vec::with_capacity(capacity);

        if tree.contains(parent) {
            for &child in tree.children(parent).iter().rev() {
                stack.push(child);
            }
        }

        Self { tree, stack }
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderChildren<'a, T> {
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

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderChildren<'_, T> {}

// ============================================================================
// RENDER CHILDREN WITH INDEX ITERATOR
// ============================================================================

/// Iterator that yields render children with their index.
///
/// The index is the position among render children (0, 1, 2, ...),
/// not the position in the element tree.
///
/// # Use Case
///
/// Useful for layout algorithms that need child positions (e.g., Flex layout).
#[derive(Debug)]
pub struct RenderChildrenWithIndex<'a, T: RenderTreeAccess + ?Sized> {
    inner: RenderChildren<'a, T>,
    index: usize,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderChildrenWithIndex<'a, T> {
    /// Creates a new indexed render children iterator.
    #[inline]
    pub fn new(tree: &'a T, parent: ElementId) -> Self {
        Self {
            inner: RenderChildren::new(tree, parent),
            index: 0,
        }
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderChildrenWithIndex<'a, T> {
    type Item = (usize, ElementId);

    fn next(&mut self) -> Option<Self::Item> {
        let child = self.inner.next()?;
        let index = self.index;
        self.index += 1;
        Some((index, child))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderChildrenWithIndex<'_, T> {}

// ============================================================================
// RENDER SIBLINGS ITERATOR
// ============================================================================

/// Direction for sibling iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiblingDirection {
    /// Iterate towards earlier siblings (left/previous).
    Previous,
    /// Iterate towards later siblings (right/next).
    Next,
    /// Iterate all siblings (excluding self).
    All,
}

/// Iterator over render siblings.
///
/// Finds other render children of the same render parent.
///
/// # Use Case
///
/// Hit testing often needs to check siblings. Layout algorithms
/// may need to query adjacent elements.
#[derive(Debug)]
pub struct RenderSiblings<'a, T: RenderTreeAccess + ?Sized> {
    _tree: &'a T,
    siblings: Vec<ElementId>,
    self_id: ElementId,
    current_idx: usize,
    direction: SiblingDirection,
    started: bool,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderSiblings<'a, T> {
    /// Creates a new render siblings iterator.
    pub fn new(tree: &'a T, id: ElementId, direction: SiblingDirection) -> Self {
        // Find render parent
        let render_parent = RenderAncestors::from_parent(tree, id).next();

        // Collect all render children of the render parent
        let siblings = if let Some(parent) = render_parent {
            RenderChildren::new(tree, parent).collect()
        } else {
            Vec::new()
        };

        // Find self position in siblings
        let self_idx = siblings.iter().position(|&s| s == id).unwrap_or(0);

        let current_idx = match direction {
            SiblingDirection::Previous => self_idx.saturating_sub(1),
            SiblingDirection::Next | SiblingDirection::All => {
                if self_idx + 1 < siblings.len() {
                    self_idx + 1
                } else {
                    siblings.len()
                }
            }
        };

        Self {
            _tree: tree,
            siblings,
            self_id: id,
            current_idx,
            direction,
            started: false,
        }
    }

    /// Creates iterator for previous siblings only.
    #[inline]
    pub fn previous(tree: &'a T, id: ElementId) -> Self {
        Self::new(tree, id, SiblingDirection::Previous)
    }

    /// Creates iterator for next siblings only.
    #[inline]
    pub fn next_siblings(tree: &'a T, id: ElementId) -> Self {
        Self::new(tree, id, SiblingDirection::Next)
    }

    /// Creates iterator for all siblings.
    #[inline]
    pub fn all(tree: &'a T, id: ElementId) -> Self {
        Self::new(tree, id, SiblingDirection::All)
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderSiblings<'a, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        match self.direction {
            SiblingDirection::Previous => {
                if self.current_idx < self.siblings.len() {
                    let sibling = self.siblings[self.current_idx];
                    if self.current_idx > 0 {
                        self.current_idx -= 1;
                    } else {
                        self.current_idx = self.siblings.len(); // Exhausted
                    }
                    if sibling != self.self_id {
                        return Some(sibling);
                    }
                }
                None
            }
            SiblingDirection::Next => {
                while self.current_idx < self.siblings.len() {
                    let sibling = self.siblings[self.current_idx];
                    self.current_idx += 1;
                    if sibling != self.self_id {
                        return Some(sibling);
                    }
                }
                None
            }
            SiblingDirection::All => {
                while self.current_idx < self.siblings.len() {
                    let sibling = self.siblings[self.current_idx];
                    self.current_idx += 1;
                    if sibling != self.self_id {
                        return Some(sibling);
                    }
                }
                // After exhausting forward, restart from beginning
                if !self.started {
                    self.started = true;
                    self.current_idx = 0;
                    let self_idx = self
                        .siblings
                        .iter()
                        .position(|&s| s == self.self_id)
                        .unwrap_or(0);
                    while self.current_idx < self_idx {
                        let sibling = self.siblings[self.current_idx];
                        self.current_idx += 1;
                        if sibling != self.self_id {
                            return Some(sibling);
                        }
                    }
                }
                None
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.siblings.len().saturating_sub(1); // Minus self
        (0, Some(remaining))
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderSiblings<'_, T> {}

// ============================================================================
// RENDER SUBTREE ITERATOR (BFS with depth)
// ============================================================================

/// Item yielded by RenderSubtree iterator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSubtreeItem {
    /// The element ID.
    pub id: ElementId,
    /// Depth relative to root (root = 0).
    pub depth: usize,
}

/// BFS iterator over render subtree with depth information.
///
/// Unlike `RenderDescendants` (DFS), this uses breadth-first traversal
/// and provides depth information.
///
/// # Use Case
///
/// Useful for level-order operations, accessibility tree building,
/// or when you need to process parents before children.
#[derive(Debug)]
pub struct RenderSubtree<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    queue: std::collections::VecDeque<(ElementId, usize)>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderSubtree<'a, T> {
    /// Creates a new BFS render subtree iterator.
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut queue = std::collections::VecDeque::with_capacity(16);

        if tree.contains(root) && tree.is_render_element(root) {
            queue.push_back((root, 0));
        }

        Self { tree, queue }
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderSubtree<'a, T> {
    type Item = RenderSubtreeItem;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((current, depth)) = self.queue.pop_front() {
            if !self.tree.contains(current) {
                continue;
            }

            // Queue render children at next depth
            for child in RenderChildren::new(self.tree, current) {
                self.queue.push_back((child, depth + 1));
            }

            return Some(RenderSubtreeItem { id: current, depth });
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.queue.len(), None)
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderSubtree<'_, T> {}

// ============================================================================
// RENDER LEAVES ITERATOR
// ============================================================================

/// Iterator that finds leaf render elements (elements with no render children).
///
/// # Use Case
///
/// Bottom-up layout algorithms start from leaves. Also useful for finding
/// text nodes or other terminal render elements.
#[derive(Debug)]
pub struct RenderLeaves<'a, T: RenderTreeAccess + ?Sized> {
    inner: RenderDescendants<'a, T>,
    tree: &'a T,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderLeaves<'a, T> {
    /// Creates a new render leaves iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        Self {
            inner: RenderDescendants::new(tree, root),
            tree,
        }
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderLeaves<'a, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = self.inner.next()?;

            // Check if this render element has any render children
            if RenderChildren::new(self.tree, id).next().is_none() {
                return Some(id);
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderLeaves<'_, T> {}

// ============================================================================
// RENDER PATH ITERATOR
// ============================================================================

/// Iterator that yields the path from root to a target element.
///
/// The path consists of all render elements between root and target,
/// yielded in root-to-target order.
///
/// # Use Case
///
/// Hit testing needs to know the full path from root to hit element.
/// Useful for focus management and accessibility.
#[derive(Debug)]
pub struct RenderPath<'a, T: RenderTreeAccess + ?Sized> {
    _tree: &'a T,
    path: Vec<ElementId>,
    current_idx: usize,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderPath<'a, T> {
    /// Creates a new render path iterator from root to target.
    pub fn new(tree: &'a T, target: ElementId) -> Self {
        // Collect path from target to root
        let path: Vec<_> = RenderAncestors::new(tree, target).collect();
        // Reverse to get root-to-target order
        let path: Vec<_> = path.into_iter().rev().collect();

        Self {
            _tree: tree,
            path,
            current_idx: 0,
        }
    }

    /// Returns the path length.
    #[inline]
    pub fn len(&self) -> usize {
        self.path.len()
    }

    /// Returns true if the path is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// Returns the path as a slice.
    #[inline]
    pub fn as_slice(&self) -> &[ElementId] {
        &self.path
    }
}

impl<'a, T: RenderTreeAccess + ?Sized> Iterator for RenderPath<'a, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx < self.path.len() {
            let id = self.path[self.current_idx];
            self.current_idx += 1;
            Some(id)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.path.len() - self.current_idx;
        (remaining, Some(remaining))
    }
}

impl<T: RenderTreeAccess + ?Sized> ExactSizeIterator for RenderPath<'_, T> {}
impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderPath<'_, T> {}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Finds the nearest render ancestor of an element.
///
/// Convenience function wrapping `RenderAncestors`.
#[inline]
pub fn find_render_ancestor<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    id: ElementId,
) -> Option<ElementId> {
    RenderAncestors::from_parent(tree, id).next()
}

/// Finds the render parent of an element.
///
/// Same as `find_render_ancestor` but more semantic.
#[inline]
pub fn render_parent<T: RenderTreeAccess + ?Sized>(tree: &T, id: ElementId) -> Option<ElementId> {
    find_render_ancestor(tree, id)
}

/// Collects all render children of a render element.
#[inline]
pub fn collect_render_children<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
) -> Vec<ElementId> {
    RenderChildren::new(tree, parent).collect()
}

/// Counts render elements in a subtree.
#[inline]
pub fn count_render_elements<T: RenderTreeAccess + ?Sized>(tree: &T, root: ElementId) -> usize {
    RenderDescendants::new(tree, root).count()
}

/// Counts render children of an element.
#[inline]
pub fn count_render_children<T: RenderTreeAccess + ?Sized>(tree: &T, parent: ElementId) -> usize {
    RenderChildren::new(tree, parent).count()
}

/// Finds the first render child of an element.
#[inline]
pub fn first_render_child<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
) -> Option<ElementId> {
    RenderChildren::new(tree, parent).next()
}

/// Finds the last render child of an element.
#[inline]
pub fn last_render_child<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
) -> Option<ElementId> {
    RenderChildren::new(tree, parent).last()
}

/// Finds the nth render child of an element.
#[inline]
pub fn nth_render_child<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
    n: usize,
) -> Option<ElementId> {
    RenderChildren::new(tree, parent).nth(n)
}

/// Checks if an element has any render children.
#[inline]
pub fn has_render_children<T: RenderTreeAccess + ?Sized>(tree: &T, parent: ElementId) -> bool {
    RenderChildren::new(tree, parent).next().is_some()
}

/// Checks if element is a render leaf (no render children).
#[inline]
pub fn is_render_leaf<T: RenderTreeAccess + ?Sized>(tree: &T, id: ElementId) -> bool {
    tree.is_render_element(id) && !has_render_children(tree, id)
}

/// Finds the render root (topmost render ancestor).
#[inline]
pub fn find_render_root<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    id: ElementId,
) -> Option<ElementId> {
    RenderAncestors::new(tree, id).last()
}

/// Calculates render depth (number of render ancestors including self).
#[inline]
pub fn render_depth<T: RenderTreeAccess + ?Sized>(tree: &T, id: ElementId) -> usize {
    RenderAncestors::new(tree, id).count()
}

/// Checks if `descendant` is a render descendant of `ancestor`.
#[inline]
pub fn is_render_descendant<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    descendant: ElementId,
    ancestor: ElementId,
) -> bool {
    RenderAncestors::new(tree, descendant).any(|id| id == ancestor)
}

/// Finds the lowest common render ancestor of two elements.
pub fn lowest_common_render_ancestor<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    a: ElementId,
    b: ElementId,
) -> Option<ElementId> {
    // Collect ancestors of `a` into a set
    let a_ancestors: std::collections::HashSet<_> = RenderAncestors::new(tree, a).collect();

    // Find first ancestor of `b` that's also an ancestor of `a`
    RenderAncestors::new(tree, b).find(|&id| a_ancestors.contains(&id))
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
            let id = ElementId::new(self.nodes.len() + 1);
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
    fn test_render_ancestors_from_parent() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));

        // Should skip render2 and start from parent
        let ancestors: Vec<_> = RenderAncestors::from_parent(&tree, render2).collect();
        assert_eq!(ancestors, vec![render1]);
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
    fn test_render_children_with_index() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));

        let indexed: Vec<_> = RenderChildrenWithIndex::new(&tree, render1).collect();
        assert_eq!(indexed.len(), 2);
        assert_eq!(indexed[0].0, 0);
        assert_eq!(indexed[1].0, 1);
    }

    #[test]
    fn test_render_subtree_bfs() {
        let mut tree = TestTree::new();

        // Build: render1 -> [render2 -> render4, render3]
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));
        let render4 = tree.insert_render(Some(render2));

        let items: Vec<_> = RenderSubtree::new(&tree, render1).collect();

        // BFS order: render1 (depth 0), render2 (depth 1), render3 (depth 1), render4 (depth 2)
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[0],
            RenderSubtreeItem {
                id: render1,
                depth: 0
            }
        );
        assert_eq!(items[1].depth, 1);
        assert_eq!(items[2].depth, 1);
        assert_eq!(items[3].depth, 2);
    }

    #[test]
    fn test_render_leaves() {
        let mut tree = TestTree::new();

        // Build: render1 -> [render2 -> render4, render3]
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));
        let render4 = tree.insert_render(Some(render2));

        let leaves: Vec<_> = RenderLeaves::new(&tree, render1).collect();

        // render3 and render4 are leaves
        assert_eq!(leaves.len(), 2);
        assert!(leaves.contains(&render3));
        assert!(leaves.contains(&render4));
    }

    #[test]
    fn test_render_path() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(render2));

        let path = RenderPath::new(&tree, render3);

        assert_eq!(path.len(), 3);
        let path_vec: Vec<_> = path.collect();
        assert_eq!(path_vec, vec![render1, render2, render3]);
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

    #[test]
    fn test_is_render_leaf() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));

        assert!(!is_render_leaf(&tree, render1));
        assert!(is_render_leaf(&tree, render2));
    }

    #[test]
    fn test_lowest_common_render_ancestor() {
        let mut tree = TestTree::new();

        // Build: render1 -> [render2 -> render4, render3 -> render5]
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));
        let render4 = tree.insert_render(Some(render2));
        let render5 = tree.insert_render(Some(render3));

        assert_eq!(
            lowest_common_render_ancestor(&tree, render4, render5),
            Some(render1)
        );
        assert_eq!(
            lowest_common_render_ancestor(&tree, render4, render2),
            Some(render2)
        );
    }
}

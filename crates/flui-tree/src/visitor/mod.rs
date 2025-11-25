//! Visitor pattern for tree traversal.
//!
//! This module provides the visitor pattern for tree operations,
//! allowing complex traversal logic with early termination support.
//!
//! # When to Use
//!
//! Use visitors when:
//! - You need to perform complex operations during traversal
//! - You want early termination (stop, skip subtree)
//! - You need pre/post visit hooks
//! - You want reusable traversal logic
//!
//! Use iterators when:
//! - Simple traversal/collection is sufficient
//! - You want to compose with iterator adaptors
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeVisitor, VisitorResult, visit_depth_first};
//!
//! struct FindById {
//!     target: ElementId,
//!     found: Option<ElementId>,
//! }
//!
//! impl TreeVisitor for FindById {
//!     fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
//!         if id == self.target {
//!             self.found = Some(id);
//!             VisitorResult::Stop
//!         } else {
//!             VisitorResult::Continue
//!         }
//!     }
//! }
//!
//! let mut visitor = FindById { target: some_id, found: None };
//! visit_depth_first(&tree, root, &mut visitor);
//! ```

use crate::traits::TreeNav;
use flui_foundation::ElementId;

/// Result of visiting a node.
///
/// Controls traversal flow after visiting each node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisitorResult {
    /// Continue normal traversal.
    Continue,

    /// Skip this node's children (but continue to siblings).
    SkipChildren,

    /// Stop traversal completely.
    Stop,
}

impl Default for VisitorResult {
    fn default() -> Self {
        Self::Continue
    }
}

impl VisitorResult {
    /// Returns `true` if traversal should continue.
    #[inline]
    pub fn should_continue(&self) -> bool {
        matches!(self, Self::Continue | Self::SkipChildren)
    }

    /// Returns `true` if children should be visited.
    #[inline]
    pub fn should_visit_children(&self) -> bool {
        matches!(self, Self::Continue)
    }

    /// Returns `true` if traversal should stop completely.
    #[inline]
    pub fn should_stop(&self) -> bool {
        matches!(self, Self::Stop)
    }
}

/// Visitor trait for tree traversal.
///
/// Implement this trait to perform custom operations during tree traversal.
/// The visitor is called for each node with its ID and depth.
pub trait TreeVisitor {
    /// Called when visiting a node.
    ///
    /// # Arguments
    ///
    /// * `id` - The element being visited
    /// * `depth` - The depth of the element (root = 0)
    ///
    /// # Returns
    ///
    /// A [`VisitorResult`] controlling further traversal.
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult;

    /// Called before visiting a node's children.
    ///
    /// This is only called if `visit` returned `Continue`.
    /// Default implementation does nothing.
    ///
    /// # Arguments
    ///
    /// * `id` - The parent element
    /// * `depth` - The depth of the parent
    #[inline]
    fn pre_children(&mut self, _id: ElementId, _depth: usize) {}

    /// Called after visiting all of a node's children.
    ///
    /// This is only called if `visit` returned `Continue`.
    /// Default implementation does nothing.
    ///
    /// # Arguments
    ///
    /// * `id` - The parent element
    /// * `depth` - The depth of the parent
    #[inline]
    fn post_children(&mut self, _id: ElementId, _depth: usize) {}
}

/// Mutable visitor trait for tree traversal with node access.
///
/// Like [`TreeVisitor`] but provides access to the actual node data.
pub trait TreeVisitorMut<T: TreeNav> {
    /// Called when visiting a node.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree being traversed
    /// * `id` - The element being visited
    /// * `depth` - The depth of the element
    fn visit(&mut self, tree: &T, id: ElementId, depth: usize) -> VisitorResult;

    /// Called before visiting a node's children.
    #[inline]
    fn pre_children(&mut self, _tree: &T, _id: ElementId, _depth: usize) {}

    /// Called after visiting all of a node's children.
    #[inline]
    fn post_children(&mut self, _tree: &T, _id: ElementId, _depth: usize) {}
}

// ============================================================================
// TRAVERSAL FUNCTIONS
// ============================================================================

/// Performs depth-first traversal with a visitor.
///
/// Visits nodes in pre-order (parent before children).
///
/// # Arguments
///
/// * `tree` - The tree to traverse
/// * `root` - The root of the subtree to traverse
/// * `visitor` - The visitor to call for each node
///
/// # Returns
///
/// `true` if traversal completed, `false` if stopped early.
pub fn visit_depth_first<T, V>(tree: &T, root: ElementId, visitor: &mut V) -> bool
where
    T: TreeNav,
    V: TreeVisitor,
{
    visit_depth_first_impl(tree, root, 0, visitor)
}

fn visit_depth_first_impl<T, V>(tree: &T, id: ElementId, depth: usize, visitor: &mut V) -> bool
where
    T: TreeNav,
    V: TreeVisitor,
{
    if !tree.contains(id) {
        return true;
    }

    match visitor.visit(id, depth) {
        VisitorResult::Stop => return false,
        VisitorResult::SkipChildren => return true,
        VisitorResult::Continue => {}
    }

    visitor.pre_children(id, depth);

    for &child in tree.children(id) {
        if !visit_depth_first_impl(tree, child, depth + 1, visitor) {
            return false;
        }
    }

    visitor.post_children(id, depth);

    true
}

/// Performs depth-first traversal with a mutable visitor.
pub fn visit_depth_first_mut<T, V>(tree: &T, root: ElementId, visitor: &mut V) -> bool
where
    T: TreeNav,
    V: TreeVisitorMut<T>,
{
    visit_depth_first_mut_impl(tree, root, 0, visitor)
}

fn visit_depth_first_mut_impl<T, V>(tree: &T, id: ElementId, depth: usize, visitor: &mut V) -> bool
where
    T: TreeNav,
    V: TreeVisitorMut<T>,
{
    if !tree.contains(id) {
        return true;
    }

    match visitor.visit(tree, id, depth) {
        VisitorResult::Stop => return false,
        VisitorResult::SkipChildren => return true,
        VisitorResult::Continue => {}
    }

    visitor.pre_children(tree, id, depth);

    // Collect children to avoid borrow issues
    let children: Vec<_> = tree.children(id).to_vec();

    for child in children {
        if !visit_depth_first_mut_impl(tree, child, depth + 1, visitor) {
            return false;
        }
    }

    visitor.post_children(tree, id, depth);

    true
}

/// Performs breadth-first traversal with a visitor.
///
/// Visits nodes level by level (all depth 0, then depth 1, etc.).
///
/// # Arguments
///
/// * `tree` - The tree to traverse
/// * `root` - The root of the subtree to traverse
/// * `visitor` - The visitor to call for each node
///
/// # Returns
///
/// `true` if traversal completed, `false` if stopped early.
pub fn visit_breadth_first<T, V>(tree: &T, root: ElementId, visitor: &mut V) -> bool
where
    T: TreeNav,
    V: TreeVisitor,
{
    use std::collections::VecDeque;

    if !tree.contains(root) {
        return true;
    }

    let mut queue = VecDeque::new();
    queue.push_back((root, 0usize));

    while let Some((id, depth)) = queue.pop_front() {
        if !tree.contains(id) {
            continue;
        }

        match visitor.visit(id, depth) {
            VisitorResult::Stop => return false,
            VisitorResult::SkipChildren => continue,
            VisitorResult::Continue => {}
        }

        for &child in tree.children(id) {
            queue.push_back((child, depth + 1));
        }
    }

    true
}

// ============================================================================
// COMMON VISITORS
// ============================================================================

/// Visitor that collects all visited element IDs.
#[derive(Debug, Default)]
pub struct CollectVisitor {
    /// Collected element IDs.
    pub collected: Vec<ElementId>,
}

impl CollectVisitor {
    /// Creates a new collector.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a collector with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            collected: Vec::with_capacity(capacity),
        }
    }

    /// Returns the collected IDs.
    #[inline]
    pub fn into_inner(self) -> Vec<ElementId> {
        self.collected
    }
}

impl TreeVisitor for CollectVisitor {
    fn visit(&mut self, id: ElementId, _depth: usize) -> VisitorResult {
        self.collected.push(id);
        VisitorResult::Continue
    }
}

/// Visitor that counts nodes.
#[derive(Debug, Default)]
pub struct CountVisitor {
    /// The count of visited nodes.
    pub count: usize,
}

impl CountVisitor {
    /// Creates a new counter.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}

impl TreeVisitor for CountVisitor {
    fn visit(&mut self, _id: ElementId, _depth: usize) -> VisitorResult {
        self.count += 1;
        VisitorResult::Continue
    }
}

/// Visitor that finds the maximum depth.
#[derive(Debug, Default)]
pub struct MaxDepthVisitor {
    /// The maximum depth encountered.
    pub max_depth: usize,
}

impl MaxDepthVisitor {
    /// Creates a new max depth finder.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}

impl TreeVisitor for MaxDepthVisitor {
    fn visit(&mut self, _id: ElementId, depth: usize) -> VisitorResult {
        if depth > self.max_depth {
            self.max_depth = depth;
        }
        VisitorResult::Continue
    }
}

/// Visitor that finds elements matching a predicate.
#[derive(Debug)]
pub struct FindVisitor<F> {
    predicate: F,
    /// Found element, if any.
    pub found: Option<ElementId>,
}

impl<F> FindVisitor<F>
where
    F: FnMut(ElementId, usize) -> bool,
{
    /// Creates a new finder with the given predicate.
    #[inline]
    pub fn new(predicate: F) -> Self {
        Self {
            predicate,
            found: None,
        }
    }
}

impl<F> TreeVisitor for FindVisitor<F>
where
    F: FnMut(ElementId, usize) -> bool,
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        if (self.predicate)(id, depth) {
            self.found = Some(id);
            VisitorResult::Stop
        } else {
            VisitorResult::Continue
        }
    }
}

/// Visitor that calls a closure for each node.
#[derive(Debug)]
pub struct ForEachVisitor<F> {
    callback: F,
}

impl<F> ForEachVisitor<F>
where
    F: FnMut(ElementId, usize),
{
    /// Creates a new for-each visitor.
    #[inline]
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> TreeVisitor for ForEachVisitor<F>
where
    F: FnMut(ElementId, usize),
{
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        (self.callback)(id, depth);
        VisitorResult::Continue
    }
}

// ============================================================================
// CONVENIENCE FUNCTIONS
// ============================================================================

/// Collects all descendants into a vector.
#[inline]
pub fn collect_all<T: TreeNav>(tree: &T, root: ElementId) -> Vec<ElementId> {
    let mut visitor = CollectVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.into_inner()
}

/// Counts all nodes in a subtree.
#[inline]
pub fn count_all<T: TreeNav>(tree: &T, root: ElementId) -> usize {
    let mut visitor = CountVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.count
}

/// Finds the maximum depth in a subtree.
#[inline]
pub fn max_depth<T: TreeNav>(tree: &T, root: ElementId) -> usize {
    let mut visitor = MaxDepthVisitor::new();
    visit_depth_first(tree, root, &mut visitor);
    visitor.max_depth
}

/// Finds the first element matching a predicate.
#[inline]
pub fn find_first<T, F>(tree: &T, root: ElementId, predicate: F) -> Option<ElementId>
where
    T: TreeNav,
    F: FnMut(ElementId, usize) -> bool,
{
    let mut visitor = FindVisitor::new(predicate);
    visit_depth_first(tree, root, &mut visitor);
    visitor.found
}

/// Calls a closure for each node in a subtree.
#[inline]
pub fn for_each<T, F>(tree: &T, root: ElementId, callback: F)
where
    T: TreeNav,
    F: FnMut(ElementId, usize),
{
    let mut visitor = ForEachVisitor::new(callback);
    visit_depth_first(tree, root, &mut visitor);
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
    fn test_visit_depth_first() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child1 = tree.insert(Some(root));
        let child2 = tree.insert(Some(root));
        let grandchild = tree.insert(Some(child1));

        let mut collector = CollectVisitor::new();
        visit_depth_first(&tree, root, &mut collector);

        assert_eq!(collector.collected, vec![root, child1, grandchild, child2]);
    }

    #[test]
    fn test_visit_breadth_first() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child1 = tree.insert(Some(root));
        let child2 = tree.insert(Some(root));
        let grandchild = tree.insert(Some(child1));

        let mut collector = CollectVisitor::new();
        visit_breadth_first(&tree, root, &mut collector);

        assert_eq!(collector.collected, vec![root, child1, child2, grandchild]);
    }

    #[test]
    fn test_early_termination() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child1 = tree.insert(Some(root));
        let _child2 = tree.insert(Some(root));

        let found = find_first(&tree, root, |id, _| id == child1);
        assert_eq!(found, Some(child1));
    }

    #[test]
    fn test_skip_children() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child1 = tree.insert(Some(root));
        let _grandchild = tree.insert(Some(child1));
        let child2 = tree.insert(Some(root));

        struct SkipChildrenVisitor {
            skip_id: ElementId,
            visited: Vec<ElementId>,
        }

        impl TreeVisitor for SkipChildrenVisitor {
            fn visit(&mut self, id: ElementId, _depth: usize) -> VisitorResult {
                self.visited.push(id);
                if id == self.skip_id {
                    VisitorResult::SkipChildren
                } else {
                    VisitorResult::Continue
                }
            }
        }

        let mut visitor = SkipChildrenVisitor {
            skip_id: child1,
            visited: Vec::new(),
        };

        visit_depth_first(&tree, root, &mut visitor);

        // Should skip grandchild (child of child1)
        assert_eq!(visitor.visited, vec![root, child1, child2]);
    }

    #[test]
    fn test_count_all() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        tree.insert(Some(root));
        tree.insert(Some(root));

        assert_eq!(count_all(&tree, root), 3);
    }

    #[test]
    fn test_max_depth() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        let child = tree.insert(Some(root));
        let grandchild = tree.insert(Some(child));
        tree.insert(Some(grandchild));

        assert_eq!(max_depth(&tree, root), 3);
    }

    #[test]
    fn test_for_each() {
        let mut tree = TestTree::new();
        let root = tree.insert(None);
        tree.insert(Some(root));
        tree.insert(Some(root));

        let mut count = 0;
        for_each(&tree, root, |_, _| count += 1);

        assert_eq!(count, 3);
    }
}

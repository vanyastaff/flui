//! Zero-copy tree views for efficient subtree access.
//!
//! This module provides view types that borrow from an existing tree
//! and present a subset of nodes as a virtual tree. This is useful for:
//!
//! - Operating on subtrees without copying data
//! - Filtering trees based on predicates
//! - Creating bounded views with depth limits
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{SubtreeView, TreeNav, TreeRead};
//!
//! fn process_subtree<T: TreeNav>(tree: &T, subtree_root: ElementId) {
//!     // Create a view that only sees the subtree
//!     let view = SubtreeView::new(tree, subtree_root);
//!
//!     // The view implements TreeNav, so standard operations work
//!     for id in view.node_ids() {
//!         println!("Node in subtree: {:?}", id);
//!     }
//! }
//! ```

use crate::TreeNav;
use flui_foundation::ElementId;
use std::collections::HashSet;

// ============================================================================
// SUBTREE VIEW
// ============================================================================

/// A zero-copy view of a subtree.
///
/// This struct borrows from an existing tree and presents only the nodes
/// in a specific subtree as if they were the entire tree.
///
/// The view's root becomes the "virtual root" of the viewed tree.
pub struct SubtreeView<'a, T: TreeNav> {
    tree: &'a T,
    root: ElementId,
}

impl<'a, T: TreeNav> SubtreeView<'a, T> {
    /// Create a new subtree view.
    ///
    /// The `root` element becomes the root of the viewed subtree.
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        Self { tree, root }
    }

    /// Get the root of this subtree view.
    #[inline]
    pub fn root(&self) -> ElementId {
        self.root
    }

    /// Get a reference to the underlying tree.
    #[inline]
    pub fn underlying_tree(&self) -> &'a T {
        self.tree
    }

    /// Check if an element is within this subtree.
    pub fn contains_in_subtree(&self, id: ElementId) -> bool {
        if id == self.root {
            return true;
        }

        // Walk up the ancestors to see if we reach the root
        let mut current = self.tree.parent(id);
        while let Some(parent) = current {
            if parent == self.root {
                return true;
            }
            current = self.tree.parent(parent);
        }

        false
    }

    /// Get the depth of an element relative to the subtree root.
    pub fn relative_depth(&self, id: ElementId) -> Option<usize> {
        if id == self.root {
            return Some(0);
        }

        let mut depth = 0;
        let mut current = Some(id);

        while let Some(c) = current {
            if c == self.root {
                return Some(depth);
            }
            depth += 1;
            current = self.tree.parent(c);
        }

        None // Not in subtree
    }
}

impl<'a, T: TreeNav> std::fmt::Debug for SubtreeView<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubtreeView")
            .field("root", &self.root)
            .finish()
    }
}

// ============================================================================
// DEPTH-LIMITED VIEW
// ============================================================================

/// A view that limits the visible depth of a tree.
///
/// Nodes deeper than `max_depth` from the root are not visible.
pub struct DepthLimitedView<'a, T: TreeNav> {
    tree: &'a T,
    root: ElementId,
    max_depth: usize,
}

impl<'a, T: TreeNav> DepthLimitedView<'a, T> {
    /// Create a new depth-limited view.
    ///
    /// Nodes with depth > `max_depth` relative to `root` are not visible.
    pub fn new(tree: &'a T, root: ElementId, max_depth: usize) -> Self {
        Self {
            tree,
            root,
            max_depth,
        }
    }

    /// Get the root element.
    #[inline]
    pub fn root(&self) -> ElementId {
        self.root
    }

    /// Get the maximum depth limit.
    #[inline]
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Check if an element is within the depth limit.
    pub fn is_within_depth(&self, id: ElementId) -> bool {
        if let Some(depth) = self.depth_of(id) {
            depth <= self.max_depth
        } else {
            false
        }
    }

    /// Get the depth of an element relative to root.
    fn depth_of(&self, id: ElementId) -> Option<usize> {
        if id == self.root {
            return Some(0);
        }

        let mut depth = 0;
        let mut current = Some(id);

        while let Some(c) = current {
            if c == self.root {
                return Some(depth);
            }
            depth += 1;
            current = self.tree.parent(c);
        }

        None
    }

    /// Get visible children (those within depth limit).
    pub fn visible_children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + 'a {
        let depth = self.depth_of(id).unwrap_or(0);
        let max_depth = self.max_depth;
        let children: Vec<_> = self.tree.children(id).collect();

        children.into_iter().filter(move |_| depth < max_depth)
    }
}

impl<'a, T: TreeNav> std::fmt::Debug for DepthLimitedView<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepthLimitedView")
            .field("root", &self.root)
            .field("max_depth", &self.max_depth)
            .finish()
    }
}

// ============================================================================
// FILTERED VIEW
// ============================================================================

/// A view that shows only nodes matching a predicate.
///
/// Note: This is a lazy filter - membership is checked on access.
pub struct FilteredView<'a, T: TreeNav, P> {
    tree: &'a T,
    root: ElementId,
    predicate: P,
}

impl<'a, T: TreeNav, P> FilteredView<'a, T, P>
where
    P: Fn(ElementId) -> bool,
{
    /// Create a new filtered view.
    pub fn new(tree: &'a T, root: ElementId, predicate: P) -> Self {
        Self {
            tree,
            root,
            predicate,
        }
    }

    /// Get the root element.
    #[inline]
    pub fn root(&self) -> ElementId {
        self.root
    }

    /// Check if an element passes the filter.
    #[inline]
    pub fn passes_filter(&self, id: ElementId) -> bool {
        (self.predicate)(id)
    }

    /// Get filtered children.
    pub fn filtered_children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
        self.tree
            .children(id)
            .filter(|&child| (self.predicate)(child))
    }

    /// Collect all nodes that pass the filter within the subtree.
    pub fn collect_matching(&self) -> Vec<ElementId> {
        let mut result = Vec::new();
        self.collect_matching_impl(self.root, &mut result);
        result
    }

    fn collect_matching_impl(&self, id: ElementId, result: &mut Vec<ElementId>) {
        if (self.predicate)(id) {
            result.push(id);
        }

        for child in self.tree.children(id) {
            self.collect_matching_impl(child, result);
        }
    }
}

impl<'a, T: TreeNav, P> std::fmt::Debug for FilteredView<'a, T, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilteredView")
            .field("root", &self.root)
            .finish()
    }
}

// ============================================================================
// SNAPSHOT VIEW
// ============================================================================

/// A snapshot of element IDs from a tree at a point in time.
///
/// Unlike other views, this owns the data and doesn't borrow the tree.
/// Useful for comparing tree states across mutations.
#[derive(Debug, Clone)]
pub struct TreeSnapshot {
    /// All element IDs in the snapshot.
    pub nodes: HashSet<ElementId>,

    /// Parent relationships.
    pub parents: std::collections::HashMap<ElementId, Option<ElementId>>,

    /// Child relationships.
    pub children: std::collections::HashMap<ElementId, Vec<ElementId>>,

    /// The root element.
    pub root: Option<ElementId>,
}

impl TreeSnapshot {
    /// Create an empty snapshot.
    pub fn new() -> Self {
        Self {
            nodes: HashSet::new(),
            parents: std::collections::HashMap::new(),
            children: std::collections::HashMap::new(),
            root: None,
        }
    }

    /// Create a snapshot from a tree.
    pub fn from_tree<T: TreeNav>(tree: &T, root: ElementId) -> Self {
        let mut snapshot = Self::new();
        snapshot.root = Some(root);
        snapshot.capture_subtree(tree, root);
        snapshot
    }

    fn capture_subtree<T: TreeNav>(&mut self, tree: &T, id: ElementId) {
        self.nodes.insert(id);
        self.parents.insert(id, tree.parent(id));

        let children: Vec<ElementId> = tree.children(id).collect();
        self.children.insert(id, children.clone());

        for child in children {
            self.capture_subtree(tree, child);
        }
    }

    /// Check if the snapshot contains an element.
    #[inline]
    pub fn contains(&self, id: ElementId) -> bool {
        self.nodes.contains(&id)
    }

    /// Get the number of nodes in the snapshot.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if snapshot is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the parent of an element in the snapshot.
    pub fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.parents.get(&id).copied().flatten()
    }

    /// Get children of an element in the snapshot.
    pub fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
        self.children
            .get(&id)
            .map(|v| v.iter().copied())
            .into_iter()
            .flatten()
    }

    /// Compare with another snapshot.
    pub fn diff(&self, other: &TreeSnapshot) -> SnapshotDiff {
        let added: Vec<_> = other.nodes.difference(&self.nodes).copied().collect();
        let removed: Vec<_> = self.nodes.difference(&other.nodes).copied().collect();

        SnapshotDiff { added, removed }
    }
}

impl Default for TreeSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Difference between two snapshots.
#[derive(Debug, Clone, Default)]
pub struct SnapshotDiff {
    /// Elements added in the new snapshot.
    pub added: Vec<ElementId>,
    /// Elements removed from the old snapshot.
    pub removed: Vec<ElementId>,
}

impl SnapshotDiff {
    /// Check if there are any differences.
    #[inline]
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }

    /// Check if snapshots are identical.
    #[inline]
    pub fn is_identical(&self) -> bool {
        !self.has_changes()
    }
}

// ============================================================================
// ANCESTOR VIEW
// ============================================================================

/// A view showing the path from a node to the root.
///
/// This is useful for displaying breadcrumbs or understanding
/// the hierarchical position of a node.
pub struct AncestorView<'a, T: TreeNav> {
    tree: &'a T,
    target: ElementId,
    path: Vec<ElementId>,
}

impl<'a, T: TreeNav> AncestorView<'a, T> {
    /// Create a new ancestor view from target to root.
    pub fn new(tree: &'a T, target: ElementId) -> Self {
        let mut path = Vec::new();
        let mut current = Some(target);

        while let Some(id) = current {
            path.push(id);
            current = tree.parent(id);
        }

        // Path is target -> ... -> root, reverse for root -> ... -> target
        path.reverse();

        Self { tree, target, path }
    }

    /// Get the target element.
    #[inline]
    pub fn target(&self) -> ElementId {
        self.target
    }

    /// Get the path from root to target.
    pub fn path(&self) -> &[ElementId] {
        &self.path
    }

    /// Get the path length (depth of target).
    #[inline]
    pub fn depth(&self) -> usize {
        self.path.len().saturating_sub(1)
    }

    /// Get the root element.
    pub fn root(&self) -> Option<ElementId> {
        self.path.first().copied()
    }

    /// Check if an element is an ancestor of the target.
    pub fn is_ancestor(&self, id: ElementId) -> bool {
        self.path.contains(&id) && id != self.target
    }

    /// Get the ancestor at a specific depth.
    pub fn ancestor_at_depth(&self, depth: usize) -> Option<ElementId> {
        self.path.get(depth).copied()
    }
}

impl<'a, T: TreeNav> std::fmt::Debug for AncestorView<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AncestorView")
            .field("target", &self.target)
            .field("path", &self.path)
            .finish()
    }
}

// ============================================================================
// SIBLING VIEW
// ============================================================================

/// A view of siblings (nodes with the same parent).
pub struct SiblingView<'a, T: TreeNav> {
    tree: &'a T,
    element: ElementId,
    siblings: Vec<ElementId>,
    index: usize,
}

impl<'a, T: TreeNav> SiblingView<'a, T> {
    /// Create a new sibling view.
    pub fn new(tree: &'a T, element: ElementId) -> Self {
        let (siblings, index) = if let Some(parent) = tree.parent(element) {
            let sibs: Vec<_> = tree.children(parent).collect();
            let idx = sibs.iter().position(|&id| id == element).unwrap_or(0);
            (sibs, idx)
        } else {
            // Root has no siblings
            (vec![element], 0)
        };

        Self {
            tree,
            element,
            siblings,
            index,
        }
    }

    /// Get the element this view is centered on.
    #[inline]
    pub fn element(&self) -> ElementId {
        self.element
    }

    /// Get all siblings (including self).
    pub fn all(&self) -> &[ElementId] {
        &self.siblings
    }

    /// Get the index of this element among siblings.
    #[inline]
    pub fn index(&self) -> usize {
        self.index
    }

    /// Get the number of siblings (including self).
    #[inline]
    pub fn count(&self) -> usize {
        self.siblings.len()
    }

    /// Check if this is the first sibling.
    #[inline]
    pub fn is_first(&self) -> bool {
        self.index == 0
    }

    /// Check if this is the last sibling.
    #[inline]
    pub fn is_last(&self) -> bool {
        self.index == self.siblings.len().saturating_sub(1)
    }

    /// Get the previous sibling.
    pub fn previous(&self) -> Option<ElementId> {
        if self.index > 0 {
            Some(self.siblings[self.index - 1])
        } else {
            None
        }
    }

    /// Get the next sibling.
    pub fn next(&self) -> Option<ElementId> {
        self.siblings.get(self.index + 1).copied()
    }

    /// Get the first sibling.
    pub fn first(&self) -> Option<ElementId> {
        self.siblings.first().copied()
    }

    /// Get the last sibling.
    pub fn last(&self) -> Option<ElementId> {
        self.siblings.last().copied()
    }

    /// Get siblings before this element.
    pub fn before(&self) -> &[ElementId] {
        &self.siblings[..self.index]
    }

    /// Get siblings after this element.
    pub fn after(&self) -> &[ElementId] {
        if self.index + 1 < self.siblings.len() {
            &self.siblings[self.index + 1..]
        } else {
            &[]
        }
    }
}

impl<'a, T: TreeNav> std::fmt::Debug for SiblingView<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SiblingView")
            .field("element", &self.element)
            .field("index", &self.index)
            .field("count", &self.siblings.len())
            .finish()
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for creating views from trees.
pub trait TreeViewExt: TreeNav {
    /// Create a subtree view rooted at the given element.
    fn subtree_view(&self, root: ElementId) -> SubtreeView<'_, Self>
    where
        Self: Sized,
    {
        SubtreeView::new(self, root)
    }

    /// Create a depth-limited view.
    fn depth_limited_view(&self, root: ElementId, max_depth: usize) -> DepthLimitedView<'_, Self>
    where
        Self: Sized,
    {
        DepthLimitedView::new(self, root, max_depth)
    }

    /// Create a filtered view.
    fn filtered_view<P>(&self, root: ElementId, predicate: P) -> FilteredView<'_, Self, P>
    where
        Self: Sized,
        P: Fn(ElementId) -> bool,
    {
        FilteredView::new(self, root, predicate)
    }

    /// Create a snapshot of the tree.
    fn snapshot(&self, root: ElementId) -> TreeSnapshot
    where
        Self: Sized,
    {
        TreeSnapshot::from_tree(self, root)
    }

    /// Create an ancestor view for an element.
    fn ancestor_view(&self, target: ElementId) -> AncestorView<'_, Self>
    where
        Self: Sized,
    {
        AncestorView::new(self, target)
    }

    /// Create a sibling view for an element.
    fn sibling_view(&self, element: ElementId) -> SiblingView<'_, Self>
    where
        Self: Sized,
    {
        SiblingView::new(self, element)
    }
}

// Blanket implementation
impl<T: TreeNav> TreeViewExt for T {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_snapshot() {
        let mut snapshot = TreeSnapshot::new();
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.len(), 0);

        let id = ElementId::new(1);
        snapshot.nodes.insert(id);
        assert!(snapshot.contains(id));
        assert!(!snapshot.is_empty());
    }

    #[test]
    fn test_snapshot_diff() {
        let mut snap1 = TreeSnapshot::new();
        snap1.nodes.insert(ElementId::new(1));
        snap1.nodes.insert(ElementId::new(2));

        let mut snap2 = TreeSnapshot::new();
        snap2.nodes.insert(ElementId::new(2));
        snap2.nodes.insert(ElementId::new(3));

        let diff = snap1.diff(&snap2);
        assert!(diff.has_changes());
        assert!(diff.added.contains(&ElementId::new(3)));
        assert!(diff.removed.contains(&ElementId::new(1)));
    }
}

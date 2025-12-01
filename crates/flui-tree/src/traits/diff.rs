//! Tree diffing and comparison utilities.
//!
//! This module provides traits and types for comparing tree structures:
//!
//! - **Structural comparison** - Compare tree topology
//! - **Node comparison** - Compare node data with custom predicates
//! - **Change detection** - Identify added, removed, and moved nodes
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeDiff, TreeNav};
//!
//! fn compare_trees<T: TreeNav + TreeDiff>(old: &T, new: &T, root: ElementId) {
//!     let diff = old.diff_structure(new, root, root);
//!
//!     println!("Added: {:?}", diff.added);
//!     println!("Removed: {:?}", diff.removed);
//!     println!("Moved: {:?}", diff.moved);
//! }
//! ```

use crate::TreeNav;
use flui_foundation::ElementId;
use std::collections::{HashMap, HashSet};

// ============================================================================
// DIFF RESULT
// ============================================================================

/// Result of comparing two trees.
///
/// Contains information about structural differences between trees.
#[derive(Debug, Clone, Default)]
pub struct TreeDiffResult {
    /// Nodes present in the new tree but not in the old tree.
    pub added: Vec<ElementId>,

    /// Nodes present in the old tree but not in the new tree.
    pub removed: Vec<ElementId>,

    /// Nodes that exist in both but have different parents.
    /// Format: (element_id, old_parent, new_parent)
    pub moved: Vec<(ElementId, Option<ElementId>, Option<ElementId>)>,

    /// Nodes that exist in both trees at the same position.
    pub unchanged: Vec<ElementId>,

    /// Nodes where children order changed.
    /// Format: (parent_id, old_children_order, new_children_order)
    pub reordered: Vec<(ElementId, Vec<ElementId>, Vec<ElementId>)>,
}

impl TreeDiffResult {
    /// Create an empty diff result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there are any differences.
    #[inline]
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty()
            || !self.removed.is_empty()
            || !self.moved.is_empty()
            || !self.reordered.is_empty()
    }

    /// Check if trees are structurally identical.
    #[inline]
    pub fn is_identical(&self) -> bool {
        !self.has_changes()
    }

    /// Get total number of changes.
    pub fn change_count(&self) -> usize {
        self.added.len() + self.removed.len() + self.moved.len() + self.reordered.len()
    }

    /// Get a summary of changes.
    pub fn summary(&self) -> DiffSummary {
        DiffSummary {
            added_count: self.added.len(),
            removed_count: self.removed.len(),
            moved_count: self.moved.len(),
            reordered_count: self.reordered.len(),
            unchanged_count: self.unchanged.len(),
        }
    }
}

/// Summary statistics for a diff operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSummary {
    /// Number of added nodes.
    pub added_count: usize,
    /// Number of removed nodes.
    pub removed_count: usize,
    /// Number of moved nodes.
    pub moved_count: usize,
    /// Number of reordered parent nodes.
    pub reordered_count: usize,
    /// Number of unchanged nodes.
    pub unchanged_count: usize,
}

impl DiffSummary {
    /// Total number of changes (not counting unchanged).
    pub fn total_changes(&self) -> usize {
        self.added_count + self.removed_count + self.moved_count + self.reordered_count
    }
}

// ============================================================================
// DIFF OPTIONS
// ============================================================================

/// Configuration options for tree diffing.
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Whether to track reordering of children.
    pub track_reordering: bool,

    /// Whether to track unchanged nodes.
    pub track_unchanged: bool,

    /// Maximum depth to compare (None = unlimited).
    pub max_depth: Option<usize>,

    /// Stop after finding this many changes.
    pub max_changes: Option<usize>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            track_reordering: true,
            track_unchanged: false, // Often not needed
            max_depth: None,
            max_changes: None,
        }
    }
}

impl DiffOptions {
    /// Create options for quick structural diff.
    pub fn quick() -> Self {
        Self {
            track_reordering: false,
            track_unchanged: false,
            max_depth: None,
            max_changes: Some(100),
        }
    }

    /// Create options for full diff with all details.
    pub fn full() -> Self {
        Self {
            track_reordering: true,
            track_unchanged: true,
            max_depth: None,
            max_changes: None,
        }
    }

    /// Builder: set max depth.
    pub fn with_max_depth(mut self, depth: Option<usize>) -> Self {
        self.max_depth = depth;
        self
    }

    /// Builder: enable/disable reordering tracking.
    pub fn with_reordering(mut self, enabled: bool) -> Self {
        self.track_reordering = enabled;
        self
    }

    /// Builder: enable/disable unchanged tracking.
    pub fn with_unchanged(mut self, enabled: bool) -> Self {
        self.track_unchanged = enabled;
        self
    }
}

// ============================================================================
// NODE MATCHER
// ============================================================================

/// Trait for matching nodes between trees.
///
/// By default, nodes are matched by their ElementId. Custom implementations
/// can match nodes by content, position, or other criteria.
pub trait NodeMatcher<T: TreeNav, U: TreeNav = T>: Send + Sync {
    /// Check if two nodes should be considered "the same" for diffing purposes.
    ///
    /// This determines whether a node is "moved" vs "added/removed".
    fn nodes_match(&self, old_tree: &T, old_id: ElementId, new_tree: &U, new_id: ElementId)
        -> bool;
}

/// Default matcher that compares by ElementId.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdMatcher;

impl<T: TreeNav, U: TreeNav> NodeMatcher<T, U> for IdMatcher {
    fn nodes_match(
        &self,
        _old_tree: &T,
        old_id: ElementId,
        _new_tree: &U,
        new_id: ElementId,
    ) -> bool {
        old_id == new_id
    }
}

/// Matcher that uses a custom predicate.
pub struct PredicateMatcher<F> {
    predicate: F,
}

impl<F> PredicateMatcher<F> {
    /// Create a new predicate-based matcher.
    pub fn new(predicate: F) -> Self {
        Self { predicate }
    }
}

impl<T, U, F> NodeMatcher<T, U> for PredicateMatcher<F>
where
    T: TreeNav,
    U: TreeNav,
    F: Fn(&T, ElementId, &U, ElementId) -> bool + Send + Sync,
{
    fn nodes_match(
        &self,
        old_tree: &T,
        old_id: ElementId,
        new_tree: &U,
        new_id: ElementId,
    ) -> bool {
        (self.predicate)(old_tree, old_id, new_tree, new_id)
    }
}

// ============================================================================
// TREE DIFF TRAIT
// ============================================================================

/// Trait for comparing tree structures.
///
/// Provides methods to compute differences between trees.
pub trait TreeDiff: TreeNav + Sized {
    /// Compare structure with another tree of the same type.
    ///
    /// Uses default ID-based matching.
    fn diff_structure(
        &self,
        other: &Self,
        self_root: ElementId,
        other_root: ElementId,
    ) -> TreeDiffResult {
        self.diff_with_options(
            other,
            self_root,
            other_root,
            &DiffOptions::default(),
            &IdMatcher,
        )
    }

    /// Compare with custom options and default matcher.
    fn diff_with_opts(
        &self,
        other: &Self,
        self_root: ElementId,
        other_root: ElementId,
        options: &DiffOptions,
    ) -> TreeDiffResult {
        self.diff_with_options(other, self_root, other_root, options, &IdMatcher)
    }

    /// Compare with custom options and matcher.
    fn diff_with_options<M: NodeMatcher<Self, Self>>(
        &self,
        other: &Self,
        self_root: ElementId,
        other_root: ElementId,
        options: &DiffOptions,
        matcher: &M,
    ) -> TreeDiffResult {
        let mut result = TreeDiffResult::new();

        // Collect all nodes from both trees
        let self_nodes = collect_nodes_with_parents(self, self_root, options.max_depth);
        let other_nodes = collect_nodes_with_parents(other, other_root, options.max_depth);

        // Build lookup sets
        let self_ids: HashSet<ElementId> = self_nodes.keys().copied().collect();
        let other_ids: HashSet<ElementId> = other_nodes.keys().copied().collect();

        // Find added nodes (in other but not in self)
        for &id in &other_ids {
            if !self_ids.contains(&id) {
                result.added.push(id);

                if let Some(max) = options.max_changes {
                    if result.change_count() >= max {
                        return result;
                    }
                }
            }
        }

        // Find removed nodes (in self but not in other)
        for &id in &self_ids {
            if !other_ids.contains(&id) {
                result.removed.push(id);

                if let Some(max) = options.max_changes {
                    if result.change_count() >= max {
                        return result;
                    }
                }
            }
        }

        // Find moved and unchanged nodes
        for &id in &self_ids {
            if other_ids.contains(&id) {
                let self_parent = self_nodes.get(&id).copied().flatten();
                let other_parent = other_nodes.get(&id).copied().flatten();

                if self_parent != other_parent {
                    result.moved.push((id, self_parent, other_parent));
                } else if options.track_unchanged {
                    result.unchanged.push(id);
                }

                if let Some(max) = options.max_changes {
                    if result.change_count() >= max {
                        return result;
                    }
                }
            }
        }

        // Check for reordering if requested
        if options.track_reordering {
            // Check nodes that exist in both trees
            for &id in &self_ids {
                if other_ids.contains(&id) {
                    let self_children: Vec<ElementId> = self.children(id).collect();
                    let other_children: Vec<ElementId> = other.children(id).collect();

                    // Only report if both have children and order differs
                    if !self_children.is_empty()
                        && !other_children.is_empty()
                        && self_children != other_children
                    {
                        // Check if it's just reordering (same elements, different order)
                        let self_set: HashSet<_> = self_children.iter().collect();
                        let other_set: HashSet<_> = other_children.iter().collect();

                        if self_set == other_set {
                            result.reordered.push((id, self_children, other_children));

                            if let Some(max) = options.max_changes {
                                if result.change_count() >= max {
                                    return result;
                                }
                            }
                        }
                    }
                }
            }
        }

        result
    }

    /// Check if two subtrees are structurally identical.
    fn is_structurally_equal(
        &self,
        other: &Self,
        self_root: ElementId,
        other_root: ElementId,
    ) -> bool {
        let options = DiffOptions::quick();
        let diff = self.diff_with_opts(other, self_root, other_root, &options);
        diff.is_identical()
    }

    /// Get nodes that were added in another tree.
    fn added_nodes(
        &self,
        other: &Self,
        self_root: ElementId,
        other_root: ElementId,
    ) -> Vec<ElementId> {
        let diff = self.diff_structure(other, self_root, other_root);
        diff.added
    }

    /// Get nodes that were removed compared to another tree.
    fn removed_nodes(
        &self,
        other: &Self,
        self_root: ElementId,
        other_root: ElementId,
    ) -> Vec<ElementId> {
        let diff = self.diff_structure(other, self_root, other_root);
        diff.removed
    }
}

// Blanket implementation for all TreeNav types
impl<T: TreeNav> TreeDiff for T {}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Collect all nodes with their parents from a subtree.
fn collect_nodes_with_parents<T: TreeNav>(
    tree: &T,
    root: ElementId,
    max_depth: Option<usize>,
) -> HashMap<ElementId, Option<ElementId>> {
    let mut result = HashMap::new();
    let mut stack: Vec<(ElementId, usize)> = vec![(root, 0)];

    while let Some((current, depth)) = stack.pop() {
        if let Some(max) = max_depth {
            if depth > max {
                continue;
            }
        }

        result.insert(current, tree.parent(current));

        for child in tree.children(current) {
            stack.push((child, depth + 1));
        }
    }

    result
}

/// Compute the minimal edit distance between two trees.
///
/// This is an approximation based on the diff result.
pub fn tree_edit_distance<T: TreeDiff>(
    tree1: &T,
    tree2: &T,
    root1: ElementId,
    root2: ElementId,
) -> usize {
    let diff = tree1.diff_structure(tree2, root1, root2);
    diff.added.len() + diff.removed.len() + diff.moved.len()
}

/// Find common subtree roots between two trees.
///
/// Returns pairs of (tree1_root, tree2_root) for matching subtrees.
pub fn find_common_subtrees<T: TreeDiff>(
    tree1: &T,
    tree2: &T,
    root1: ElementId,
    root2: ElementId,
) -> Vec<(ElementId, ElementId)> {
    let diff = tree1.diff_structure(tree2, root1, root2);

    // Common subtrees are unchanged nodes that have matching children structure
    diff.unchanged
        .iter()
        .filter(|&&id| {
            let children1: Vec<_> = tree1.children(id).collect();
            let children2: Vec<_> = tree2.children(id).collect();
            children1 == children2
        })
        .map(|&id| (id, id))
        .collect()
}

// ============================================================================
// INCREMENTAL DIFF
// ============================================================================

/// Tracker for incremental tree changes.
///
/// Useful for tracking changes as they happen rather than
/// computing a full diff.
#[derive(Debug, Clone, Default)]
pub struct ChangeTracker {
    added: Vec<ElementId>,
    removed: Vec<ElementId>,
    moved: Vec<(ElementId, Option<ElementId>, Option<ElementId>)>,
}

impl ChangeTracker {
    /// Create a new change tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a node addition.
    pub fn record_add(&mut self, id: ElementId) {
        self.added.push(id);
    }

    /// Record a node removal.
    pub fn record_remove(&mut self, id: ElementId) {
        self.removed.push(id);
    }

    /// Record a node move.
    pub fn record_move(
        &mut self,
        id: ElementId,
        old_parent: Option<ElementId>,
        new_parent: Option<ElementId>,
    ) {
        self.moved.push((id, old_parent, new_parent));
    }

    /// Check if any changes have been recorded.
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || !self.moved.is_empty()
    }

    /// Convert to a diff result.
    pub fn into_diff(self) -> TreeDiffResult {
        TreeDiffResult {
            added: self.added,
            removed: self.removed,
            moved: self.moved,
            unchanged: Vec::new(),
            reordered: Vec::new(),
        }
    }

    /// Clear all recorded changes.
    pub fn clear(&mut self) {
        self.added.clear();
        self.removed.clear();
        self.moved.clear();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_result() {
        let mut diff = TreeDiffResult::new();
        assert!(diff.is_identical());
        assert!(!diff.has_changes());

        let id = ElementId::new(1);
        diff.added.push(id);

        assert!(!diff.is_identical());
        assert!(diff.has_changes());
        assert_eq!(diff.change_count(), 1);

        let summary = diff.summary();
        assert_eq!(summary.added_count, 1);
        assert_eq!(summary.total_changes(), 1);
    }

    #[test]
    fn test_diff_options_builders() {
        let quick = DiffOptions::quick();
        assert!(!quick.track_reordering);
        assert!(quick.max_changes.is_some());

        let full = DiffOptions::full();
        assert!(full.track_reordering);
        assert!(full.track_unchanged);

        let custom = DiffOptions::default()
            .with_max_depth(Some(10))
            .with_reordering(false)
            .with_unchanged(true);

        assert_eq!(custom.max_depth, Some(10));
        assert!(!custom.track_reordering);
        assert!(custom.track_unchanged);
    }

    #[test]
    fn test_change_tracker() {
        let mut tracker = ChangeTracker::new();
        assert!(!tracker.has_changes());

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);

        tracker.record_add(id1);
        tracker.record_remove(id2);
        tracker.record_move(id1, None, Some(id2));

        assert!(tracker.has_changes());

        let diff = tracker.into_diff();
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.moved.len(), 1);
    }

    #[test]
    fn test_diff_summary() {
        let summary = DiffSummary {
            added_count: 5,
            removed_count: 3,
            moved_count: 2,
            reordered_count: 1,
            unchanged_count: 10,
        };

        assert_eq!(summary.total_changes(), 11);
    }
}

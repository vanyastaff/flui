//! Tree statistics visitor for analyzing tree structure.
//!
//! This module provides visitors that collect statistical information
//! about tree structure, useful for debugging and optimization.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{StatisticsVisitor, visit_depth_first, TreeNav};
//!
//! let mut stats = StatisticsVisitor::new();
//! visit_depth_first(&tree, root, &mut stats);
//!
//! let report = stats.report();
//! println!("Total nodes: {}", report.total_nodes);
//! println!("Max depth: {}", report.max_depth);
//! println!("Avg branching factor: {:.2}", report.avg_branching_factor);
//! ```

use super::{sealed, TreeVisitor, VisitorResult};
use crate::TreeNav;
use flui_foundation::ElementId;
use std::collections::HashMap;

// ============================================================================
// STATISTICS REPORT
// ============================================================================

/// Comprehensive statistics about a tree structure.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeStatistics {
    /// Total number of nodes in the tree.
    pub total_nodes: usize,

    /// Maximum depth encountered.
    pub max_depth: usize,

    /// Minimum depth encountered (usually 0).
    pub min_depth: usize,

    /// Average depth of all nodes.
    pub avg_depth: f64,

    /// Number of leaf nodes (nodes with no children).
    pub leaf_count: usize,

    /// Number of internal nodes (non-leaf).
    pub internal_count: usize,

    /// Average branching factor (children per non-leaf node).
    pub avg_branching_factor: f64,

    /// Maximum branching factor (most children).
    pub max_branching_factor: usize,

    /// Distribution of nodes by depth level.
    pub depth_distribution: HashMap<usize, usize>,

    /// Distribution of branching factors.
    pub branching_distribution: HashMap<usize, usize>,

    /// Width at each level (max nodes at same depth).
    pub max_width: usize,

    /// Level with maximum width.
    pub widest_level: usize,
}

impl Default for TreeStatistics {
    fn default() -> Self {
        Self {
            total_nodes: 0,
            max_depth: 0,
            min_depth: usize::MAX,
            avg_depth: 0.0,
            leaf_count: 0,
            internal_count: 0,
            avg_branching_factor: 0.0,
            max_branching_factor: 0,
            depth_distribution: HashMap::new(),
            branching_distribution: HashMap::new(),
            max_width: 0,
            widest_level: 0,
        }
    }
}

impl TreeStatistics {
    /// Check if statistics represent an empty tree.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_nodes == 0
    }

    /// Get the tree height (same as max_depth).
    #[inline]
    pub fn height(&self) -> usize {
        self.max_depth
    }

    /// Calculate balance factor (1.0 = perfectly balanced).
    ///
    /// Returns ratio of actual height to theoretical minimum height
    /// for this number of nodes with average branching factor.
    pub fn balance_factor(&self) -> f64 {
        if self.total_nodes <= 1 || self.avg_branching_factor <= 1.0 {
            return 1.0;
        }

        // Theoretical minimum height for b-ary tree
        let theoretical_min = (self.total_nodes as f64).log(self.avg_branching_factor) as usize + 1;

        if theoretical_min == 0 {
            return 1.0;
        }

        theoretical_min as f64 / (self.max_depth + 1) as f64
    }

    /// Estimate memory overhead based on node count.
    ///
    /// Assumes each node uses approximately `bytes_per_node` bytes.
    pub fn estimated_memory(&self, bytes_per_node: usize) -> usize {
        self.total_nodes * bytes_per_node
    }

    /// Get a summary string.
    pub fn summary(&self) -> String {
        format!(
            "nodes={}, depth={}, leaves={}, avg_bf={:.2}",
            self.total_nodes, self.max_depth, self.leaf_count, self.avg_branching_factor
        )
    }
}

// ============================================================================
// STATISTICS VISITOR
// ============================================================================

/// Visitor that collects comprehensive tree statistics.
///
/// This visitor tracks various metrics during traversal and produces
/// a detailed [`TreeStatistics`] report.
pub struct StatisticsVisitor {
    // Accumulators
    total_nodes: usize,
    total_depth: usize, // For average calculation
    max_depth: usize,
    min_depth: usize,

    // Child tracking (element -> child count)
    child_counts: HashMap<ElementId, usize>,

    // Parent tracking for leaf detection
    parents: HashMap<ElementId, Option<ElementId>>,

    // Depth distribution
    depth_counts: HashMap<usize, usize>,
}

impl StatisticsVisitor {
    /// Create a new statistics visitor.
    pub fn new() -> Self {
        Self {
            total_nodes: 0,
            total_depth: 0,
            max_depth: 0,
            min_depth: usize::MAX,
            child_counts: HashMap::new(),
            parents: HashMap::new(),
            depth_counts: HashMap::new(),
        }
    }

    /// Record that a node has a child.
    fn record_child(&mut self, parent: ElementId, child: ElementId) {
        *self.child_counts.entry(parent).or_insert(0) += 1;
        self.parents.insert(child, Some(parent));
    }

    /// Generate final statistics report.
    pub fn report(&self) -> TreeStatistics {
        let mut stats = TreeStatistics::default();

        stats.total_nodes = self.total_nodes;
        stats.max_depth = self.max_depth;
        stats.min_depth = if self.min_depth == usize::MAX {
            0
        } else {
            self.min_depth
        };

        // Average depth
        if self.total_nodes > 0 {
            stats.avg_depth = self.total_depth as f64 / self.total_nodes as f64;
        }

        // Leaf and internal count
        for (id, _) in &self.parents {
            if !self.child_counts.contains_key(id) {
                stats.leaf_count += 1;
            }
        }
        stats.internal_count = self.total_nodes.saturating_sub(stats.leaf_count);

        // Branching factor statistics
        let mut total_children = 0usize;
        let mut max_branching = 0usize;
        let mut branching_dist: HashMap<usize, usize> = HashMap::new();

        for &count in self.child_counts.values() {
            total_children += count;
            if count > max_branching {
                max_branching = count;
            }
            *branching_dist.entry(count).or_insert(0) += 1;
        }

        stats.max_branching_factor = max_branching;
        stats.branching_distribution = branching_dist;

        if stats.internal_count > 0 {
            stats.avg_branching_factor = total_children as f64 / stats.internal_count as f64;
        }

        // Width statistics
        let mut max_width = 0usize;
        let mut widest_level = 0usize;

        for (&depth, &count) in &self.depth_counts {
            if count > max_width {
                max_width = count;
                widest_level = depth;
            }
        }

        stats.max_width = max_width;
        stats.widest_level = widest_level;
        stats.depth_distribution = self.depth_counts.clone();

        stats
    }

    /// Reset the visitor for reuse.
    pub fn reset(&mut self) {
        self.total_nodes = 0;
        self.total_depth = 0;
        self.max_depth = 0;
        self.min_depth = usize::MAX;
        self.child_counts.clear();
        self.parents.clear();
        self.depth_counts.clear();
    }
}

impl Default for StatisticsVisitor {
    fn default() -> Self {
        Self::new()
    }
}

impl sealed::Sealed for StatisticsVisitor {}

impl TreeVisitor for StatisticsVisitor {
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        self.total_nodes += 1;
        self.total_depth += depth;

        if depth > self.max_depth {
            self.max_depth = depth;
        }
        if depth < self.min_depth {
            self.min_depth = depth;
        }

        // Track depth distribution
        *self.depth_counts.entry(depth).or_insert(0) += 1;

        // Initialize parent entry if not already set
        self.parents.entry(id).or_insert(None);

        VisitorResult::Continue
    }

    fn pre_children(&mut self, id: ElementId, _depth: usize) {
        // Initialize child count
        self.child_counts.entry(id).or_insert(0);
    }
}

// ============================================================================
// STATISTICS WITH TREE ACCESS
// ============================================================================

/// Visitor that collects statistics with full tree access.
///
/// This version can track parent-child relationships more accurately.
pub struct StatisticsVisitorMut<'a, T: TreeNav> {
    tree: &'a T,
    stats: StatisticsVisitor,
}

impl<'a, T: TreeNav> StatisticsVisitorMut<'a, T> {
    /// Create with tree reference.
    pub fn new(tree: &'a T) -> Self {
        Self {
            tree,
            stats: StatisticsVisitor::new(),
        }
    }

    /// Process the tree starting from root.
    pub fn analyze(&mut self, root: ElementId) {
        self.analyze_impl(root, 0);
    }

    fn analyze_impl(&mut self, id: ElementId, depth: usize) {
        self.stats.visit(id, depth);

        let children: Vec<ElementId> = self.tree.children(id).collect();
        let child_count = children.len();

        if child_count > 0 {
            self.stats.child_counts.insert(id, child_count);
        }

        for child in children {
            self.stats.record_child(id, child);
            self.analyze_impl(child, depth + 1);
        }
    }

    /// Get the statistics report.
    pub fn report(&self) -> TreeStatistics {
        self.stats.report()
    }
}

// ============================================================================
// CONVENIENCE FUNCTIONS
// ============================================================================

/// Collect statistics for a tree.
pub fn collect_statistics<T: TreeNav>(tree: &T, root: ElementId) -> TreeStatistics {
    let mut visitor = StatisticsVisitorMut::new(tree);
    visitor.analyze(root);
    visitor.report()
}

/// Get a quick summary of tree structure.
pub fn tree_summary<T: TreeNav>(tree: &T, root: ElementId) -> String {
    let stats = collect_statistics(tree, root);
    stats.summary()
}

/// Compare statistics of two trees.
pub fn compare_statistics(a: &TreeStatistics, b: &TreeStatistics) -> StatisticsComparison {
    StatisticsComparison {
        node_diff: b.total_nodes as isize - a.total_nodes as isize,
        depth_diff: b.max_depth as isize - a.max_depth as isize,
        leaf_diff: b.leaf_count as isize - a.leaf_count as isize,
        width_diff: b.max_width as isize - a.max_width as isize,
        branching_factor_diff: b.avg_branching_factor - a.avg_branching_factor,
    }
}

/// Comparison between two tree statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct StatisticsComparison {
    /// Change in total nodes.
    pub node_diff: isize,
    /// Change in max depth.
    pub depth_diff: isize,
    /// Change in leaf count.
    pub leaf_diff: isize,
    /// Change in max width.
    pub width_diff: isize,
    /// Change in average branching factor.
    pub branching_factor_diff: f64,
}

impl StatisticsComparison {
    /// Check if any metric changed.
    pub fn has_changes(&self) -> bool {
        self.node_diff != 0
            || self.depth_diff != 0
            || self.leaf_diff != 0
            || self.width_diff != 0
            || self.branching_factor_diff.abs() > f64::EPSILON
    }

    /// Check if the tree grew.
    pub fn is_growth(&self) -> bool {
        self.node_diff > 0
    }

    /// Check if the tree shrunk.
    pub fn is_shrinkage(&self) -> bool {
        self.node_diff < 0
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_statistics_default() {
        let stats = TreeStatistics::default();
        assert!(stats.is_empty());
        assert_eq!(stats.height(), 0);
    }

    #[test]
    fn test_statistics_visitor() {
        let mut visitor = StatisticsVisitor::new();

        // Simulate visiting a small tree
        visitor.visit(ElementId::new(1), 0); // root
        visitor.visit(ElementId::new(2), 1); // child 1
        visitor.visit(ElementId::new(3), 1); // child 2
        visitor.visit(ElementId::new(4), 2); // grandchild

        let report = visitor.report();
        assert_eq!(report.total_nodes, 4);
        assert_eq!(report.max_depth, 2);
        assert_eq!(report.min_depth, 0);
    }

    #[test]
    fn test_statistics_comparison() {
        let a = TreeStatistics {
            total_nodes: 10,
            max_depth: 3,
            leaf_count: 5,
            max_width: 4,
            avg_branching_factor: 2.0,
            ..Default::default()
        };

        let b = TreeStatistics {
            total_nodes: 15,
            max_depth: 4,
            leaf_count: 8,
            max_width: 5,
            avg_branching_factor: 2.5,
            ..Default::default()
        };

        let cmp = compare_statistics(&a, &b);
        assert!(cmp.has_changes());
        assert!(cmp.is_growth());
        assert_eq!(cmp.node_diff, 5);
        assert_eq!(cmp.depth_diff, 1);
    }

    #[test]
    fn test_balance_factor() {
        let balanced = TreeStatistics {
            total_nodes: 15,
            max_depth: 3,
            avg_branching_factor: 2.0,
            ..Default::default()
        };

        let factor = balanced.balance_factor();
        assert!(factor > 0.0 && factor <= 1.0);
    }

    #[test]
    fn test_statistics_summary() {
        let stats = TreeStatistics {
            total_nodes: 100,
            max_depth: 5,
            leaf_count: 50,
            avg_branching_factor: 2.5,
            ..Default::default()
        };

        let summary = stats.summary();
        assert!(summary.contains("100"));
        assert!(summary.contains("5"));
        assert!(summary.contains("50"));
    }
}

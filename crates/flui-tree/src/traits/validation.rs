//! Tree validation traits and utilities.
//!
//! This module provides traits and types for validating tree invariants:
//!
//! - **Structural integrity** - Parent/child consistency
//! - **Cycle detection** - No circular references
//! - **Orphan detection** - All nodes reachable from root
//! - **Depth limits** - Maximum tree depth validation
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeValidator, TreeNav};
//!
//! fn check_tree<T: TreeNav + TreeValidator>(tree: &T, root: ElementId) {
//!     match tree.validate(root) {
//!         Ok(report) => println!("Valid: {} nodes", report.total_nodes),
//!         Err(issues) => {
//!             for issue in issues.iter() {
//!                 println!("Issue: {:?}", issue);
//!             }
//!         }
//!     }
//! }
//! ```

use crate::{TreeError, TreeNav, TreeResult};
use flui_foundation::ElementId;
use std::collections::{HashMap, HashSet};

// ============================================================================
// VALIDATION REPORT
// ============================================================================

/// Report generated after successful tree validation.
///
/// Contains statistics and metrics about the validated tree.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// Total number of nodes in the tree.
    pub total_nodes: usize,

    /// Maximum depth encountered.
    pub max_depth: usize,

    /// Number of leaf nodes (nodes with no children).
    pub leaf_count: usize,

    /// Average branching factor (children per non-leaf node).
    pub avg_branching_factor: f32,

    /// Root element ID.
    pub root: ElementId,
}

impl ValidationReport {
    /// Create a new validation report.
    pub fn new(root: ElementId) -> Self {
        Self {
            total_nodes: 0,
            max_depth: 0,
            leaf_count: 0,
            avg_branching_factor: 0.0,
            root,
        }
    }

    /// Check if the tree is empty (only root node).
    #[inline]
    pub fn is_single_node(&self) -> bool {
        self.total_nodes == 1
    }

    /// Check if the tree is "flat" (max depth is 1).
    #[inline]
    pub fn is_flat(&self) -> bool {
        self.max_depth <= 1
    }
}

// ============================================================================
// VALIDATION ISSUES
// ============================================================================

/// Types of validation issues that can be detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    /// A cycle was detected in the tree structure.
    ///
    /// Contains the path of element IDs forming the cycle.
    CycleDetected {
        /// The element where the cycle was detected.
        element: ElementId,
        /// The path leading to the cycle.
        path: Vec<ElementId>,
    },

    /// An orphaned node was found (not reachable from root).
    OrphanedNode {
        /// The orphaned element ID.
        element: ElementId,
    },

    /// Parent reference is invalid (parent doesn't contain child).
    InvalidParentReference {
        /// The child element.
        child: ElementId,
        /// The claimed parent (which doesn't list child).
        claimed_parent: ElementId,
    },

    /// Child reference is invalid (child's parent doesn't match).
    InvalidChildReference {
        /// The parent element.
        parent: ElementId,
        /// The claimed child (whose parent doesn't match).
        claimed_child: ElementId,
    },

    /// Tree exceeds maximum allowed depth.
    MaxDepthExceeded {
        /// The element that exceeded depth.
        element: ElementId,
        /// The depth of the element.
        depth: usize,
        /// The maximum allowed depth.
        max_allowed: usize,
    },

    /// Duplicate element ID found in different positions.
    DuplicateElement {
        /// The duplicated element ID.
        element: ElementId,
        /// First occurrence parent.
        first_parent: Option<ElementId>,
        /// Second occurrence parent.
        second_parent: Option<ElementId>,
    },

    /// Root element not found in tree.
    RootNotFound {
        /// The expected root element.
        root: ElementId,
    },

    /// Element references itself as parent.
    SelfReference {
        /// The self-referencing element.
        element: ElementId,
    },
}

impl ValidationIssue {
    /// Get the primary element ID associated with this issue.
    pub fn element_id(&self) -> Option<ElementId> {
        match self {
            Self::CycleDetected { element, .. } => Some(*element),
            Self::OrphanedNode { element } => Some(*element),
            Self::InvalidParentReference { child, .. } => Some(*child),
            Self::InvalidChildReference { parent, .. } => Some(*parent),
            Self::MaxDepthExceeded { element, .. } => Some(*element),
            Self::DuplicateElement { element, .. } => Some(*element),
            Self::RootNotFound { root } => Some(*root),
            Self::SelfReference { element } => Some(*element),
        }
    }

    /// Check if this issue indicates a critical structural problem.
    ///
    /// Critical issues prevent the tree from functioning correctly.
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            Self::CycleDetected { .. }
                | Self::RootNotFound { .. }
                | Self::SelfReference { .. }
                | Self::DuplicateElement { .. }
        )
    }

    /// Check if this issue is recoverable without rebuilding the tree.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::OrphanedNode { .. } | Self::MaxDepthExceeded { .. }
        )
    }
}

/// Collection of validation issues.
#[derive(Debug, Clone, Default)]
pub struct ValidationIssues {
    issues: Vec<ValidationIssue>,
}

impl ValidationIssues {
    /// Create empty issues collection.
    pub fn new() -> Self {
        Self { issues: Vec::new() }
    }

    /// Add an issue to the collection.
    pub fn push(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }

    /// Check if there are any issues.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// Get the number of issues.
    #[inline]
    pub fn len(&self) -> usize {
        self.issues.len()
    }

    /// Iterate over all issues.
    pub fn iter(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues.iter()
    }

    /// Check if there are any critical issues.
    pub fn has_critical(&self) -> bool {
        self.issues.iter().any(ValidationIssue::is_critical)
    }

    /// Get only critical issues.
    pub fn critical_issues(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues.iter().filter(|i| i.is_critical())
    }

    /// Get only recoverable issues.
    pub fn recoverable_issues(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues.iter().filter(|i| i.is_recoverable())
    }

    /// Convert to Vec.
    pub fn into_vec(self) -> Vec<ValidationIssue> {
        self.issues
    }
}

impl IntoIterator for ValidationIssues {
    type Item = ValidationIssue;
    type IntoIter = std::vec::IntoIter<ValidationIssue>;

    fn into_iter(self) -> Self::IntoIter {
        self.issues.into_iter()
    }
}

impl<'a> IntoIterator for &'a ValidationIssues {
    type Item = &'a ValidationIssue;
    type IntoIter = std::slice::Iter<'a, ValidationIssue>;

    fn into_iter(self) -> Self::IntoIter {
        self.issues.iter()
    }
}

// ============================================================================
// VALIDATION OPTIONS
// ============================================================================

/// Configuration options for tree validation.
#[derive(Debug, Clone)]
pub struct ValidationOptions {
    /// Maximum allowed tree depth (None = no limit).
    pub max_depth: Option<usize>,

    /// Whether to check for orphaned nodes.
    pub check_orphans: bool,

    /// Whether to verify parent/child consistency.
    pub check_parent_child_consistency: bool,

    /// Whether to detect cycles.
    pub check_cycles: bool,

    /// Stop validation on first critical issue.
    pub fail_fast: bool,

    /// Maximum number of issues to collect before stopping.
    pub max_issues: Option<usize>,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            max_depth: Some(1000), // Reasonable default
            check_orphans: true,
            check_parent_child_consistency: true,
            check_cycles: true,
            fail_fast: false,
            max_issues: Some(100),
        }
    }
}

impl ValidationOptions {
    /// Create options for quick validation (cycles only).
    pub fn quick() -> Self {
        Self {
            max_depth: None,
            check_orphans: false,
            check_parent_child_consistency: false,
            check_cycles: true,
            fail_fast: true,
            max_issues: Some(1),
        }
    }

    /// Create options for thorough validation.
    pub fn thorough() -> Self {
        Self {
            max_depth: Some(10000),
            check_orphans: true,
            check_parent_child_consistency: true,
            check_cycles: true,
            fail_fast: false,
            max_issues: None,
        }
    }

    /// Builder: set max depth.
    pub fn with_max_depth(mut self, depth: Option<usize>) -> Self {
        self.max_depth = depth;
        self
    }

    /// Builder: enable/disable orphan checking.
    pub fn with_orphan_check(mut self, enabled: bool) -> Self {
        self.check_orphans = enabled;
        self
    }

    /// Builder: enable/disable fail-fast mode.
    pub fn with_fail_fast(mut self, enabled: bool) -> Self {
        self.fail_fast = enabled;
        self
    }
}

// ============================================================================
// TREE VALIDATOR TRAIT
// ============================================================================

/// Trait for validating tree structure and invariants.
///
/// This trait extends `TreeNav` with validation capabilities.
/// It provides methods to check tree integrity and detect structural issues.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{TreeValidator, ValidationOptions};
///
/// // Quick validation (cycles only)
/// let result = tree.validate_quick(root);
///
/// // Full validation with custom options
/// let options = ValidationOptions::default()
///     .with_max_depth(Some(50))
///     .with_fail_fast(true);
/// let result = tree.validate_with_options(root, &options);
/// ```
pub trait TreeValidator: TreeNav {
    /// Validate the tree starting from the given root.
    ///
    /// Returns a validation report on success, or validation issues on failure.
    fn validate(&self, root: ElementId) -> Result<ValidationReport, ValidationIssues> {
        self.validate_with_options(root, &ValidationOptions::default())
    }

    /// Perform quick validation (cycle detection only).
    fn validate_quick(&self, root: ElementId) -> Result<ValidationReport, ValidationIssues> {
        self.validate_with_options(root, &ValidationOptions::quick())
    }

    /// Perform thorough validation with all checks enabled.
    fn validate_thorough(&self, root: ElementId) -> Result<ValidationReport, ValidationIssues> {
        self.validate_with_options(root, &ValidationOptions::thorough())
    }

    /// Validate with custom options.
    fn validate_with_options(
        &self,
        root: ElementId,
        options: &ValidationOptions,
    ) -> Result<ValidationReport, ValidationIssues> {
        let mut issues = ValidationIssues::new();
        let mut report = ValidationReport::new(root);

        // Check root exists
        if self.get(root).is_none() {
            issues.push(ValidationIssue::RootNotFound { root });
            return Err(issues);
        }

        // Track visited nodes for cycle detection and orphan checking
        let mut visited: HashSet<ElementId> = HashSet::new();
        let mut node_parents: HashMap<ElementId, Option<ElementId>> = HashMap::new();

        // DFS traversal for validation
        let mut stack: Vec<(ElementId, usize, Vec<ElementId>)> = vec![(root, 0, vec![root])];
        let mut total_children = 0usize;
        let mut non_leaf_count = 0usize;

        while let Some((current, depth, path)) = stack.pop() {
            // Check issue limit
            if let Some(max) = options.max_issues {
                if issues.len() >= max {
                    break;
                }
            }

            // Check for fail-fast
            if options.fail_fast && issues.has_critical() {
                break;
            }

            // Check self-reference
            if self.parent(current) == Some(current) {
                issues.push(ValidationIssue::SelfReference { element: current });
                continue;
            }

            // Check for cycles
            if options.check_cycles && visited.contains(&current) {
                issues.push(ValidationIssue::CycleDetected {
                    element: current,
                    path: path.clone(),
                });
                continue;
            }

            visited.insert(current);

            // Check max depth
            if let Some(max_depth) = options.max_depth {
                if depth > max_depth {
                    issues.push(ValidationIssue::MaxDepthExceeded {
                        element: current,
                        depth,
                        max_allowed: max_depth,
                    });
                    // Don't continue to children if depth exceeded
                    continue;
                }
            }

            // Update report stats
            report.total_nodes += 1;
            if depth > report.max_depth {
                report.max_depth = depth;
            }

            // Check parent/child consistency
            if options.check_parent_child_consistency {
                if let Some(parent_id) = self.parent(current) {
                    // Verify parent actually contains this child
                    let parent_has_child = self.children(parent_id).any(|c| c == current);
                    if !parent_has_child {
                        issues.push(ValidationIssue::InvalidParentReference {
                            child: current,
                            claimed_parent: parent_id,
                        });
                    }

                    // Check for duplicate in different parents
                    if let Some(&prev_parent) = node_parents.get(&current) {
                        if prev_parent != Some(parent_id) {
                            issues.push(ValidationIssue::DuplicateElement {
                                element: current,
                                first_parent: prev_parent,
                                second_parent: Some(parent_id),
                            });
                        }
                    }
                }
                node_parents.insert(current, self.parent(current));
            }

            // Process children
            let children: Vec<ElementId> = self.children(current).collect();
            let child_count = children.len();

            if child_count == 0 {
                report.leaf_count += 1;
            } else {
                non_leaf_count += 1;
                total_children += child_count;

                // Verify children reference this node as parent
                if options.check_parent_child_consistency {
                    for &child in &children {
                        if self.parent(child) != Some(current) {
                            issues.push(ValidationIssue::InvalidChildReference {
                                parent: current,
                                claimed_child: child,
                            });
                        }
                    }
                }

                // Add children to stack with updated path
                for child in children.into_iter().rev() {
                    let mut new_path = path.clone();
                    new_path.push(child);
                    stack.push((child, depth + 1, new_path));
                }
            }
        }

        // Calculate average branching factor
        if non_leaf_count > 0 {
            report.avg_branching_factor = total_children as f32 / non_leaf_count as f32;
        }

        // Check for orphaned nodes
        if options.check_orphans {
            for id in self.node_ids() {
                if !visited.contains(&id) {
                    issues.push(ValidationIssue::OrphanedNode { element: id });

                    if let Some(max) = options.max_issues {
                        if issues.len() >= max {
                            break;
                        }
                    }
                }
            }
        }

        if issues.is_empty() {
            Ok(report)
        } else {
            Err(issues)
        }
    }

    /// Check if a specific subtree is valid.
    ///
    /// This is more efficient than full validation when you only need
    /// to validate a portion of the tree.
    fn is_valid_subtree(&self, root: ElementId) -> bool {
        self.validate_quick(root).is_ok()
    }

    /// Detect if adding a child would create a cycle.
    ///
    /// Returns `true` if adding `child` under `parent` would create a cycle.
    fn would_create_cycle(&self, parent: ElementId, child: ElementId) -> bool {
        if parent == child {
            return true;
        }

        // Check if parent is a descendant of child
        let mut current = Some(parent);
        let mut visited = HashSet::new();

        while let Some(id) = current {
            if id == child {
                return true;
            }
            if !visited.insert(id) {
                // Already visited, there's an existing cycle
                return true;
            }
            current = self.parent(id);
        }

        false
    }
}

// Blanket implementation for all TreeNav types
impl<T: TreeNav> TreeValidator for T {}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Validate a tree and return a TreeResult.
///
/// Convenience function that converts validation issues to TreeError.
pub fn validate_tree<T: TreeValidator>(tree: &T, root: ElementId) -> TreeResult<ValidationReport> {
    tree.validate(root).map_err(|issues| {
        if let Some(first_critical) = issues.critical_issues().next() {
            match first_critical {
                ValidationIssue::CycleDetected { element, .. } => {
                    TreeError::cycle_detected(*element)
                }
                ValidationIssue::RootNotFound { root } => TreeError::not_found(*root),
                ValidationIssue::SelfReference { element } => TreeError::cycle_detected(*element),
                ValidationIssue::DuplicateElement { element, .. } => {
                    TreeError::already_exists(*element)
                }
                _ => TreeError::internal("validation failed"),
            }
        } else if let Some(first) = issues.iter().next() {
            match first {
                ValidationIssue::OrphanedNode { element } => TreeError::not_found(*element),
                ValidationIssue::MaxDepthExceeded {
                    element,
                    max_allowed,
                    ..
                } => TreeError::max_depth_exceeded(*element, *max_allowed),
                ValidationIssue::InvalidParentReference {
                    child,
                    claimed_parent,
                } => TreeError::invalid_parent(*child, *claimed_parent),
                ValidationIssue::InvalidChildReference { parent, .. } => {
                    TreeError::internal(format!("invalid child reference at {}", parent.get()))
                }
                _ => TreeError::internal("validation failed"),
            }
        } else {
            TreeError::internal("validation failed with unknown issues")
        }
    })
}

/// Quick check if a tree has cycles.
pub fn has_cycles<T: TreeValidator>(tree: &T, root: ElementId) -> bool {
    tree.validate_quick(root).is_err()
}

/// Get all orphaned nodes in a tree.
pub fn find_orphans<T: TreeValidator>(tree: &T, root: ElementId) -> Vec<ElementId> {
    let options = ValidationOptions {
        check_orphans: true,
        check_cycles: false,
        check_parent_child_consistency: false,
        max_depth: None,
        fail_fast: false,
        max_issues: None,
    };

    match tree.validate_with_options(root, &options) {
        Ok(_) => Vec::new(),
        Err(issues) => issues
            .iter()
            .filter_map(|issue| match issue {
                ValidationIssue::OrphanedNode { element } => Some(*element),
                _ => None,
            })
            .collect(),
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_report() {
        let root = ElementId::new(1);
        let mut report = ValidationReport::new(root);

        assert!(report.is_single_node() || report.total_nodes == 0);

        report.total_nodes = 1;
        assert!(report.is_single_node());

        report.max_depth = 0;
        assert!(report.is_flat());

        report.max_depth = 2;
        assert!(!report.is_flat());
    }

    #[test]
    fn test_validation_issue_methods() {
        let id = ElementId::new(1);

        let cycle = ValidationIssue::CycleDetected {
            element: id,
            path: vec![id],
        };
        assert!(cycle.is_critical());
        assert!(!cycle.is_recoverable());
        assert_eq!(cycle.element_id(), Some(id));

        let orphan = ValidationIssue::OrphanedNode { element: id };
        assert!(!orphan.is_critical());
        assert!(orphan.is_recoverable());

        let depth = ValidationIssue::MaxDepthExceeded {
            element: id,
            depth: 100,
            max_allowed: 50,
        };
        assert!(!depth.is_critical());
        assert!(depth.is_recoverable());
    }

    #[test]
    fn test_validation_issues_collection() {
        let mut issues = ValidationIssues::new();
        assert!(issues.is_empty());

        let id = ElementId::new(1);
        issues.push(ValidationIssue::OrphanedNode { element: id });

        assert!(!issues.is_empty());
        assert_eq!(issues.len(), 1);
        assert!(!issues.has_critical());

        issues.push(ValidationIssue::CycleDetected {
            element: id,
            path: vec![],
        });

        assert!(issues.has_critical());
        assert_eq!(issues.critical_issues().count(), 1);
        assert_eq!(issues.recoverable_issues().count(), 1);
    }

    #[test]
    fn test_validation_options_builders() {
        let quick = ValidationOptions::quick();
        assert!(quick.fail_fast);
        assert!(!quick.check_orphans);

        let thorough = ValidationOptions::thorough();
        assert!(!thorough.fail_fast);
        assert!(thorough.check_orphans);

        let custom = ValidationOptions::default()
            .with_max_depth(Some(100))
            .with_orphan_check(false)
            .with_fail_fast(true);

        assert_eq!(custom.max_depth, Some(100));
        assert!(!custom.check_orphans);
        assert!(custom.fail_fast);
    }
}

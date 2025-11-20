//! Snapshot testing utilities
//!
//! Provides utilities for capturing and comparing element tree snapshots.

use crate::element::{Element, ElementId, ElementTree};
use std::fmt;

/// Snapshot of an element tree for testing
///
/// Captures the structure of the element tree for comparison in tests.
///
/// # Examples
///
/// ```rust,ignore
/// let snapshot = ElementTreeSnapshot::capture(&tree);
/// println!("{}", snapshot);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementTreeSnapshot {
    /// Number of elements in the tree
    pub element_count: usize,
    /// Component count
    pub component_count: usize,
    /// Render element count
    pub render_count: usize,
    /// Provider count
    pub provider_count: usize,
    /// Tree structure as text
    pub structure: String,
}

impl ElementTreeSnapshot {
    /// Capture a snapshot of the current element tree
    pub fn capture(tree: &ElementTree) -> Self {
        let mut component_count = 0;
        let mut render_count = 0;
        let mut provider_count = 0;
        let mut structure = String::new();

        for i in 0..tree.len() {
            let id = ElementId::new(i + 1);
            if let Some(element) = tree.get(id) {
                match element {
                    Element::Component(_) => {
                        component_count += 1;
                        structure.push_str(&format!("  Component({})\n", id.get()));
                    }
                    Element::Render(_) => {
                        render_count += 1;
                        structure.push_str(&format!("  Render({})\n", id.get()));
                    }
                    // TODO: Re-enable after sliver migration
                    // Element::Sliver(_) => {
                    //     render_count += 1;
                    //     structure.push_str(&format!("  Sliver({})\n", id.get()));
                    // }
                    Element::Provider(_) => {
                        provider_count += 1;
                        structure.push_str(&format!("  Provider({})\n", id.get()));
                    }
                }
            }
        }

        Self {
            element_count: tree.len(),
            component_count,
            render_count,
            provider_count,
            structure,
        }
    }

    /// Compare two snapshots and return the differences
    pub fn diff(&self, other: &Self) -> SnapshotDiff {
        SnapshotDiff {
            element_count_diff: other.element_count as i32 - self.element_count as i32,
            component_count_diff: other.component_count as i32 - self.component_count as i32,
            render_count_diff: other.render_count as i32 - self.render_count as i32,
            provider_count_diff: other.provider_count as i32 - self.provider_count as i32,
            structure_changed: self.structure != other.structure,
        }
    }

    /// Check if the snapshot matches expected counts
    pub fn matches(&self, components: usize, renders: usize, providers: usize) -> bool {
        self.component_count == components
            && self.render_count == renders
            && self.provider_count == providers
    }
}

impl fmt::Display for ElementTreeSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ElementTree Snapshot:")?;
        writeln!(f, "  Total elements: {}", self.element_count)?;
        writeln!(f, "  Components: {}", self.component_count)?;
        writeln!(f, "  Renders: {}", self.render_count)?;
        writeln!(f, "  Providers: {}", self.provider_count)?;
        writeln!(f, "Structure:")?;
        write!(f, "{}", self.structure)
    }
}

/// Difference between two snapshots
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotDiff {
    /// Difference in element count
    pub element_count_diff: i32,
    /// Difference in component count
    pub component_count_diff: i32,
    /// Difference in render count
    pub render_count_diff: i32,
    /// Difference in provider count
    pub provider_count_diff: i32,
    /// Whether the structure changed
    pub structure_changed: bool,
}

impl SnapshotDiff {
    /// Check if there are any differences
    pub fn has_changes(&self) -> bool {
        self.element_count_diff != 0
            || self.component_count_diff != 0
            || self.render_count_diff != 0
            || self.provider_count_diff != 0
            || self.structure_changed
    }
}

impl fmt::Display for SnapshotDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.has_changes() {
            return writeln!(f, "No changes");
        }

        writeln!(f, "Snapshot differences:")?;
        if self.element_count_diff != 0 {
            writeln!(f, "  Elements: {:+}", self.element_count_diff)?;
        }
        if self.component_count_diff != 0 {
            writeln!(f, "  Components: {:+}", self.component_count_diff)?;
        }
        if self.render_count_diff != 0 {
            writeln!(f, "  Renders: {:+}", self.render_count_diff)?;
        }
        if self.provider_count_diff != 0 {
            writeln!(f, "  Providers: {:+}", self.provider_count_diff)?;
        }
        if self.structure_changed {
            writeln!(f, "  Structure: CHANGED")?;
        }
        Ok(())
    }
}

/// Assert that a tree snapshot matches expected counts
///
/// # Panics
///
/// Panics if the counts don't match.
///
/// # Examples
///
/// ```rust,ignore
/// assert_tree_snapshot(&tree, 2, 3, 1); // 2 components, 3 renders, 1 provider
/// ```
pub fn assert_tree_snapshot(
    tree: &ElementTree,
    components: usize,
    renders: usize,
    providers: usize,
) {
    let snapshot = ElementTreeSnapshot::capture(tree);
    assert!(
        snapshot.matches(components, renders, providers),
        "Tree snapshot mismatch:\n{}\nExpected: {} components, {} renders, {} providers",
        snapshot,
        components,
        renders,
        providers
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_diff() {
        let snapshot1 = ElementTreeSnapshot {
            element_count: 5,
            component_count: 2,
            render_count: 3,
            provider_count: 0,
            structure: String::from("test"),
        };

        let snapshot2 = ElementTreeSnapshot {
            element_count: 7,
            component_count: 3,
            render_count: 4,
            provider_count: 0,
            structure: String::from("test2"),
        };

        let diff = snapshot1.diff(&snapshot2);
        assert_eq!(diff.element_count_diff, 2);
        assert_eq!(diff.component_count_diff, 1);
        assert_eq!(diff.render_count_diff, 1);
        assert!(diff.structure_changed);
        assert!(diff.has_changes());
    }

    #[test]
    fn test_snapshot_matches() {
        let snapshot = ElementTreeSnapshot {
            element_count: 5,
            component_count: 2,
            render_count: 3,
            provider_count: 0,
            structure: String::new(),
        };

        assert!(snapshot.matches(2, 3, 0));
        assert!(!snapshot.matches(1, 3, 0));
    }
}

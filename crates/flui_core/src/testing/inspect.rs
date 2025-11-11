//! State inspection utilities
//!
//! Provides utilities for inspecting and debugging element state during tests.

use crate::element::{Element, ElementId, ElementTree};
use std::fmt;

/// Inspector for examining element tree state
///
/// # Examples
///
/// ```rust,ignore
/// let inspector = TreeInspector::new(&tree);
/// inspector.print_tree();
/// ```
#[derive(Debug)]
pub struct TreeInspector<'a> {
    tree: &'a ElementTree,
}

impl<'a> TreeInspector<'a> {
    /// Create a new tree inspector
    pub fn new(tree: &'a ElementTree) -> Self {
        Self { tree }
    }

    /// Get a summary of the tree
    pub fn summary(&self) -> TreeSummary {
        let mut summary = TreeSummary::default();

        for i in 0..self.tree.len() {
            let id = ElementId::new(i + 1);
            if let Some(element) = self.tree.get(id) {
                match element {
                    Element::Component(_) => summary.component_count += 1,
                    Element::Render(_) => summary.render_count += 1,
                    Element::Sliver(_) => summary.render_count += 1, // Count slivers as render objects
                    Element::Provider(_) => summary.provider_count += 1,
                }

                if element.is_dirty() {
                    summary.dirty_count += 1;
                }
            }
        }

        summary.total_count = self.tree.len();
        summary
    }

    /// Find all elements of a specific type
    pub fn find_components(&self) -> Vec<ElementId> {
        let mut components = Vec::new();
        for i in 0..self.tree.len() {
            let id = ElementId::new(i + 1);
            if let Some(element) = self.tree.get(id) {
                if element.as_component().is_some() {
                    components.push(id);
                }
            }
        }
        components
    }

    /// Find all render elements
    pub fn find_renders(&self) -> Vec<ElementId> {
        let mut renders = Vec::new();
        for i in 0..self.tree.len() {
            let id = ElementId::new(i + 1);
            if let Some(element) = self.tree.get(id) {
                if element.as_render().is_some() {
                    renders.push(id);
                }
            }
        }
        renders
    }

    /// Find all provider elements
    pub fn find_providers(&self) -> Vec<ElementId> {
        let mut providers = Vec::new();
        for i in 0..self.tree.len() {
            let id = ElementId::new(i + 1);
            if let Some(element) = self.tree.get(id) {
                if element.as_provider().is_some() {
                    providers.push(id);
                }
            }
        }
        providers
    }

    /// Find all dirty elements
    pub fn find_dirty(&self) -> Vec<ElementId> {
        let mut dirty = Vec::new();
        for i in 0..self.tree.len() {
            let id = ElementId::new(i + 1);
            if let Some(element) = self.tree.get(id) {
                if element.is_dirty() {
                    dirty.push(id);
                }
            }
        }
        dirty
    }

    /// Print a visual representation of the tree
    pub fn print_tree(&self) {
        println!("Element Tree:");
        println!("═════════════");
        for i in 0..self.tree.len() {
            let id = ElementId::new(i + 1);
            if let Some(element) = self.tree.get(id) {
                let type_name = match element {
                    Element::Component(_) => "Component",
                    Element::Render(_) => "Render",
                    Element::Sliver(_) => "Sliver",
                    Element::Provider(_) => "Provider ",
                };
                let dirty = if element.is_dirty() { " [DIRTY]" } else { "" };
                println!("  {} (ID: {}){}", type_name, id.get(), dirty);
            }
        }
        println!();
        println!("{}", self.summary());
    }
}

/// Summary of element tree state
#[derive(Debug, Clone, Default)]
pub struct TreeSummary {
    /// Total number of elements
    pub total_count: usize,
    /// Number of component elements
    pub component_count: usize,
    /// Number of render elements
    pub render_count: usize,
    /// Number of provider elements
    pub provider_count: usize,
    /// Number of dirty elements
    pub dirty_count: usize,
}

impl fmt::Display for TreeSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Tree Summary:")?;
        writeln!(f, "  Total:      {}", self.total_count)?;
        writeln!(f, "  Components: {}", self.component_count)?;
        writeln!(f, "  Renders:    {}", self.render_count)?;
        writeln!(f, "  Providers:  {}", self.provider_count)?;
        writeln!(f, "  Dirty:      {}", self.dirty_count)
    }
}

/// Helper to get tree summary
pub fn tree_summary(tree: &ElementTree) -> TreeSummary {
    TreeInspector::new(tree).summary()
}

/// Helper to print tree
pub fn print_tree(tree: &ElementTree) {
    TreeInspector::new(tree).print_tree();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_summary() {
        let tree = ElementTree::new();
        let summary = tree_summary(&tree);
        assert_eq!(summary.total_count, 0);
        assert_eq!(summary.component_count, 0);
    }
}

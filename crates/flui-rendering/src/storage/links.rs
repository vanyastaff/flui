//! NodeLinks - Tree structure data shared across all protocols.
//!
//! This module provides `NodeLinks`, which stores the parent/child relationships
//! and depth information for render nodes. This is separated from protocol-specific
//! data to allow shared implementation across Box and Sliver protocols.

use flui_foundation::RenderId;

/// Tree structure links shared across all protocols.
///
/// Contains the parent/child relationships and depth information that
/// is common to all render nodes regardless of their protocol.
///
/// # Example
///
/// ```rust,ignore
/// let mut links = NodeLinks::new();
/// links.add_child(child_id);
/// assert_eq!(links.children().len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct NodeLinks {
    /// Parent node ID (None for root).
    parent: Option<RenderId>,

    /// Child node IDs in insertion order.
    children: Vec<RenderId>,

    /// Depth in the tree (root = 0).
    depth: u16,
}

impl Default for NodeLinks {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeLinks {
    /// Creates new links with no parent (root node).
    #[inline]
    pub fn new() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Creates new links with a parent.
    #[inline]
    pub fn with_parent(parent: RenderId, depth: u16) -> Self {
        Self {
            parent: Some(parent),
            children: Vec::new(),
            depth,
        }
    }

    // ========================================================================
    // Parent Access
    // ========================================================================

    /// Returns the parent ID.
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.parent
    }

    /// Sets the parent ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<RenderId>) {
        self.parent = parent;
    }

    /// Returns true if this is a root node (no parent).
    #[inline]
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    // ========================================================================
    // Children Access
    // ========================================================================

    /// Returns the children IDs.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        &self.children
    }

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns true if this node has no children.
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Adds a child ID to the end.
    #[inline]
    pub fn add_child(&mut self, child: RenderId) {
        self.children.push(child);
    }

    /// Inserts a child ID at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > self.child_count()`.
    #[inline]
    pub fn insert_child(&mut self, index: usize, child: RenderId) {
        self.children.insert(index, child);
    }

    /// Removes a child ID.
    ///
    /// Returns `true` if the child was found and removed.
    pub fn remove_child(&mut self, child: RenderId) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child) {
            self.children.remove(pos);
            true
        } else {
            false
        }
    }

    /// Removes all children.
    #[inline]
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    // ========================================================================
    // Depth Access
    // ========================================================================

    /// Returns the depth in the tree.
    #[inline]
    pub fn depth(&self) -> u16 {
        self.depth
    }

    /// Sets the depth.
    #[inline]
    pub fn set_depth(&mut self, depth: u16) {
        self.depth = depth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_id(n: usize) -> RenderId {
        RenderId::new(n)
    }

    #[test]
    fn test_new() {
        let links = NodeLinks::new();
        assert!(links.is_root());
        assert!(links.is_leaf());
        assert_eq!(links.depth(), 0);
    }

    #[test]
    fn test_with_parent() {
        let parent_id = make_id(1);
        let links = NodeLinks::with_parent(parent_id, 5);

        assert_eq!(links.parent(), Some(parent_id));
        assert!(!links.is_root());
        assert_eq!(links.depth(), 5);
    }

    #[test]
    fn test_add_children() {
        let mut links = NodeLinks::new();
        let child1 = make_id(1);
        let child2 = make_id(2);

        links.add_child(child1);
        links.add_child(child2);

        assert_eq!(links.child_count(), 2);
        assert!(!links.is_leaf());
        assert_eq!(links.children(), &[child1, child2]);
    }

    #[test]
    fn test_remove_child() {
        let mut links = NodeLinks::new();
        let child1 = make_id(1);
        let child2 = make_id(2);

        links.add_child(child1);
        links.add_child(child2);

        assert!(links.remove_child(child1));
        assert_eq!(links.children(), &[child2]);

        assert!(!links.remove_child(child1)); // Already removed
    }

    #[test]
    fn test_insert_child() {
        let mut links = NodeLinks::new();
        let child1 = make_id(1);
        let child2 = make_id(2);
        let child3 = make_id(3);

        links.add_child(child1);
        links.add_child(child3);
        links.insert_child(1, child2);

        assert_eq!(links.children(), &[child1, child2, child3]);
    }
}

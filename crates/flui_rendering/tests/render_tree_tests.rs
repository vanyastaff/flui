//! Integration tests for RenderTree.
//!
//! These tests verify the RenderTree works correctly with real render objects
//! and demonstrates typical usage patterns.

use flui_foundation::RenderId;
use flui_rendering::objects::r#box::basic::{RenderAlign, RenderPadding};
use flui_rendering::objects::r#box::effects::RenderOpacity;
use flui_rendering::traits::RenderBox;
use flui_rendering::tree::{RenderNode, RenderTree};
use flui_types::{Alignment, EdgeInsets};

// ============================================================================
// Helper Functions
// ============================================================================

fn create_padding(padding: f32) -> Box<dyn RenderBox> {
    Box::new(RenderPadding::new(EdgeInsets::all(padding)))
}

fn create_align(alignment: Alignment) -> Box<dyn RenderBox> {
    Box::new(RenderAlign::new(alignment))
}

fn create_opacity(opacity: f32) -> Box<dyn RenderBox> {
    Box::new(RenderOpacity::new(opacity))
}

// ============================================================================
// Basic Tree Operations
// ============================================================================

#[test]
fn test_build_simple_tree() {
    // Build a tree:
    //   root (Padding)
    //     └── child (Align)
    //           └── grandchild (Opacity)

    let mut tree = RenderTree::new();

    // Insert root
    let root_id = tree.insert(create_padding(10.0));
    tree.set_root(Some(root_id));

    // Insert child
    let child_id = tree
        .insert_child(root_id, create_align(Alignment::CENTER))
        .unwrap();

    // Insert grandchild
    let grandchild_id = tree.insert_child(child_id, create_opacity(0.5)).unwrap();

    // Verify structure
    assert_eq!(tree.len(), 3);
    assert_eq!(tree.root(), Some(root_id));

    // Verify parent relationships
    assert_eq!(tree.parent(root_id), None);
    assert_eq!(tree.parent(child_id), Some(root_id));
    assert_eq!(tree.parent(grandchild_id), Some(child_id));

    // Verify children
    assert_eq!(tree.children(root_id), &[child_id]);
    assert_eq!(tree.children(child_id), &[grandchild_id]);
    assert_eq!(tree.children(grandchild_id), &[]);

    // Verify depths
    assert_eq!(tree.depth(root_id), Some(0));
    assert_eq!(tree.depth(child_id), Some(1));
    assert_eq!(tree.depth(grandchild_id), Some(2));
}

#[test]
fn test_build_wide_tree() {
    // Build a tree with multiple children:
    //   root (Padding)
    //     ├── child1 (Align)
    //     ├── child2 (Opacity)
    //     └── child3 (Padding)

    let mut tree = RenderTree::new();

    let root_id = tree.insert(create_padding(10.0));
    tree.set_root(Some(root_id));

    let child1 = tree
        .insert_child(root_id, create_align(Alignment::TOP_LEFT))
        .unwrap();
    let child2 = tree.insert_child(root_id, create_opacity(0.8)).unwrap();
    let child3 = tree.insert_child(root_id, create_padding(5.0)).unwrap();

    assert_eq!(tree.len(), 4);
    assert_eq!(tree.children(root_id).len(), 3);
    assert_eq!(tree.children(root_id), &[child1, child2, child3]);

    // All children have same depth
    assert_eq!(tree.depth(child1), Some(1));
    assert_eq!(tree.depth(child2), Some(1));
    assert_eq!(tree.depth(child3), Some(1));
}

// ============================================================================
// Tree Navigation
// ============================================================================

#[test]
fn test_ancestor_relationships() {
    let mut tree = RenderTree::new();

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));

    let child = tree
        .insert_child(root, create_align(Alignment::CENTER))
        .unwrap();
    let grandchild = tree.insert_child(child, create_opacity(0.5)).unwrap();

    // Test is_ancestor
    assert!(tree.is_ancestor(root, child));
    assert!(tree.is_ancestor(root, grandchild));
    assert!(tree.is_ancestor(child, grandchild));

    // Not ancestors
    assert!(!tree.is_ancestor(child, root));
    assert!(!tree.is_ancestor(grandchild, root));
    assert!(!tree.is_ancestor(grandchild, child));

    // Self is not ancestor
    assert!(!tree.is_ancestor(root, root));
}

#[test]
fn test_path_to_root() {
    let mut tree = RenderTree::new();

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));

    let child = tree
        .insert_child(root, create_align(Alignment::CENTER))
        .unwrap();
    let grandchild = tree.insert_child(child, create_opacity(0.5)).unwrap();

    // Path from grandchild to root
    let path = tree.path_to_root(grandchild);
    assert_eq!(path, vec![root, child, grandchild]);

    // Path from root
    let path = tree.path_to_root(root);
    assert_eq!(path, vec![root]);
}

// ============================================================================
// Tree Modification
// ============================================================================

#[test]
fn test_remove_leaf_node() {
    let mut tree = RenderTree::new();

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));

    let child = tree
        .insert_child(root, create_align(Alignment::CENTER))
        .unwrap();

    assert_eq!(tree.len(), 2);

    // Remove child (leaf)
    let removed = tree.remove(child);
    assert!(removed.is_some());
    assert_eq!(tree.len(), 1);
    assert!(!tree.contains(child));

    // Root's children should be empty
    assert_eq!(tree.children(root), &[]);
}

#[test]
fn test_remove_recursive() {
    let mut tree = RenderTree::new();

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));

    let child = tree
        .insert_child(root, create_align(Alignment::CENTER))
        .unwrap();
    let grandchild1 = tree.insert_child(child, create_opacity(0.5)).unwrap();
    let grandchild2 = tree.insert_child(child, create_padding(5.0)).unwrap();

    assert_eq!(tree.len(), 4);

    // Remove child and all its descendants
    let count = tree.remove_recursive(child);
    assert_eq!(count, 3); // child + 2 grandchildren

    assert_eq!(tree.len(), 1);
    assert!(tree.contains(root));
    assert!(!tree.contains(child));
    assert!(!tree.contains(grandchild1));
    assert!(!tree.contains(grandchild2));
}

#[test]
fn test_remove_root_recursive() {
    let mut tree = RenderTree::new();

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));

    tree.insert_child(root, create_align(Alignment::CENTER))
        .unwrap();
    tree.insert_child(root, create_opacity(0.5)).unwrap();

    assert_eq!(tree.len(), 3);

    // Remove entire tree
    let count = tree.remove_recursive(root);
    assert_eq!(count, 3);
    assert!(tree.is_empty());
    assert!(tree.root().is_none());
}

// ============================================================================
// Traversal
// ============================================================================

#[test]
fn test_depth_first_traversal() {
    let mut tree = RenderTree::new();

    // Build tree:
    //   root
    //     ├── a
    //     │   └── a1
    //     └── b

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));

    let a = tree
        .insert_child(root, create_align(Alignment::CENTER))
        .unwrap();
    let a1 = tree.insert_child(a, create_opacity(0.5)).unwrap();
    let b = tree.insert_child(root, create_padding(5.0)).unwrap();

    // Collect traversal order
    let mut visited = Vec::new();
    tree.visit_depth_first(|id, _node| {
        visited.push(id);
    });

    // Pre-order: root, a, a1, b
    assert_eq!(visited, vec![root, a, a1, b]);
}

#[test]
fn test_iteration() {
    let mut tree = RenderTree::new();

    let id1 = tree.insert(create_padding(10.0));
    let id2 = tree.insert(create_align(Alignment::CENTER));
    let id3 = tree.insert(create_opacity(0.5));

    // Collect all IDs
    let ids: Vec<RenderId> = tree.ids().collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
    assert!(ids.contains(&id3));

    // Collect all nodes
    let nodes: Vec<&RenderNode> = tree.nodes().collect();
    assert_eq!(nodes.len(), 3);
}

// ============================================================================
// Dirty Node Collection
// ============================================================================

// Note: test_collect_dirty_nodes is disabled because RenderPadding::base()
// is not fully implemented yet. The collect_nodes_needing_* methods
// require BaseRenderObject storage in concrete render objects.
//
// TODO: Enable this test once render objects properly implement base() method.

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_empty_tree() {
    let tree = RenderTree::new();

    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
    assert!(tree.root().is_none());

    // Operations on empty tree shouldn't panic
    let mut visited = Vec::new();
    tree.visit_depth_first(|id, _| visited.push(id));
    assert!(visited.is_empty());
}

#[test]
fn test_single_node_tree() {
    let mut tree = RenderTree::new();

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));

    assert_eq!(tree.len(), 1);
    assert_eq!(tree.depth(root), Some(0));
    assert_eq!(tree.parent(root), None);
    assert_eq!(tree.children(root), &[]);

    let path = tree.path_to_root(root);
    assert_eq!(path, vec![root]);
}

#[test]
fn test_insert_child_to_nonexistent_parent() {
    let mut tree = RenderTree::new();

    // Try to insert child to non-existent parent
    let fake_id = RenderId::new(999);
    let result = tree.insert_child(fake_id, create_padding(10.0));

    assert!(result.is_none());
    assert!(tree.is_empty());
}

#[test]
fn test_reuse_slots_after_removal() {
    let mut tree = RenderTree::new();

    // Insert and remove nodes
    let id1 = tree.insert(create_padding(10.0));
    let id2 = tree.insert(create_align(Alignment::CENTER));

    tree.remove(id1);

    // Insert new node - Slab reuses slots, so id3 will be same as id1
    let id3 = tree.insert(create_opacity(0.5));

    // id3 should reuse id1's slot (Slab behavior)
    assert_eq!(id3, id1); // Slot reuse!
    assert!(tree.contains(id2));
    assert!(tree.contains(id3));
    // After reuse, id1 slot is now occupied by id3
    assert!(tree.contains(id1)); // Same as id3
}

// ============================================================================
// Capacity and Performance
// ============================================================================

#[test]
fn test_with_capacity() {
    let tree = RenderTree::with_capacity(100);
    assert!(tree.is_empty());
    // Capacity is pre-allocated but tree is still empty
}

#[test]
fn test_reserve() {
    let mut tree = RenderTree::new();
    tree.insert(create_padding(10.0));

    // Reserve additional capacity
    tree.reserve(100);

    // Tree should still have same content
    assert_eq!(tree.len(), 1);
}

#[test]
fn test_clear() {
    let mut tree = RenderTree::new();

    let root = tree.insert(create_padding(10.0));
    tree.set_root(Some(root));
    tree.insert_child(root, create_align(Alignment::CENTER));
    tree.insert_child(root, create_opacity(0.5));

    assert_eq!(tree.len(), 3);

    tree.clear();

    assert!(tree.is_empty());
    assert!(tree.root().is_none());
}

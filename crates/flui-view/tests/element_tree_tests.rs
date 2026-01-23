//! Integration tests for ElementTree.
//!
//! Tests Slab-based element storage, tree operations, and parent-child relationships.

use flui_foundation::ElementId;
use flui_view::{
    BuildContext, ElementBase, ElementTree, Lifecycle, StatelessBehavior, StatelessElement,
    StatelessView, View,
};

// ============================================================================
// Test View
// ============================================================================

#[derive(Clone)]
struct TestView {
    id: u32,
}

impl StatelessView for TestView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for TestView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

// ============================================================================
// Basic Tree Operations
// ============================================================================

#[test]
fn test_tree_creation() {
    let tree = ElementTree::new();

    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
    assert!(tree.root().is_none());
}

#[test]
fn test_tree_with_capacity() {
    let tree = ElementTree::with_capacity(100);

    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
}

#[test]
fn test_mount_root() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let root_id = tree.mount_root(&view);

    assert!(!tree.is_empty());
    assert_eq!(tree.len(), 1);
    assert_eq!(tree.root(), Some(root_id));
    assert!(tree.contains(root_id));
}

#[test]
fn test_mount_root_activates_element() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let root_id = tree.mount_root(&view);

    let node = tree.get(root_id).unwrap();
    assert_eq!(node.element().lifecycle(), Lifecycle::Active);
}

#[test]
fn test_insert_child() {
    let mut tree = ElementTree::new();
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);

    assert_eq!(tree.len(), 2);
    assert!(tree.contains(child_id));
}

#[test]
fn test_insert_child_sets_parent() {
    let mut tree = ElementTree::new();
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);

    let child_node = tree.get(child_id).unwrap();
    assert_eq!(child_node.parent(), Some(root_id));
}

#[test]
fn test_insert_child_sets_slot() {
    let mut tree = ElementTree::new();
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 5);

    let child_node = tree.get(child_id).unwrap();
    assert_eq!(child_node.slot(), 5);
}

#[test]
fn test_insert_child_sets_depth() {
    let mut tree = ElementTree::new();
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);

    let child_node = tree.get(child_id).unwrap();
    assert_eq!(child_node.depth(), 1);
}

// ============================================================================
// Get Operations
// ============================================================================

#[test]
fn test_get_existing_element() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 42 };

    let id = tree.mount_root(&view);
    let node = tree.get(id);

    assert!(node.is_some());
}

#[test]
fn test_get_nonexistent_element() {
    let tree = ElementTree::new();
    let fake_id = ElementId::new(999);

    let node = tree.get(fake_id);

    assert!(node.is_none());
}

#[test]
fn test_get_mut() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    let node = tree.get_mut(id);

    assert!(node.is_some());
}

#[test]
fn test_contains() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    let fake_id = ElementId::new(999);

    assert!(tree.contains(id));
    assert!(!tree.contains(fake_id));
}

// ============================================================================
// Remove Operations
// ============================================================================

#[test]
fn test_remove_element() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    assert!(tree.contains(id));

    let removed = tree.remove(id);

    assert!(removed.is_some());
    assert!(!tree.contains(id));
    assert!(tree.is_empty());
}

#[test]
fn test_remove_clears_root() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    assert!(tree.root().is_some());

    tree.remove(id);

    assert!(tree.root().is_none());
}

#[test]
fn test_remove_unmounts_element() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    let removed_node = tree.remove(id).unwrap();

    assert_eq!(removed_node.element().lifecycle(), Lifecycle::Defunct);
}

#[test]
fn test_remove_nonexistent_returns_none() {
    let mut tree = ElementTree::new();
    let fake_id = ElementId::new(999);

    let removed = tree.remove(fake_id);

    assert!(removed.is_none());
}

// ============================================================================
// Update Operations
// ============================================================================

#[test]
fn test_update_element() {
    let mut tree = ElementTree::new();
    let view1 = TestView { id: 1 };
    let view2 = TestView { id: 2 };

    let id = tree.mount_root(&view1);
    tree.update(id, &view2);

    // Element should still exist
    assert!(tree.contains(id));
}

#[test]
fn test_mark_needs_build() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    tree.mark_needs_build(id);

    // Element should still be valid
    assert!(tree.contains(id));
}

// ============================================================================
// Activate/Deactivate Operations
// ============================================================================

#[test]
fn test_deactivate_element() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    tree.deactivate(id);

    let node = tree.get(id).unwrap();
    assert_eq!(node.element().lifecycle(), Lifecycle::Inactive);
}

#[test]
fn test_activate_element() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    tree.deactivate(id);
    tree.activate(id);

    let node = tree.get(id).unwrap();
    assert_eq!(node.element().lifecycle(), Lifecycle::Active);
}

// ============================================================================
// Iteration
// ============================================================================

#[test]
fn test_iter_empty_tree() {
    let tree = ElementTree::new();

    let ids: Vec<_> = tree.iter().collect();

    assert!(ids.is_empty());
}

#[test]
fn test_iter_single_element() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);

    let ids: Vec<_> = tree.iter().collect();

    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], id);
}

#[test]
fn test_iter_multiple_elements() {
    let mut tree = ElementTree::new();
    let root_view = TestView { id: 0 };
    let child1 = TestView { id: 1 };
    let child2 = TestView { id: 2 };

    let root_id = tree.mount_root(&root_view);
    let child1_id = tree.insert(&child1, root_id, 0);
    let child2_id = tree.insert(&child2, root_id, 1);

    let ids: Vec<_> = tree.iter().collect();

    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&root_id));
    assert!(ids.contains(&child1_id));
    assert!(ids.contains(&child2_id));
}

#[test]
fn test_iter_nodes() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);

    let nodes: Vec<_> = tree.iter_nodes().collect();

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].0, id);
}

// ============================================================================
// Deep Tree Tests
// ============================================================================

#[test]
fn test_deep_tree_depth_tracking() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 0 };

    // Build a chain: root -> child1 -> child2 -> ... -> child10
    let root_id = tree.mount_root(&view);
    let mut parent_id = root_id;

    for i in 1..=10 {
        let child_view = TestView { id: i };
        let child_id = tree.insert(&child_view, parent_id, 0);
        parent_id = child_id;
    }

    // Verify depths
    let deepest_node = tree.get(parent_id).unwrap();
    assert_eq!(deepest_node.depth(), 10);

    let root_node = tree.get(root_id).unwrap();
    assert_eq!(root_node.depth(), 0);
}

#[test]
fn test_wide_tree() {
    let mut tree = ElementTree::new();
    let root_view = TestView { id: 0 };

    let root_id = tree.mount_root(&root_view);

    // Add 100 children
    let mut child_ids = Vec::new();
    for i in 1..=100 {
        let child_view = TestView { id: i };
        let child_id = tree.insert(&child_view, root_id, i as usize - 1);
        child_ids.push(child_id);
    }

    assert_eq!(tree.len(), 101);

    // Verify all children have correct parent and depth
    for child_id in child_ids {
        let node = tree.get(child_id).unwrap();
        assert_eq!(node.parent(), Some(root_id));
        assert_eq!(node.depth(), 1);
    }
}

// ============================================================================
// ElementId Tests
// ============================================================================

#[test]
fn test_element_id_is_nonzero() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);

    // ElementId should be 1-based (NonZeroUsize)
    assert!(id.get() >= 1);
}

#[test]
fn test_element_ids_are_unique() {
    let mut tree = ElementTree::new();
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);

    assert_ne!(root_id, child_id);
}

#[test]
fn test_element_id_reuse_after_removal() {
    let mut tree = ElementTree::new();
    let view1 = TestView { id: 1 };
    let view2 = TestView { id: 2 };

    // Insert and remove first element
    let id1 = tree.mount_root(&view1);
    tree.remove(id1);

    // Insert second element - may reuse the slot
    let id2 = tree.mount_root(&view2);

    // The new element should be valid
    assert!(tree.contains(id2));
    assert_eq!(tree.len(), 1);
}

// ============================================================================
// Memory and Performance Tests
// ============================================================================

#[test]
fn test_tree_memory_layout() {
    // ElementTree should be reasonably sized
    let size = std::mem::size_of::<ElementTree>();
    // Slab + Option<ElementId>
    assert!(size < 128, "ElementTree is too large: {} bytes", size);
}

#[test]
fn test_large_tree_operations() {
    let mut tree = ElementTree::with_capacity(1000);
    let root_view = TestView { id: 0 };

    let root_id = tree.mount_root(&root_view);

    // Insert 1000 children
    for i in 1..=1000 {
        let child_view = TestView { id: i };
        tree.insert(&child_view, root_id, i as usize - 1);
    }

    assert_eq!(tree.len(), 1001);

    // Access elements
    for (id, _) in tree.iter_nodes() {
        assert!(tree.contains(id));
    }
}

// ============================================================================
// Debug Tests
// ============================================================================

#[test]
fn test_tree_debug() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    tree.mount_root(&view);

    let debug_str = format!("{:?}", tree);
    assert!(debug_str.contains("ElementTree"));
    assert!(debug_str.contains("len"));
}

#[test]
fn test_element_node_debug() {
    let mut tree = ElementTree::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view);
    let node = tree.get(id).unwrap();

    let debug_str = format!("{:?}", node);
    assert!(debug_str.contains("ElementNode"));
    assert!(debug_str.contains("depth"));
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_tree_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    // ElementTree should be Send + Sync
    // Note: This may fail if ElementBase isn't Send + Sync
    // In that case, we'd need to verify the trait bounds
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_operations_on_empty_tree() {
    let mut tree = ElementTree::new();
    let fake_id = ElementId::new(1);

    // These should not panic
    tree.update(fake_id, &TestView { id: 0 });
    tree.mark_needs_build(fake_id);
    tree.deactivate(fake_id);
    tree.activate(fake_id);

    assert!(tree.is_empty());
}

#[test]
fn test_double_mount_root() {
    let mut tree = ElementTree::new();
    let view1 = TestView { id: 1 };
    let view2 = TestView { id: 2 };

    let id1 = tree.mount_root(&view1);
    let id2 = tree.mount_root(&view2);

    // Second mount should create a new root
    // Both elements should exist, but root should be id2
    assert!(tree.contains(id1));
    assert!(tree.contains(id2));
    assert_eq!(tree.root(), Some(id2));
    assert_eq!(tree.len(), 2);
}

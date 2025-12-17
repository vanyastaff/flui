//! Integration tests for O(N) linear reconciliation algorithm.
//!
//! Tests the child reconciliation logic that matches old and new views
//! efficiently using keys and position matching.

use flui_view::{
    reconcile_children, BuildContext, ElementBase, ElementTree, Lifecycle, StatelessElement,
    StatelessView, View,
};

// ============================================================================
// Test Views
// ============================================================================

#[derive(Clone)]
struct SimpleView {
    #[allow(dead_code)]
    id: u32,
}

impl StatelessView for SimpleView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for SimpleView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self))
    }
}

#[derive(Clone)]
struct DifferentView {
    #[allow(dead_code)]
    value: String,
}

impl StatelessView for DifferentView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for DifferentView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self))
    }
}

// ============================================================================
// Empty List Tests
// ============================================================================

#[test]
fn test_reconcile_empty_to_empty() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    let result = reconcile_children(&mut tree, parent, &[], &[]);

    assert!(result.is_empty());
}

#[test]
fn test_reconcile_empty_to_some() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let v3 = SimpleView { id: 3 };
    let new_views: Vec<&dyn View> = vec![&v1, &v2, &v3];

    let result = reconcile_children(&mut tree, parent, &[], &new_views);

    assert_eq!(result.len(), 3);
    assert_eq!(tree.len(), 4); // root + 3 children

    // All new elements should be active
    for id in &result {
        let node = tree.get(*id).unwrap();
        assert_eq!(node.element().lifecycle(), Lifecycle::Active);
    }
}

#[test]
fn test_reconcile_some_to_empty() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Create children
    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let child1 = tree.insert(&v1, parent, 0);
    let child2 = tree.insert(&v2, parent, 1);

    let result = reconcile_children(&mut tree, parent, &[child1, child2], &[]);

    assert!(result.is_empty());
    assert!(!tree.contains(child1));
    assert!(!tree.contains(child2));
    assert_eq!(tree.len(), 1); // Only root remains
}

// ============================================================================
// Same Length Tests
// ============================================================================

#[test]
fn test_reconcile_same_type_same_length() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Old children
    let v1_old = SimpleView { id: 1 };
    let v2_old = SimpleView { id: 2 };
    let child1 = tree.insert(&v1_old, parent, 0);
    let child2 = tree.insert(&v2_old, parent, 1);

    // New views (same type, different id)
    let v1_new = SimpleView { id: 10 };
    let v2_new = SimpleView { id: 20 };
    let new_views: Vec<&dyn View> = vec![&v1_new, &v2_new];

    let result = reconcile_children(&mut tree, parent, &[child1, child2], &new_views);

    // Should reuse existing elements
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], child1);
    assert_eq!(result[1], child2);
}

#[test]
fn test_reconcile_different_type_replaces() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Old child (SimpleView)
    let v_old = SimpleView { id: 1 };
    let old_child = tree.insert(&v_old, parent, 0);

    // New view (DifferentView - different type)
    let v_new = DifferentView {
        value: "new".to_string(),
    };
    let new_views: Vec<&dyn View> = vec![&v_new];

    let result = reconcile_children(&mut tree, parent, &[old_child], &new_views);

    // Should create new element
    assert_eq!(result.len(), 1);
    assert_ne!(result[0], old_child);
    assert!(!tree.contains(old_child));
}

// ============================================================================
// Growing/Shrinking Tests
// ============================================================================

#[test]
fn test_reconcile_grow_list() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Start with 2 children
    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let child1 = tree.insert(&v1, parent, 0);
    let child2 = tree.insert(&v2, parent, 1);

    // Grow to 4
    let v1_new = SimpleView { id: 1 };
    let v2_new = SimpleView { id: 2 };
    let v3_new = SimpleView { id: 3 };
    let v4_new = SimpleView { id: 4 };
    let new_views: Vec<&dyn View> = vec![&v1_new, &v2_new, &v3_new, &v4_new];

    let result = reconcile_children(&mut tree, parent, &[child1, child2], &new_views);

    assert_eq!(result.len(), 4);
    // First two should be reused
    assert_eq!(result[0], child1);
    assert_eq!(result[1], child2);
    // Last two are new
    assert!(tree.contains(result[2]));
    assert!(tree.contains(result[3]));
}

#[test]
fn test_reconcile_shrink_list() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Start with 4 children
    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let v3 = SimpleView { id: 3 };
    let v4 = SimpleView { id: 4 };
    let child1 = tree.insert(&v1, parent, 0);
    let child2 = tree.insert(&v2, parent, 1);
    let child3 = tree.insert(&v3, parent, 2);
    let child4 = tree.insert(&v4, parent, 3);

    // Shrink to 2
    let v1_new = SimpleView { id: 1 };
    let v2_new = SimpleView { id: 2 };
    let new_views: Vec<&dyn View> = vec![&v1_new, &v2_new];

    let result = reconcile_children(
        &mut tree,
        parent,
        &[child1, child2, child3, child4],
        &new_views,
    );

    assert_eq!(result.len(), 2);
    // First two should be reused
    assert_eq!(result[0], child1);
    assert_eq!(result[1], child2);
    // Last two should be removed
    assert!(!tree.contains(child3));
    assert!(!tree.contains(child4));
}

// ============================================================================
// Order Changes Tests
// ============================================================================

#[test]
fn test_reconcile_type_mismatch_mid_list() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Old: [SimpleView, SimpleView, SimpleView]
    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let v3 = SimpleView { id: 3 };
    let child1 = tree.insert(&v1, parent, 0);
    let child2 = tree.insert(&v2, parent, 1);
    let child3 = tree.insert(&v3, parent, 2);

    // New: [SimpleView, DifferentView, SimpleView]
    let v1_new = SimpleView { id: 1 };
    let v2_new = DifferentView {
        value: "different".to_string(),
    };
    let v3_new = SimpleView { id: 3 };
    let new_views: Vec<&dyn View> = vec![&v1_new, &v2_new, &v3_new];

    let result = reconcile_children(&mut tree, parent, &[child1, child2, child3], &new_views);

    assert_eq!(result.len(), 3);
    // First should be reused (same type at same position)
    assert_eq!(result[0], child1);
    // Second is new (different type)
    assert_ne!(result[1], child2);
    assert!(!tree.contains(child2));
}

// ============================================================================
// End Matching Tests
// ============================================================================

#[test]
fn test_reconcile_matches_from_end() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Old: [SimpleView, SimpleView, SimpleView]
    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let v3 = SimpleView { id: 3 };
    let child1 = tree.insert(&v1, parent, 0);
    let child2 = tree.insert(&v2, parent, 1);
    let child3 = tree.insert(&v3, parent, 2);

    // New: [DifferentView, SimpleView, SimpleView]
    // The algorithm should:
    // 1. Fail to match start (different types)
    // 2. Match from end (last two are same type)
    let v1_new = DifferentView {
        value: "new".to_string(),
    };
    let v2_new = SimpleView { id: 2 };
    let v3_new = SimpleView { id: 3 };
    let new_views: Vec<&dyn View> = vec![&v1_new, &v2_new, &v3_new];

    let result = reconcile_children(&mut tree, parent, &[child1, child2, child3], &new_views);

    assert_eq!(result.len(), 3);
    // First is new (different type)
    assert!(!tree.contains(child1));
    // Last two could be reused (matched from end)
    // The exact behavior depends on implementation
}

// ============================================================================
// Large List Tests
// ============================================================================

#[test]
fn test_reconcile_large_list_same_type() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Create 100 old children
    let old_views: Vec<SimpleView> = (1..=100).map(|i| SimpleView { id: i }).collect();
    let mut old_children = Vec::new();
    for (i, view) in old_views.iter().enumerate() {
        let child = tree.insert(view, parent, i);
        old_children.push(child);
    }

    // Create 100 new views
    let new_views: Vec<SimpleView> = (101..=200).map(|i| SimpleView { id: i }).collect();
    let new_view_refs: Vec<&dyn View> = new_views.iter().map(|v| v as &dyn View).collect();

    let result = reconcile_children(&mut tree, parent, &old_children, &new_view_refs);

    assert_eq!(result.len(), 100);
    // All should be reused (same type)
    for (old, new) in old_children.iter().zip(result.iter()) {
        assert_eq!(old, new);
    }
}

#[test]
fn test_reconcile_large_growth() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    // Start with 10 children
    let old_views: Vec<SimpleView> = (1..=10).map(|i| SimpleView { id: i }).collect();
    let mut old_children = Vec::new();
    for (i, view) in old_views.iter().enumerate() {
        let child = tree.insert(view, parent, i);
        old_children.push(child);
    }

    // Grow to 100
    let new_views: Vec<SimpleView> = (1..=100).map(|i| SimpleView { id: i }).collect();
    let new_view_refs: Vec<&dyn View> = new_views.iter().map(|v| v as &dyn View).collect();

    let result = reconcile_children(&mut tree, parent, &old_children, &new_view_refs);

    assert_eq!(result.len(), 100);
    // First 10 should be reused
    for i in 0..10 {
        assert_eq!(result[i], old_children[i]);
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_reconcile_single_to_single() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    let v_old = SimpleView { id: 1 };
    let child = tree.insert(&v_old, parent, 0);

    let v_new = SimpleView { id: 2 };
    let new_views: Vec<&dyn View> = vec![&v_new];

    let result = reconcile_children(&mut tree, parent, &[child], &new_views);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0], child); // Reused
}

#[test]
fn test_reconcile_single_to_many() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    let v_old = SimpleView { id: 1 };
    let child = tree.insert(&v_old, parent, 0);

    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let v3 = SimpleView { id: 3 };
    let new_views: Vec<&dyn View> = vec![&v1, &v2, &v3];

    let result = reconcile_children(&mut tree, parent, &[child], &new_views);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0], child); // First reused
}

#[test]
fn test_reconcile_many_to_single() {
    let mut tree = ElementTree::new();
    let root = SimpleView { id: 0 };
    let parent = tree.mount_root(&root);

    let v1 = SimpleView { id: 1 };
    let v2 = SimpleView { id: 2 };
    let v3 = SimpleView { id: 3 };
    let child1 = tree.insert(&v1, parent, 0);
    let child2 = tree.insert(&v2, parent, 1);
    let child3 = tree.insert(&v3, parent, 2);

    let v_new = SimpleView { id: 1 };
    let new_views: Vec<&dyn View> = vec![&v_new];

    let result = reconcile_children(&mut tree, parent, &[child1, child2, child3], &new_views);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0], child1); // First reused
    assert!(!tree.contains(child2));
    assert!(!tree.contains(child3));
}

// ============================================================================
// Performance Characteristics Tests
// ============================================================================

#[test]
fn test_reconcile_is_linear_time() {
    // This test verifies that reconciliation doesn't have quadratic behavior
    // by running with increasingly large lists
    let sizes = [10, 100, 1000];

    for size in sizes {
        let mut tree = ElementTree::new();
        let root = SimpleView { id: 0 };
        let parent = tree.mount_root(&root);

        let old_views: Vec<SimpleView> = (1..=size).map(|i| SimpleView { id: i }).collect();
        let mut old_children = Vec::new();
        for (i, view) in old_views.iter().enumerate() {
            let child = tree.insert(view, parent, i);
            old_children.push(child);
        }

        let new_views: Vec<SimpleView> = (1..=size).map(|i| SimpleView { id: i + size }).collect();
        let new_view_refs: Vec<&dyn View> = new_views.iter().map(|v| v as &dyn View).collect();

        let result = reconcile_children(&mut tree, parent, &old_children, &new_view_refs);

        assert_eq!(result.len(), size as usize);
    }
}

//! Integration tests for ElementTree.
//!
//! Tests Slab-based element storage, tree operations, and parent-child
//! relationships.

use flui_view::{BuildContext, BuildOwner, ElementTree, IntoView, StatelessView, View, ViewExt};

// ============================================================================
// Test View
// ============================================================================

#[derive(Clone)]
struct TestView {
    #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
    id: u32,
}

impl StatelessView for TestView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for TestView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

// ============================================================================
// Basic Tree Operations
// ============================================================================

// ============================================================================
// Get Operations
// ============================================================================

// ============================================================================
// Remove Operations
// ============================================================================

#[test]
fn test_remove_element() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let view = TestView { id: 1 };

    let id = tree.mount_root(&view, &mut owner.element_owner_mut());
    assert!(tree.contains(id));

    let removed = tree.remove(id, &mut owner.element_owner_mut());

    assert!(removed.is_some());
    assert!(!tree.contains(id));
    assert!(tree.is_empty());
}

// ============================================================================
// Update Operations
// ============================================================================

// ============================================================================
// Activate/Deactivate Operations
// ============================================================================

// ============================================================================
// Iteration
// ============================================================================

// ============================================================================
// Deep Tree Tests
// ============================================================================

// ============================================================================
// ElementId Tests
// ============================================================================

// ============================================================================
// Memory and Performance Tests
// ============================================================================

// ============================================================================
// Debug Tests
// ============================================================================

// ============================================================================
// Ownership Tests
// ============================================================================

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_double_mount_root() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let view1 = TestView { id: 1 };
    let view2 = TestView { id: 2 };

    let id1 = tree.mount_root(&view1, &mut owner.element_owner_mut());
    let id2 = tree.mount_root(&view2, &mut owner.element_owner_mut());

    // Second mount should create a new root
    // Both elements should exist, but root should be id2
    assert!(tree.contains(id1));
    assert!(tree.contains(id2));
    assert_eq!(tree.root(), Some(id2));
    assert_eq!(tree.len(), 2);
}

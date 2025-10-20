//! Integration tests for Element Lifecycle (Phase 3)
//!
//! Tests lifecycle state transitions, update_child() algorithm,
//! and InactiveElements integration.

use flui_core::{
    AnyElement, AnyWidget, Context, ElementLifecycle, ElementTree, StatelessWidget, Widget,
};
use std::sync::Arc;
use parking_lot::RwLock;

// ============================================================================
// Test Widgets
// ============================================================================

/// Simple test widget for lifecycle testing
#[derive(Debug, Clone)]
struct TestWidget {
    id: u32,
}

impl StatelessWidget for TestWidget {
    fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
        Box::new(TestWidget { id: self.id + 100 })
    }
}

/// Another test widget with different type
#[derive(Debug, Clone)]
struct OtherWidget {
    value: u32,
}

impl StatelessWidget for OtherWidget {
    fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
        Box::new(OtherWidget { value: self.value + 1 })
    }
}

// ============================================================================
// Lifecycle State Transition Tests
// ============================================================================

#[test]
fn test_element_lifecycle_initial_state() {
    let widget = TestWidget { id: 1 };
    let element = widget.into_element();

    // New elements start in Initial state
    assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
}

#[test]
fn test_element_lifecycle_mount_transition() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    // Initial state
    assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

    // Mount transitions to Active
    element.mount(None, 0);
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);
}

#[test]
fn test_element_lifecycle_deactivate_transition() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    // Mount first
    element.mount(None, 0);
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);

    // Deactivate transitions to Inactive
    element.deactivate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);
}

#[test]
fn test_element_lifecycle_activate_transition() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    // Mount, then deactivate
    element.mount(None, 0);
    element.deactivate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

    // Activate transitions back to Active
    element.activate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);

    // Should be marked dirty after activation
    assert!(element.is_dirty());
}

#[test]
fn test_element_lifecycle_unmount_transition() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    // Mount first
    element.mount(None, 0);
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);

    // Unmount transitions to Defunct
    element.unmount();
    assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
}

#[test]
fn test_element_lifecycle_full_cycle() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    // Initial → Active → Inactive → Active → Defunct
    assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

    element.mount(None, 0);
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);

    element.deactivate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

    element.activate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);

    element.unmount();
    assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
}

// ============================================================================
// ElementLifecycle Enum Tests
// ============================================================================

#[test]
fn test_lifecycle_is_active() {
    assert!(!ElementLifecycle::Initial.is_active());
    assert!(ElementLifecycle::Active.is_active());
    assert!(!ElementLifecycle::Inactive.is_active());
    assert!(!ElementLifecycle::Defunct.is_active());
}

#[test]
fn test_lifecycle_can_reactivate() {
    assert!(!ElementLifecycle::Initial.can_reactivate());
    assert!(!ElementLifecycle::Active.can_reactivate());
    assert!(ElementLifecycle::Inactive.can_reactivate());
    assert!(!ElementLifecycle::Defunct.can_reactivate());
}

#[test]
fn test_lifecycle_is_mounted() {
    assert!(!ElementLifecycle::Initial.is_mounted());
    assert!(ElementLifecycle::Active.is_mounted());
    assert!(ElementLifecycle::Inactive.is_mounted());
    assert!(!ElementLifecycle::Defunct.is_mounted());
}

// ============================================================================
// update_child() Algorithm Tests
// ============================================================================

#[test]
fn test_update_child_case1_no_new_widget() {
    // Case 1: new_widget is None → unmount old child
    let mut tree = ElementTree::new();
    let tree_arc = Arc::new(RwLock::new(ElementTree::new()));
    tree.set_tree_ref(tree_arc.clone());

    // Create parent element
    let parent_widget = TestWidget { id: 1 };
    let parent_id = tree.set_root(Box::new(parent_widget));

    // Create child element
    let child_widget = TestWidget { id: 2 };
    let child_id = tree.insert_child(parent_id, Box::new(child_widget), 0);
    assert!(child_id.is_some());

    let old_child_id = child_id.unwrap();

    // Update with None → should remove child
    let result = tree.update_child(Some(old_child_id), None, parent_id, 0);
    assert!(result.is_none());

    // Child should be removed from tree
    assert!(tree.get(old_child_id).is_none());
}

#[test]
fn test_update_child_case2_no_old_child() {
    // Case 2: old_child is None → inflate new widget
    let mut tree = ElementTree::new();
    let tree_arc = Arc::new(RwLock::new(ElementTree::new()));
    tree.set_tree_ref(tree_arc.clone());

    let parent_widget = TestWidget { id: 1 };
    let parent_id = tree.set_root(Box::new(parent_widget));

    // Update with no old child → should create new element
    let new_widget = Box::new(TestWidget { id: 2 });
    let result = tree.update_child(None, Some(new_widget), parent_id, 0);

    assert!(result.is_some());
    let new_child_id = result.unwrap();

    // New child should exist in tree
    assert!(tree.get(new_child_id).is_some());
}

#[test]
fn test_update_child_case3_compatible_update() {
    // Case 3a: Compatible types → update in-place
    let mut tree = ElementTree::new();
    let tree_arc = Arc::new(RwLock::new(ElementTree::new()));
    tree.set_tree_ref(tree_arc.clone());

    let parent_widget = TestWidget { id: 1 };
    let parent_id = tree.set_root(Box::new(parent_widget));

    // Create initial child
    let old_widget = TestWidget { id: 2 };
    let old_child_id = tree.insert_child(parent_id, Box::new(old_widget), 0).unwrap();

    // Update with compatible widget (same type, no key)
    let new_widget = Box::new(TestWidget { id: 3 });
    let result = tree.update_child(Some(old_child_id), Some(new_widget), parent_id, 0);

    // Should return same element ID (updated in-place)
    assert_eq!(result, Some(old_child_id));

    // Element should still exist
    assert!(tree.get(old_child_id).is_some());
}

// ============================================================================
// InactiveElements Integration Tests
// ============================================================================

#[test]
fn test_inactive_elements_finalize_tree() {
    let mut tree = ElementTree::new();
    let tree_arc = Arc::new(RwLock::new(ElementTree::new()));
    tree.set_tree_ref(tree_arc.clone());

    let parent_widget = TestWidget { id: 1 };
    let parent_id = tree.set_root(Box::new(parent_widget));

    // Create child
    let child_widget = TestWidget { id: 2 };
    let child_id = tree.insert_child(parent_id, Box::new(child_widget), 0).unwrap();

    // Get element and deactivate it manually
    {
        let mut guard = tree_arc.write();
        if let Some(element) = guard.get_mut(child_id) {
            element.deactivate();
        }
    }

    // Rebuild should clean up inactive elements
    tree.rebuild();

    // After finalize_tree, inactive elements should be removed
    // (This test assumes finalize_tree is called in rebuild)
}

#[test]
fn test_reactivate_element() {
    let mut tree = ElementTree::new();
    let tree_arc = Arc::new(RwLock::new(ElementTree::new()));
    tree.set_tree_ref(tree_arc.clone());

    let parent1_widget = TestWidget { id: 1 };
    let parent1_id = tree.set_root(Box::new(parent1_widget));

    // Create child with TestWidget type
    let child1_widget = TestWidget { id: 2 };
    let child1_id = tree.insert_child(parent1_id, Box::new(child1_widget), 0).unwrap();

    // Use update_child to replace with DIFFERENT widget type (OtherWidget)
    // This will deactivate child1 (TestWidget element) and add it to inactive_elements
    let new_widget = Box::new(OtherWidget { value: 10 });
    let _new_child = tree.update_child(Some(child1_id), Some(new_widget), parent1_id, 0);

    // Verify child1 was deactivated (incompatible types)
    assert_eq!(tree.get(child1_id).unwrap().lifecycle(), ElementLifecycle::Inactive);

    // Create new parent
    let parent2_widget = TestWidget { id: 20 };
    let parent2_id = tree.insert_child(parent1_id, Box::new(parent2_widget), 1).unwrap();

    // Reactivate child1 under new parent (GlobalKey reparenting simulation)
    let reactivated = tree.reactivate_element(child1_id, parent2_id, 0);
    assert!(reactivated);

    // Element should be active again
    assert_eq!(tree.get(child1_id).unwrap().lifecycle(), ElementLifecycle::Active);
}

// ============================================================================
// Dirty Marking Tests
// ============================================================================

#[test]
fn test_activate_marks_dirty() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    element.mount(None, 0);
    element.deactivate();

    // Clear dirty flag
    element.rebuild();
    assert!(!element.is_dirty());

    // Activate should mark dirty
    element.activate();
    assert!(element.is_dirty());
}

#[test]
fn test_update_child_marks_dirty() {
    let mut tree = ElementTree::new();
    let tree_arc = Arc::new(RwLock::new(ElementTree::new()));
    tree.set_tree_ref(tree_arc.clone());

    let parent_widget = TestWidget { id: 1 };
    let parent_id = tree.set_root(Box::new(parent_widget));

    let child_widget = TestWidget { id: 2 };
    let child_id = tree.insert_child(parent_id, Box::new(child_widget), 0).unwrap();

    // Clear dirty flag
    tree.rebuild();

    // Update child
    let new_widget = Box::new(TestWidget { id: 3 });
    tree.update_child(Some(child_id), Some(new_widget), parent_id, 0);

    // Element should be dirty
    assert!(tree.get(child_id).unwrap().is_dirty());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_multiple_deactivate_calls() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    element.mount(None, 0);
    element.deactivate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

    // Multiple deactivate calls should be idempotent
    element.deactivate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);
}

#[test]
fn test_multiple_activate_calls() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    element.mount(None, 0);
    element.deactivate();
    element.activate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);

    // Multiple activate calls should be idempotent
    element.activate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);
}

#[test]
fn test_activate_without_mount() {
    let widget = TestWidget { id: 1 };
    let mut element = widget.into_element();

    // Activate without mount - should still transition to Active
    element.activate();
    assert_eq!(element.lifecycle(), ElementLifecycle::Active);
}

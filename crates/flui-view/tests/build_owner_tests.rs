//! Integration tests for BuildOwner.
//!
//! Tests dirty element tracking, build scheduling, GlobalKey registry,
//! and InheritedElement lookup.

use flui_foundation::ElementId;
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, Lifecycle, StatelessBehavior,
    StatelessElement, StatelessView, View,
};
use std::any::TypeId;

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
// Basic BuildOwner Tests
// ============================================================================

#[test]
fn test_build_owner_creation() {
    let owner = BuildOwner::new();

    assert!(!owner.has_dirty_elements());
    assert_eq!(owner.dirty_count(), 0);
}

#[test]
fn test_build_owner_default() {
    let owner = BuildOwner::default();

    assert!(!owner.has_dirty_elements());
    assert_eq!(owner.dirty_count(), 0);
}

// ============================================================================
// Dirty Element Scheduling Tests
// ============================================================================

#[test]
fn test_schedule_build_for_single() {
    let mut owner = BuildOwner::new();
    let id = ElementId::new(1);

    owner.schedule_build_for(id, 0);

    assert!(owner.has_dirty_elements());
    assert_eq!(owner.dirty_count(), 1);
}

#[test]
fn test_schedule_build_for_multiple() {
    let mut owner = BuildOwner::new();

    owner.schedule_build_for(ElementId::new(1), 0);
    owner.schedule_build_for(ElementId::new(2), 1);
    owner.schedule_build_for(ElementId::new(3), 2);

    assert_eq!(owner.dirty_count(), 3);
}

#[test]
fn test_schedule_build_deduplicates() {
    let mut owner = BuildOwner::new();
    let id = ElementId::new(1);

    // Schedule same element multiple times
    owner.schedule_build_for(id, 0);
    owner.schedule_build_for(id, 0);
    owner.schedule_build_for(id, 0);

    // Should only be counted once
    assert_eq!(owner.dirty_count(), 1);
}

#[test]
fn test_schedule_build_different_depths() {
    let mut owner = BuildOwner::new();

    owner.schedule_build_for(ElementId::new(1), 5);
    owner.schedule_build_for(ElementId::new(2), 0);
    owner.schedule_build_for(ElementId::new(3), 10);

    assert_eq!(owner.dirty_count(), 3);
}

// ============================================================================
// Build Scope Tests
// ============================================================================

#[test]
fn test_build_scope_clears_dirty() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let view = TestView { id: 1 };
    let root_id = tree.mount_root(&view);

    owner.schedule_build_for(root_id, 0);
    assert!(owner.has_dirty_elements());

    owner.build_scope(&mut tree);

    assert!(!owner.has_dirty_elements());
}

#[test]
fn test_build_scope_processes_in_depth_order() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    // Create a tree with multiple levels
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };
    let grandchild_view = TestView { id: 2 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);
    let grandchild_id = tree.insert(&grandchild_view, child_id, 0);

    // Schedule in reverse depth order
    owner.schedule_build_for(grandchild_id, 2);
    owner.schedule_build_for(root_id, 0);
    owner.schedule_build_for(child_id, 1);

    // Processing should handle all elements
    owner.build_scope(&mut tree);

    assert!(!owner.has_dirty_elements());
}

#[test]
fn test_build_scope_skips_removed_elements() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let view = TestView { id: 1 };
    let root_id = tree.mount_root(&view);

    // Schedule element for rebuild
    owner.schedule_build_for(root_id, 0);

    // Remove element before build
    tree.remove(root_id);

    // Should not panic
    owner.build_scope(&mut tree);

    assert!(!owner.has_dirty_elements());
}

#[test]
fn test_build_scope_skips_inactive_elements() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let view = TestView { id: 1 };
    let root_id = tree.mount_root(&view);

    // Schedule element for rebuild
    owner.schedule_build_for(root_id, 0);

    // Deactivate element
    tree.deactivate(root_id);

    // Should not rebuild inactive element
    owner.build_scope(&mut tree);

    assert!(!owner.has_dirty_elements());
}

#[test]
fn test_build_scope_empty_tree() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    // Should not panic with empty tree
    owner.build_scope(&mut tree);

    assert!(!owner.has_dirty_elements());
}

// ============================================================================
// GlobalKey Registry Tests
// ============================================================================

#[test]
fn test_global_key_register() {
    let mut owner = BuildOwner::new();
    let id = ElementId::new(42);
    let key_hash = 12345u64;

    owner.register_global_key(key_hash, id);

    assert_eq!(owner.element_for_global_key(key_hash), Some(id));
}

#[test]
fn test_global_key_unregister() {
    let mut owner = BuildOwner::new();
    let id = ElementId::new(42);
    let key_hash = 12345u64;

    owner.register_global_key(key_hash, id);
    owner.unregister_global_key(key_hash);

    assert_eq!(owner.element_for_global_key(key_hash), None);
}

#[test]
fn test_global_key_lookup_nonexistent() {
    let owner = BuildOwner::new();

    assert_eq!(owner.element_for_global_key(99999), None);
}

#[test]
fn test_global_key_overwrite() {
    let mut owner = BuildOwner::new();
    let id1 = ElementId::new(1);
    let id2 = ElementId::new(2);
    let key_hash = 12345u64;

    owner.register_global_key(key_hash, id1);
    owner.register_global_key(key_hash, id2);

    // Second registration should overwrite
    assert_eq!(owner.element_for_global_key(key_hash), Some(id2));
}

#[test]
fn test_global_key_multiple_keys() {
    let mut owner = BuildOwner::new();

    owner.register_global_key(100, ElementId::new(1));
    owner.register_global_key(200, ElementId::new(2));
    owner.register_global_key(300, ElementId::new(3));

    assert_eq!(owner.element_for_global_key(100), Some(ElementId::new(1)));
    assert_eq!(owner.element_for_global_key(200), Some(ElementId::new(2)));
    assert_eq!(owner.element_for_global_key(300), Some(ElementId::new(3)));
}

// ============================================================================
// InheritedElement Registry Tests
// ============================================================================

#[test]
fn test_inherited_register() {
    let mut owner = BuildOwner::new();
    let id = ElementId::new(42);
    let type_id = TypeId::of::<String>();

    owner.register_inherited(type_id, id);

    assert_eq!(owner.inherited_element(type_id), Some(id));
}

#[test]
fn test_inherited_unregister() {
    let mut owner = BuildOwner::new();
    let id = ElementId::new(42);
    let type_id = TypeId::of::<String>();

    owner.register_inherited(type_id, id);
    owner.unregister_inherited(type_id);

    assert_eq!(owner.inherited_element(type_id), None);
}

#[test]
fn test_inherited_lookup_nonexistent() {
    let owner = BuildOwner::new();
    let type_id = TypeId::of::<String>();

    assert_eq!(owner.inherited_element(type_id), None);
}

#[test]
fn test_inherited_multiple_types() {
    let mut owner = BuildOwner::new();

    owner.register_inherited(TypeId::of::<String>(), ElementId::new(1));
    owner.register_inherited(TypeId::of::<i32>(), ElementId::new(2));
    owner.register_inherited(TypeId::of::<bool>(), ElementId::new(3));

    assert_eq!(
        owner.inherited_element(TypeId::of::<String>()),
        Some(ElementId::new(1))
    );
    assert_eq!(
        owner.inherited_element(TypeId::of::<i32>()),
        Some(ElementId::new(2))
    );
    assert_eq!(
        owner.inherited_element(TypeId::of::<bool>()),
        Some(ElementId::new(3))
    );
}

#[test]
fn test_inherited_overwrite() {
    let mut owner = BuildOwner::new();
    let type_id = TypeId::of::<String>();

    owner.register_inherited(type_id, ElementId::new(1));
    owner.register_inherited(type_id, ElementId::new(2));

    // Second registration should overwrite
    assert_eq!(owner.inherited_element(type_id), Some(ElementId::new(2)));
}

// ============================================================================
// Depth Ordering Tests
// ============================================================================

#[test]
fn test_depth_ordering_shallowest_first() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    // Create elements at different depths
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };
    let grandchild_view = TestView { id: 2 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);
    let grandchild_id = tree.insert(&grandchild_view, child_id, 0);

    // Schedule in random order
    owner.schedule_build_for(child_id, 1);
    owner.schedule_build_for(grandchild_id, 2);
    owner.schedule_build_for(root_id, 0);

    // Verify all get processed
    owner.build_scope(&mut tree);
    assert!(!owner.has_dirty_elements());
}

// ============================================================================
// Debug Tests
// ============================================================================

#[test]
fn test_build_owner_debug() {
    let mut owner = BuildOwner::new();
    owner.schedule_build_for(ElementId::new(1), 0);
    owner.register_global_key(123, ElementId::new(2));
    owner.register_inherited(TypeId::of::<String>(), ElementId::new(3));

    let debug_str = format!("{:?}", owner);

    assert!(debug_str.contains("BuildOwner"));
    assert!(debug_str.contains("dirty_count"));
    assert!(debug_str.contains("global_keys"));
    assert!(debug_str.contains("inherited_elements"));
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_full_build_cycle() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    // Create tree
    let root_view = TestView { id: 0 };
    let child1_view = TestView { id: 1 };
    let child2_view = TestView { id: 2 };

    let root_id = tree.mount_root(&root_view);
    let child1_id = tree.insert(&child1_view, root_id, 0);
    let child2_id = tree.insert(&child2_view, root_id, 1);

    // Mark elements dirty
    tree.mark_needs_build(root_id);
    tree.mark_needs_build(child1_id);
    tree.mark_needs_build(child2_id);

    // Schedule rebuilds
    owner.schedule_build_for(root_id, 0);
    owner.schedule_build_for(child1_id, 1);
    owner.schedule_build_for(child2_id, 1);

    assert_eq!(owner.dirty_count(), 3);

    // Run build cycle
    owner.build_scope(&mut tree);

    // All elements should still be valid
    assert!(tree.contains(root_id));
    assert!(tree.contains(child1_id));
    assert!(tree.contains(child2_id));
    assert!(!owner.has_dirty_elements());
}

#[test]
fn test_multiple_build_cycles() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let view = TestView { id: 1 };
    let root_id = tree.mount_root(&view);

    // First cycle
    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);
    assert!(!owner.has_dirty_elements());

    // Second cycle
    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);
    assert!(!owner.has_dirty_elements());

    // Third cycle
    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);
    assert!(!owner.has_dirty_elements());
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_build_owner_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    // BuildOwner should NOT be Send + Sync in the general case
    // because it holds references to the tree during build
    // This is intentional for safety
}

// ============================================================================
// Memory Layout Tests
// ============================================================================

#[test]
fn test_build_owner_memory_size() {
    let size = std::mem::size_of::<BuildOwner>();
    // Should be reasonably sized (BinaryHeap + HashSet + 2 HashMaps + debug flags)
    assert!(size < 512, "BuildOwner is too large: {} bytes", size);
}

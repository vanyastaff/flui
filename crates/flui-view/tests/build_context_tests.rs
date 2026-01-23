//! Integration tests for BuildContext and ElementBuildContext.
//!
//! Tests the BuildContext trait implementation, dependency tracking,
//! ancestor lookups, and rebuild scheduling.

use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementBase, ElementBuildContext,
    ElementBuildContextBuilder, ElementTree, Lifecycle, StatelessBehavior, StatelessElement,
    StatelessView, View,
};
use parking_lot::RwLock;
use std::any::TypeId;
use std::sync::Arc;

// ============================================================================
// Test Views
// ============================================================================

#[derive(Clone)]
struct SimpleView {
    #[allow(dead_code)]
    name: String,
}

impl StatelessView for SimpleView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for SimpleView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

#[derive(Clone)]
struct ChildView {
    #[allow(dead_code)]
    parent_name: String,
}

impl StatelessView for ChildView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for ChildView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_tree_and_owner() -> (Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    (tree, owner)
}

// ============================================================================
// ElementBuildContext Creation Tests
// ============================================================================

#[test]
fn test_context_creation_for_root() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "root".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone());

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.element_id(), root_id);
    assert_eq!(ctx.depth(), 0);
    assert!(ctx.mounted());
}

#[test]
fn test_context_creation_for_child() {
    let (tree, owner) = create_tree_and_owner();

    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let child_view = ChildView {
        parent_name: "root".to_string(),
    };

    let root_id = tree.write().mount_root(&root_view);
    let child_id = tree.write().insert(&child_view, root_id, 0);

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone());

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.element_id(), child_id);
    assert_eq!(ctx.depth(), 1);
    assert!(ctx.mounted());
}

#[test]
fn test_context_creation_nonexistent_element() {
    let (tree, owner) = create_tree_and_owner();

    let fake_id = flui_foundation::ElementId::new(999);
    let ctx = ElementBuildContext::for_element(fake_id, tree, owner);

    assert!(ctx.is_none());
}

#[test]
fn test_context_builder() {
    let (builder, tree, _owner) = ElementBuildContextBuilder::new().with_new_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = builder.build_for(root_id);

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.element_id(), root_id);
}

#[test]
fn test_context_builder_without_tree() {
    let builder = ElementBuildContextBuilder::new();
    let fake_id = flui_foundation::ElementId::new(1);

    let ctx = builder.build_for(fake_id);

    assert!(ctx.is_none());
}

// ============================================================================
// BuildContext Trait Methods Tests
// ============================================================================

#[test]
fn test_element_id() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    assert_eq!(ctx.element_id(), root_id);
}

#[test]
fn test_depth_root() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "root".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    assert_eq!(ctx.depth(), 0);
}

#[test]
fn test_depth_nested() {
    let (tree, owner) = create_tree_and_owner();

    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let child_view = ChildView {
        parent_name: "root".to_string(),
    };
    let grandchild_view = SimpleView {
        name: "grandchild".to_string(),
    };

    let root_id = tree.write().mount_root(&root_view);
    let child_id = tree.write().insert(&child_view, root_id, 0);
    let grandchild_id = tree.write().insert(&grandchild_view, child_id, 0);

    let ctx = ElementBuildContext::for_element(grandchild_id, tree, owner).unwrap();

    assert_eq!(ctx.depth(), 2);
}

#[test]
fn test_mounted_active_element() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    assert!(ctx.mounted());
}

#[test]
fn test_mounted_deactivated_element() {
    let (tree, _owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);
    tree.write().deactivate(root_id);

    // Note: Context is created with mounted status at creation time
    // After deactivation, we need to check the element directly
    let tree_guard = tree.read();
    let node = tree_guard.get(root_id).unwrap();
    assert_eq!(node.element().lifecycle(), Lifecycle::Inactive);
}

// ============================================================================
// mark_needs_build Tests
// ============================================================================

#[test]
fn test_mark_needs_build() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone()).unwrap();

    assert!(!owner.read().has_dirty_elements());

    ctx.mark_needs_build();

    assert!(owner.read().has_dirty_elements());
    assert_eq!(owner.read().dirty_count(), 1);
}

#[test]
fn test_mark_needs_build_multiple_times() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone()).unwrap();

    // Mark multiple times - should deduplicate
    ctx.mark_needs_build();
    ctx.mark_needs_build();
    ctx.mark_needs_build();

    assert_eq!(owner.read().dirty_count(), 1);
}

#[test]
fn test_mark_needs_build_multiple_elements() {
    let (tree, owner) = create_tree_and_owner();

    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let child_view = ChildView {
        parent_name: "root".to_string(),
    };

    let root_id = tree.write().mount_root(&root_view);
    let child_id = tree.write().insert(&child_view, root_id, 0);

    let root_ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone()).unwrap();
    let child_ctx =
        ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    root_ctx.mark_needs_build();
    child_ctx.mark_needs_build();

    assert_eq!(owner.read().dirty_count(), 2);
}

// ============================================================================
// visit_ancestor_elements Tests
// ============================================================================

#[test]
fn test_visit_ancestor_elements_empty() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "root".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    let mut ancestors = Vec::new();
    ctx.visit_ancestor_elements(&mut |id| {
        ancestors.push(id);
        true
    });

    // Root has no ancestors
    assert!(ancestors.is_empty());
}

#[test]
fn test_visit_ancestor_elements_chain() {
    let (tree, owner) = create_tree_and_owner();

    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let child_view = ChildView {
        parent_name: "root".to_string(),
    };
    let grandchild_view = SimpleView {
        name: "grandchild".to_string(),
    };

    let root_id = tree.write().mount_root(&root_view);
    let child_id = tree.write().insert(&child_view, root_id, 0);
    let grandchild_id = tree.write().insert(&grandchild_view, child_id, 0);

    let ctx = ElementBuildContext::for_element(grandchild_id, tree, owner).unwrap();

    let mut ancestors = Vec::new();
    ctx.visit_ancestor_elements(&mut |id| {
        ancestors.push(id);
        true
    });

    assert_eq!(ancestors.len(), 2);
    assert_eq!(ancestors[0], child_id);
    assert_eq!(ancestors[1], root_id);
}

#[test]
fn test_visit_ancestor_elements_early_stop() {
    let (tree, owner) = create_tree_and_owner();

    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let child_view = ChildView {
        parent_name: "root".to_string(),
    };
    let grandchild_view = SimpleView {
        name: "grandchild".to_string(),
    };

    let root_id = tree.write().mount_root(&root_view);
    let child_id = tree.write().insert(&child_view, root_id, 0);
    let grandchild_id = tree.write().insert(&grandchild_view, child_id, 0);

    let ctx = ElementBuildContext::for_element(grandchild_id, tree, owner).unwrap();

    let mut ancestors = Vec::new();
    ctx.visit_ancestor_elements(&mut |id| {
        ancestors.push(id);
        false // Stop after first
    });

    assert_eq!(ancestors.len(), 1);
    assert_eq!(ancestors[0], child_id);
}

// ============================================================================
// find_ancestor_element Tests
// ============================================================================

#[test]
fn test_find_ancestor_element_found() {
    let (tree, owner) = create_tree_and_owner();

    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let child_view = ChildView {
        parent_name: "root".to_string(),
    };

    let root_id = tree.write().mount_root(&root_view);
    let child_id = tree.write().insert(&child_view, root_id, 0);

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let ancestor = ctx.find_ancestor_element(TypeId::of::<SimpleView>());

    assert_eq!(ancestor, Some(root_id));
}

#[test]
fn test_find_ancestor_element_not_found() {
    let (tree, owner) = create_tree_and_owner();

    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let child_view = ChildView {
        parent_name: "root".to_string(),
    };

    let root_id = tree.write().mount_root(&root_view);
    let child_id = tree.write().insert(&child_view, root_id, 0);

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    // Look for a type that doesn't exist in ancestors
    struct NonExistentView;
    let ancestor = ctx.find_ancestor_element(TypeId::of::<NonExistentView>());

    assert!(ancestor.is_none());
}

#[test]
fn test_find_ancestor_element_from_root() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "root".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    // Root has no ancestors
    let ancestor = ctx.find_ancestor_element(TypeId::of::<SimpleView>());

    assert!(ancestor.is_none());
}

// ============================================================================
// is_building Tests
// ============================================================================

#[test]
fn test_is_building_default_false() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    assert!(!ctx.is_building());
}

#[cfg(debug_assertions)]
#[test]
fn test_set_building_flag() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let mut ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    ctx.set_building(true);
    assert!(ctx.is_building());

    ctx.set_building(false);
    assert!(!ctx.is_building());
}

// ============================================================================
// owner() Tests
// ============================================================================

#[test]
fn test_owner_returns_none() {
    // Note: owner() returns None because we can't return reference through RwLock
    // This is a design limitation that may need addressing
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    assert!(ctx.owner().is_none());
}

#[test]
fn test_build_owner_access_via_method() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner.clone()).unwrap();

    // Access owner through the direct method
    let ctx_owner = ctx.build_owner();
    assert!(Arc::ptr_eq(ctx_owner, &owner));
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_context_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ElementBuildContext>();
}

#[test]
fn test_context_builder_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ElementBuildContextBuilder>();
}

// ============================================================================
// BuildContextExt Tests
// ============================================================================

#[test]
fn test_depend_on_returns_none() {
    // depend_on returns None because of lifetime issues with RwLock
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    let result: Option<&String> = ctx.depend_on::<String>();
    assert!(result.is_none());
}

#[test]
fn test_get_returns_none() {
    // get returns None because of lifetime issues with RwLock
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    let result: Option<&i32> = ctx.get::<i32>();
    assert!(result.is_none());
}

// ============================================================================
// Debug Tests
// ============================================================================

#[test]
fn test_context_debug() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree.write().mount_root(&view);

    let ctx = ElementBuildContext::for_element(root_id, tree, owner).unwrap();

    let debug_str = format!("{:?}", ctx);
    assert!(debug_str.contains("ElementBuildContext"));
    assert!(debug_str.contains("element_id"));
    assert!(debug_str.contains("depth"));
    assert!(debug_str.contains("mounted"));
}

#[test]
fn test_builder_debug() {
    let builder = ElementBuildContextBuilder::new();
    let debug_str = format!("{:?}", builder);
    assert!(debug_str.contains("ElementBuildContextBuilder"));
}

// ============================================================================
// Deep Tree Tests
// ============================================================================

#[test]
fn test_deep_tree_ancestor_traversal() {
    let (tree, owner) = create_tree_and_owner();

    // Build a chain of 10 elements
    let root_view = SimpleView {
        name: "root".to_string(),
    };
    let root_id = tree.write().mount_root(&root_view);

    let mut parent_id = root_id;
    let mut all_ids = vec![root_id];

    for i in 1..10 {
        let view = SimpleView {
            name: format!("node_{}", i),
        };
        let child_id = tree.write().insert(&view, parent_id, 0);
        all_ids.push(child_id);
        parent_id = child_id;
    }

    // Get context for deepest element
    let deepest_id = *all_ids.last().unwrap();
    let ctx = ElementBuildContext::for_element(deepest_id, tree, owner).unwrap();

    // Should have depth 9 (0-indexed from root)
    assert_eq!(ctx.depth(), 9);

    // Visit all ancestors
    let mut ancestors = Vec::new();
    ctx.visit_ancestor_elements(&mut |id| {
        ancestors.push(id);
        true
    });

    // Should find all 9 ancestors
    assert_eq!(ancestors.len(), 9);

    // Verify they're in correct order (parent first, root last)
    for (i, &ancestor_id) in ancestors.iter().enumerate() {
        // ancestors[0] should be all_ids[8] (parent of deepest)
        // ancestors[8] should be all_ids[0] (root)
        let expected_idx = 8 - i;
        assert_eq!(ancestor_id, all_ids[expected_idx]);
    }
}

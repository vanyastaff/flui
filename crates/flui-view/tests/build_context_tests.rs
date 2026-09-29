//! Integration tests for BuildContext and ElementBuildContext.
//!
//! Tests the BuildContext trait implementation, dependency tracking,
//! ancestor lookups, and rebuild scheduling.

// ADR-0027: ElementBuildContext's current test/prod seam still takes
// Arc<RwLock<ElementTree/BuildOwner>>. The owner graph is !Send; do not restore
// Send + Sync to satisfy clippy. Future UiRealm/Rc migration should remove this.
#![expect(clippy::arc_with_non_send_sync)]

use std::{any::TypeId, sync::Arc};

use flui_view::{
    BuildContext, BuildOwner, ElementBuildContext, ElementTree, IntoView, LifecycleContext,
    StatelessView, View, ViewExt,
};
use parking_lot::RwLock;

// ============================================================================
// Test Views
// ============================================================================

#[derive(Clone)]
struct SimpleView {
    #[expect(dead_code)]
    name: String,
}

impl StatelessView for SimpleView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for SimpleView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct ChildView {
    #[expect(dead_code)]
    parent_name: String,
}

impl StatelessView for ChildView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for ChildView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
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

// ============================================================================
// BuildContext Trait Methods Tests
// ============================================================================

// ============================================================================
// mark_needs_build Tests
// ============================================================================

// ============================================================================
// visit_ancestor_elements Tests
// ============================================================================

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

    let root_id = tree
        .write()
        .mount_root(&root_view, &mut owner.write().element_owner_mut());
    let child_id = tree.write().insert(
        &child_view,
        root_id,
        0,
        &mut owner.write().element_owner_mut(),
    );
    let grandchild_id = tree.write().insert(
        &grandchild_view,
        child_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

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

    let root_id = tree
        .write()
        .mount_root(&root_view, &mut owner.write().element_owner_mut());
    let child_id = tree.write().insert(
        &child_view,
        root_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let ancestor = ctx.find_ancestor_element(TypeId::of::<SimpleView>());

    assert_eq!(ancestor, Some(root_id));
}

// ============================================================================
// is_building Tests
// ============================================================================

// ============================================================================
// rebuild_handle() Tests
// ============================================================================

/// `ElementBuildContext` mints a REAL handle — it owns the `BuildOwner` `Arc`.
/// This replaces the old `owner() -> None` stub, which reported a design
/// limitation instead of providing the capability callers actually needed.
#[test]
fn rebuild_handle_from_element_build_context_is_active_and_bound() {
    let (tree, owner) = create_tree_and_owner();

    let view = SimpleView {
        name: "test".to_string(),
    };
    let root_id = tree
        .write()
        .mount_root(&view, &mut owner.write().element_owner_mut());

    let ctx = ElementBuildContext::for_element(root_id, tree, Arc::clone(&owner)).unwrap();
    let handle = ctx.rebuild_handle();

    assert!(handle.is_active());
    assert_eq!(handle.element_id(), Some(root_id));

    // Scheduling routes into the same inbox `build_scope` drains.
    handle.schedule(flui_view::RebuildReason::StateChange);
    assert_eq!(owner.read().pending_external_builds(), 1);
}

// ============================================================================
// Ownership Tests
// ============================================================================

// ============================================================================
// BuildContextExt Tests
// ============================================================================

// ============================================================================
// Debug Tests
// ============================================================================

// ============================================================================
// Deep Tree Tests
// ============================================================================

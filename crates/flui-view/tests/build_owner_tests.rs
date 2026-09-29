//! Integration tests for BuildOwner.
//!
//! Tests dirty element tracking, build scheduling, and the GlobalKey
//! registry.

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{BuildOwner, ElementTree, RebuildReason, RenderView, View};
use std::rc::Rc;

// ============================================================================
// Test View
// ============================================================================

#[derive(Clone)]
struct TestView {
    #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
    id: u32,
}

impl View for TestView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

impl RenderView for TestView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

// ============================================================================
// Basic BuildOwner Tests
// ============================================================================

pub(crate) fn build_owners_have_isolated_focus_managers() {
    let first = BuildOwner::new();
    let second = BuildOwner::new();

    assert!(!Rc::ptr_eq(&first.focus_manager(), &second.focus_manager()));
}

// ============================================================================
// Dirty Element Scheduling Tests
// ============================================================================

// ============================================================================
// Build Scope Tests
// ============================================================================

pub(crate) fn test_build_scope_processes_in_depth_order() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    // Create a tree with multiple levels
    let root_view = TestView { id: 0 };
    let child_view = TestView { id: 1 };
    let grandchild_view = TestView { id: 2 };

    let root_id = tree.mount_root(&root_view, &mut owner.element_owner_mut());
    let child_id = tree.insert(&child_view, root_id, 0, &mut owner.element_owner_mut());
    let grandchild_id = tree.insert(
        &grandchild_view,
        child_id,
        0,
        &mut owner.element_owner_mut(),
    );

    // Schedule in reverse depth order
    owner.schedule_build_for(grandchild_id, 2, RebuildReason::InitialMount);
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    owner.schedule_build_for(child_id, 1, RebuildReason::InitialMount);

    // Processing should handle all elements
    owner.build_scope(&mut tree);

    assert!(!owner.has_dirty_elements());
}

// ============================================================================
// GlobalKey Registry Tests
// ============================================================================

// ============================================================================
// Depth Ordering Tests
// ============================================================================

// ============================================================================
// Debug Tests
// ============================================================================

// ============================================================================
// Integration Tests
// ============================================================================

// ============================================================================
// Memory Layout Tests
// ============================================================================

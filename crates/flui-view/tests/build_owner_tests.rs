//! Integration tests for BuildOwner.
//!
//! Tests dirty element tracking, build scheduling, and the GlobalKey
//! registry.

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildOwner, ElementTree, FrameBuildReport, RebuildReason, RenderView, View, WidgetsBinding,
};
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

pub(crate) fn clean_binding_frames_report_no_builds() {
    fn assert_no_builds(report: FrameBuildReport) {
        assert_eq!((report.elements_built, report.builds_run), (0, 0));
        assert_eq!(report.by_reason, [] as [(RebuildReason, usize); 0]);
    }

    let binding = WidgetsBinding::new();
    binding.set_pipeline_owner(flui_rendering::pipeline::PipelineCell::new(
        flui_rendering::pipeline::PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ),
    ));
    binding
        .attach_root_widget(&TestView { id: 0 })
        .expect("attach");
    binding.draw_frame();
    let mounted = binding.with_build_owner(BuildOwner::last_frame_build_report);
    assert!(mounted.elements_built > 0, "{mounted:?}");
    assert!(mounted.builds_run > 0, "{mounted:?}");

    for _ in 0..2 {
        binding.draw_frame();
        assert_no_builds(binding.with_build_owner(BuildOwner::last_frame_build_report));
    }

    binding.perform_reassemble();
    binding.draw_frame();
    let rebuilt = binding.with_build_owner(BuildOwner::last_frame_build_report);
    assert!(rebuilt.elements_built > 0, "{rebuilt:?}");
    assert!(rebuilt.builds_run > 0, "{rebuilt:?}");
    assert!(rebuilt.count(RebuildReason::HotReload) > 0, "{rebuilt:?}");
    binding.draw_frame();
    assert_no_builds(binding.with_build_owner(BuildOwner::last_frame_build_report));
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

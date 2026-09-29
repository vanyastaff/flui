//! Integration tests for BuildOwner.
//!
//! Tests dirty element tracking, build scheduling, and the GlobalKey
//! registry.

use flui_foundation::ElementId;
use flui_interaction::{InteractionLane, PointerTarget};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildOwner, ElementTree, GlobalKey, RebuildReason, RenderObjectContext, RenderView, View,
};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{rc::Rc, sync::Arc};

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

#[test]
fn build_owners_have_isolated_focus_managers() {
    let first = BuildOwner::new();
    let second = BuildOwner::new();

    assert!(!Rc::ptr_eq(&first.focus_manager(), &second.focus_manager()));
}

#[derive(Clone)]
struct InteractionContextView {
    create_count: Arc<AtomicUsize>,
    update_count: Arc<AtomicUsize>,
    unmount_count: Arc<AtomicUsize>,
    target: Arc<RwLock<Option<PointerTarget>>>,
}

impl View for InteractionContextView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

impl RenderView for InteractionContextView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let target = ctx
            .register_pointer(|_| {})
            .expect("mount runs with the BuildOwner interaction capability active");
        *self.target.write() = Some(target);
        self.create_count.fetch_add(1, Ordering::Relaxed);
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let target = (*self.target.read()).expect("create stored the target before update");
        ctx.replace_pointer(target, |_| {})
            .expect("update runs with the same BuildOwner interaction capability active");
        self.update_count.fetch_add(1, Ordering::Relaxed);
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn did_unmount_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        let target = (*self.target.read()).expect("create stored the target before unmount");
        ctx.unregister_pointer(target)
            .expect("unmount runs with the same BuildOwner interaction capability active");
        self.unmount_count.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn render_object_context_reaches_create_and_update_from_build_owner() {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let create_count = Arc::new(AtomicUsize::new(0));
    let update_count = Arc::new(AtomicUsize::new(0));
    let unmount_count = Arc::new(AtomicUsize::new(0));
    let target = Arc::new(RwLock::new(None));
    let first = InteractionContextView {
        create_count: Arc::clone(&create_count),
        update_count: Arc::clone(&update_count),
        unmount_count: Arc::clone(&unmount_count),
        target: Arc::clone(&target),
    };
    let second = first.clone();
    let mut owner = BuildOwner::new();
    owner.set_interaction_dispatch_handle(handle.clone());
    let mut tree = ElementTree::new();

    lane.enter(|| {
        let root = tree.mount_root_with_pipeline_owner(
            &first,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        tree.update(root, &second, &mut owner.element_owner_mut());
        tree.remove(root, &mut owner.element_owner_mut());
        let target = (*target.read()).expect("create stored a target");
        let entry = flui_interaction::HitTestEntry::new(flui_foundation::RenderId::new(1))
            .pointer_target(target);
        let resolution = handle
            .resolve_pointer_route(&[entry])
            .expect("resolution itself still succeeds for same-lane targets");
        assert_eq!(
            resolution.misses().len(),
            1,
            "unmount unregisters the target from future route resolution"
        );
    });

    assert_eq!(create_count.load(Ordering::Relaxed), 1);
    assert_eq!(update_count.load(Ordering::Relaxed), 1);
    assert_eq!(unmount_count.load(Ordering::Relaxed), 1);
    assert!(
        target.read().is_some(),
        "create stores the data-only target minted from the owner lane"
    );
}

// ============================================================================
// Dirty Element Scheduling Tests
// ============================================================================

#[test]
fn test_schedule_build_deduplicates() {
    let mut owner = BuildOwner::new();
    let id = ElementId::new(1);

    // Schedule same element multiple times
    owner.schedule_build_for(id, 0, RebuildReason::StateChange);
    owner.schedule_build_for(id, 0, RebuildReason::AnimationTick);
    owner.schedule_build_for(id, 0, RebuildReason::DependencyChange);

    // Rebuild once, without erasing the three independent causes.
    assert_eq!(owner.dirty_count(), 1);
    let reasons = owner
        .pending_rebuild_reasons(id)
        .expect("the element is still queued");
    assert_eq!(reasons.len(), 3);
    assert!(reasons.contains(RebuildReason::StateChange));
    assert!(reasons.contains(RebuildReason::AnimationTick));
    assert!(reasons.contains(RebuildReason::DependencyChange));
}

// ============================================================================
// Build Scope Tests
// ============================================================================

#[test]
fn test_build_scope_processes_in_depth_order() {
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

#[test]
fn test_build_scope_skips_inactive_elements() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let view = TestView { id: 1 };
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

    // Schedule element for rebuild
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);

    // Deactivate element
    tree.deactivate(root_id, &mut owner.element_owner_mut());

    // Should not rebuild inactive element
    owner.build_scope(&mut tree);

    assert!(!owner.has_dirty_elements());
}

// ============================================================================
// GlobalKey Registry Tests
// ============================================================================

/// Two keys of DIFFERENT `T` can share a hash-space neighbourhood, and the
/// registry must keep them apart by identity rather than by hash: a
/// `GlobalKey<u8>` never answers a `GlobalKey<i64>`'s lookup even when both
/// were minted adjacently.
#[test]
fn distinct_key_types_never_answer_each_others_lookups() {
    let mut owner = BuildOwner::new();
    let numeric = GlobalKey::<u8>::new();
    let textual = GlobalKey::<String>::new();

    owner.register_global_key(&numeric, ElementId::new(1));
    owner.register_global_key(&textual, ElementId::new(2));

    assert_eq!(
        owner.element_for_global_key(&numeric),
        Some(ElementId::new(1))
    );
    assert_eq!(
        owner.element_for_global_key(&textual),
        Some(ElementId::new(2))
    );

    owner.unregister_global_key(&numeric);
    assert_eq!(owner.element_for_global_key(&numeric), None);
    assert_eq!(
        owner.element_for_global_key(&textual),
        Some(ElementId::new(2)),
        "unregistering one key must not disturb the other",
    );
}

// ============================================================================
// Depth Ordering Tests
// ============================================================================

// ============================================================================
// Debug Tests
// ============================================================================

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_reassemble_marks_all_live_elements_dirty() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let root_id = tree.mount_root(&TestView { id: 1 }, &mut owner.element_owner_mut());
    let _child1 = tree.insert(
        &TestView { id: 2 },
        root_id,
        0,
        &mut owner.element_owner_mut(),
    );
    let _child2 = tree.insert(
        &TestView { id: 3 },
        root_id,
        1,
        &mut owner.element_owner_mut(),
    );

    owner.build_scope(&mut tree);
    assert!(!owner.has_dirty_elements());

    owner.reassemble(&mut tree);
    assert_eq!(owner.dirty_count(), tree.len());

    // The heap count alone is not the contract: a queued entry whose element's
    // own dirty flag is unset is skipped by the drain's guard and rebuilds
    // nothing. Assert the flag too, then prove the drain consumes the queue.
    for (id, node) in tree.iter_nodes() {
        assert!(
            node.element().is_dirty(),
            "reassemble must set each element's dirty flag, not only queue it"
        );
        let _ = id;
    }
    owner.build_scope(&mut tree);
    assert!(
        !owner.has_dirty_elements(),
        "the queued reassemble work must actually drain (no entry left skipped)"
    );
}

// ============================================================================
// Memory Layout Tests
// ============================================================================

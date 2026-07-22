//! Integration tests for BuildOwner.
//!
//! Tests dirty element tracking, build scheduling, and the GlobalKey
//! registry.

use flui_foundation::ElementId;
use flui_interaction::{InteractionLane, PointerTarget};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildOwner, ElementTree, RenderObjectContext, RenderObjectContextError, RenderView, View,
};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    ) {
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
    ) {
        let target = (*self.target.read()).expect("create stored the target before update");
        ctx.replace_pointer(target, |_| {})
            .expect("update runs with the same BuildOwner interaction capability active");
        self.update_count.fetch_add(1, Ordering::Relaxed);
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
fn detached_render_object_context_reports_inactive_realm() {
    let ctx = RenderObjectContext::detached();

    assert!(matches!(
        ctx.register_pointer(|_| {}),
        Err(RenderObjectContextError::InteractionUnavailable)
    ));
}

#[test]
fn render_object_context_reaches_create_and_update_from_build_owner() {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
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
            Some(Arc::clone(&pipeline)),
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
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

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

    let root_id = tree.mount_root(&root_view, &mut owner.element_owner_mut());
    let child_id = tree.insert(&child_view, root_id, 0, &mut owner.element_owner_mut());
    let grandchild_id = tree.insert(
        &grandchild_view,
        child_id,
        0,
        &mut owner.element_owner_mut(),
    );

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
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

    // Schedule element for rebuild
    owner.schedule_build_for(root_id, 0);

    // Remove element before build
    tree.remove(root_id, &mut owner.element_owner_mut());

    // Should not panic
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

    let root_id = tree.mount_root(&root_view, &mut owner.element_owner_mut());
    let child_id = tree.insert(&child_view, root_id, 0, &mut owner.element_owner_mut());
    let grandchild_id = tree.insert(
        &grandchild_view,
        child_id,
        0,
        &mut owner.element_owner_mut(),
    );

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

    let debug_str = format!("{owner:?}");

    assert!(debug_str.contains("BuildOwner"));
    assert!(debug_str.contains("dirty_count"));
    assert!(debug_str.contains("global_keys"));
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

    let root_id = tree.mount_root(&root_view, &mut owner.element_owner_mut());
    let child1_id = tree.insert(&child1_view, root_id, 0, &mut owner.element_owner_mut());
    let child2_id = tree.insert(&child2_view, root_id, 1, &mut owner.element_owner_mut());

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
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

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

    owner.reassemble(&tree);
    assert_eq!(owner.dirty_count(), tree.len());
}

// ============================================================================
// Memory Layout Tests
// ============================================================================

#[test]
fn test_build_owner_memory_size() {
    let size = std::mem::size_of::<BuildOwner>();
    // Should be reasonably sized (BinaryHeap + HashSet + HashMap + debug flags)
    assert!(size < 512, "BuildOwner is too large: {size} bytes");
}

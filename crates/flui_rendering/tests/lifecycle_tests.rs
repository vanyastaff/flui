//! Integration tests for lifecycle module.
//!
//! These tests verify the Flutter-style lifecycle management works correctly.

use flui_rendering::lifecycle::{
    BaseRenderObject, DirtyFlags, RelayoutBoundary, RenderObjectFlags, RenderObjectState,
};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;
use std::sync::Arc;

// ============================================================================
// DirtyFlags Tests
// ============================================================================

#[test]
fn test_dirty_flags_memory_layout() {
    // DirtyFlags should be 1 byte (vs 6-8+ bytes of booleans in Flutter)
    assert_eq!(std::mem::size_of::<DirtyFlags>(), 1);
}

#[test]
fn test_dirty_flags_initial_state() {
    let flags = DirtyFlags::initial();

    // New render objects need layout and paint
    assert!(flags.needs_layout());
    assert!(flags.needs_paint());

    // Other flags are false by default
    assert!(!flags.needs_compositing_bits_update());
    assert!(!flags.needs_semantics_update());
    assert!(!flags.is_repaint_boundary());
    assert!(!flags.was_repaint_boundary());
}

#[test]
fn test_dirty_flags_manipulation() {
    let mut flags = DirtyFlags::empty();

    // Set individual flags
    flags.insert(DirtyFlags::NEEDS_LAYOUT);
    assert!(flags.needs_layout());

    flags.insert(DirtyFlags::NEEDS_PAINT);
    assert!(flags.needs_paint());

    // Clear flags
    flags.remove(DirtyFlags::NEEDS_LAYOUT);
    assert!(!flags.needs_layout());
    assert!(flags.needs_paint()); // Other flag unchanged

    // Set multiple flags at once
    flags.insert(DirtyFlags::NEEDS_COMPOSITING_BITS_UPDATE | DirtyFlags::NEEDS_SEMANTICS_UPDATE);
    assert!(flags.needs_compositing_bits_update());
    assert!(flags.needs_semantics_update());
}

#[test]
fn test_dirty_flags_repaint_boundary() {
    let mut flags = DirtyFlags::empty();

    // Set repaint boundary
    flags.insert(DirtyFlags::IS_REPAINT_BOUNDARY);
    assert!(flags.is_repaint_boundary());
    assert!(!flags.was_repaint_boundary());

    // Track previous state
    flags.insert(DirtyFlags::WAS_REPAINT_BOUNDARY);
    assert!(flags.was_repaint_boundary());

    // Change boundary state
    flags.remove(DirtyFlags::IS_REPAINT_BOUNDARY);
    assert!(!flags.is_repaint_boundary());
    assert!(flags.was_repaint_boundary()); // Previous state preserved
}

// ============================================================================
// RelayoutBoundary Tests
// ============================================================================

#[test]
fn test_relayout_boundary_memory_layout() {
    // RelayoutBoundary should be 1 byte
    assert_eq!(std::mem::size_of::<RelayoutBoundary>(), 1);
}

#[test]
fn test_relayout_boundary_states() {
    // Unknown (initial state, like null in Flutter)
    let unknown = RelayoutBoundary::Unknown;
    assert!(!unknown.is_boundary());
    assert!(!unknown.is_known());

    // Yes (is a relayout boundary)
    let yes = RelayoutBoundary::Yes;
    assert!(yes.is_boundary());
    assert!(yes.is_known());

    // No (not a relayout boundary)
    let no = RelayoutBoundary::No;
    assert!(!no.is_boundary());
    assert!(no.is_known());
}

#[test]
fn test_relayout_boundary_option_conversion() {
    // From Option<bool>
    assert_eq!(
        RelayoutBoundary::from_option(None),
        RelayoutBoundary::Unknown
    );
    assert_eq!(
        RelayoutBoundary::from_option(Some(true)),
        RelayoutBoundary::Yes
    );
    assert_eq!(
        RelayoutBoundary::from_option(Some(false)),
        RelayoutBoundary::No
    );

    // To Option<bool>
    assert_eq!(RelayoutBoundary::Unknown.to_option(), None);
    assert_eq!(RelayoutBoundary::Yes.to_option(), Some(true));
    assert_eq!(RelayoutBoundary::No.to_option(), Some(false));
}

// ============================================================================
// RenderObjectFlags Tests
// ============================================================================

#[test]
fn test_render_object_flags_memory_layout() {
    // RenderObjectFlags should be 2 bytes (dirty flags + relayout boundary)
    assert_eq!(std::mem::size_of::<RenderObjectFlags>(), 2);
}

#[test]
fn test_render_object_flags_new() {
    let flags = RenderObjectFlags::new();

    // Initial state matches Flutter
    assert!(flags.needs_layout());
    assert!(flags.needs_paint());
    assert!(!flags.needs_compositing_bits_update());
    assert_eq!(flags.relayout_boundary(), RelayoutBoundary::Unknown);
}

#[test]
fn test_render_object_flags_marking() {
    let mut flags = RenderObjectFlags::new();

    // Clear and mark layout
    flags.clear_needs_layout();
    assert!(!flags.needs_layout());

    flags.mark_needs_layout();
    assert!(flags.needs_layout());

    // Clear and mark paint
    flags.clear_needs_paint();
    assert!(!flags.needs_paint());

    flags.mark_needs_paint();
    assert!(flags.needs_paint());
}

#[test]
fn test_render_object_flags_boundaries() {
    let mut flags = RenderObjectFlags::new();

    // Relayout boundary
    flags.set_relayout_boundary(RelayoutBoundary::Yes);
    assert!(flags.is_relayout_boundary());

    flags.set_relayout_boundary(RelayoutBoundary::No);
    assert!(!flags.is_relayout_boundary());

    flags.clear_relayout_boundary();
    assert_eq!(flags.relayout_boundary(), RelayoutBoundary::Unknown);

    // Repaint boundary
    flags.set_repaint_boundary(true);
    assert!(flags.is_repaint_boundary());
    assert!(!flags.was_repaint_boundary());

    flags.sync_was_repaint_boundary();
    assert!(flags.was_repaint_boundary());
}

// ============================================================================
// RenderObjectState Tests
// ============================================================================

#[test]
fn test_render_object_state_initial() {
    let state = RenderObjectState::new();

    // Initial state
    assert!(!state.is_attached());
    assert!(state.needs_layout());
    assert!(state.needs_paint());
    assert_eq!(state.depth(), 0);
    assert!(state.owner().is_none());
    assert!(!state.is_disposed());
}

#[test]
fn test_render_object_state_attach_detach() {
    let mut state = RenderObjectState::with_node_id(1);
    let owner = Arc::new(RwLock::new(PipelineOwner::new()));

    // Initially detached
    assert!(!state.is_attached());

    // Attach
    state.attach(owner.clone());
    assert!(state.is_attached());
    assert!(state.owner().is_some());

    // Detach
    state.detach();
    assert!(!state.is_attached());
    assert!(state.owner().is_none());
}

#[test]
fn test_render_object_state_dirty_scheduling() {
    let mut state = RenderObjectState::with_node_id(1);
    let owner = Arc::new(RwLock::new(PipelineOwner::new()));

    state.attach(owner.clone());

    // Clear initial dirty state
    {
        let mut owner_guard = owner.write();
        owner_guard.flush_layout();
        owner_guard.flush_paint();
    }
    state.clear_needs_layout();
    state.clear_needs_paint();

    // Mark needs layout - should schedule with owner
    state.mark_needs_layout();
    assert!(state.needs_layout());
    assert_eq!(owner.read().nodes_needing_layout().len(), 1);

    // Mark needs paint - should schedule with owner
    state.mark_needs_paint();
    assert!(state.needs_paint());
    assert_eq!(owner.read().nodes_needing_paint().len(), 1);
}

#[test]
fn test_render_object_state_dispose() {
    let mut state = RenderObjectState::with_node_id(1);
    let owner = Arc::new(RwLock::new(PipelineOwner::new()));

    state.attach(owner);
    assert!(state.is_attached());

    state.dispose();
    assert!(state.is_disposed());
    assert!(!state.is_attached());
    assert!(state.owner().is_none());
}

#[test]
fn test_render_object_state_depth() {
    let mut state = RenderObjectState::new();

    state.set_depth(5);
    assert_eq!(state.depth(), 5);

    state.set_depth(0);
    assert_eq!(state.depth(), 0);

    // Max depth
    state.set_depth(65535);
    assert_eq!(state.depth(), 65535);
}

// ============================================================================
// BaseRenderObject Tests
// ============================================================================

#[test]
fn test_base_render_object_initial() {
    let base = BaseRenderObject::new();

    assert!(!base.is_attached());
    assert!(base.needs_layout());
    assert!(base.needs_paint());
    assert_eq!(base.depth(), 0);
    assert!(base.parent_data().is_none());
    assert!(base.debug_creator().is_none());
}

#[test]
fn test_base_render_object_lifecycle() {
    let mut base = BaseRenderObject::with_node_id(1);
    let owner = Arc::new(RwLock::new(PipelineOwner::new()));

    // Attach
    base.attach(owner.clone());
    assert!(base.is_attached());

    // Dirty marking
    base.clear_needs_layout();
    base.mark_needs_layout();
    assert!(base.needs_layout());

    // Detach
    base.detach();
    assert!(!base.is_attached());

    // Dispose
    base.dispose();
}

#[test]
fn test_base_render_object_debug_creator() {
    let mut base = BaseRenderObject::new();

    base.set_debug_creator(Some("MyWidget".to_string()));
    assert_eq!(base.debug_creator(), Some("MyWidget"));

    base.set_debug_creator(None);
    assert!(base.debug_creator().is_none());
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<DirtyFlags>();
    assert_send_sync::<RelayoutBoundary>();
    assert_send_sync::<RenderObjectFlags>();
    assert_send_sync::<RenderObjectState>();
    assert_send_sync::<BaseRenderObject>();
}

// ============================================================================
// Flutter Equivalence Tests
// ============================================================================

/// Test that our implementation matches Flutter's behavior for dirty marking.
#[test]
fn test_flutter_dirty_marking_behavior() {
    let mut state = RenderObjectState::with_node_id(1);
    let owner = Arc::new(RwLock::new(PipelineOwner::new()));

    // In Flutter, new render objects have:
    // - _needsLayout = true
    // - _needsPaint = true
    // - _isRelayoutBoundary = null
    assert!(state.needs_layout());
    assert!(state.needs_paint());
    assert_eq!(state.relayout_boundary(), RelayoutBoundary::Unknown);

    // After attach, layout should be scheduled
    state.attach(owner.clone());

    // Verify owner has scheduled layout
    assert!(!owner.read().nodes_needing_layout().is_empty());
}

/// Test Flutter's attach/detach semantics (owner != null means attached).
#[test]
fn test_flutter_attach_semantics() {
    let mut state = RenderObjectState::new();
    let owner = Arc::new(RwLock::new(PipelineOwner::new()));

    // Flutter: bool get attached => _owner != null;
    assert!(!state.is_attached()); // owner is None
    assert!(state.owner().is_none());

    state.attach(owner.clone());
    assert!(state.is_attached()); // owner is Some
    assert!(state.owner().is_some());

    state.detach();
    assert!(!state.is_attached()); // owner is None again
    assert!(state.owner().is_none());
}

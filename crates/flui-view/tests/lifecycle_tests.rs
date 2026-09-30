//! Integration tests for Element lifecycle.
//!
//! Tests the lifecycle states and transitions: Initial → Active ⇄ Inactive →
//! Defunct

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildContext, BuildOwner, ElementTree, IntoView, RenderView, StatefulView, View, ViewExt,
    ViewState,
};

// ============================================================================
// Test Views with lifecycle tracking
// ============================================================================

/// A true tree leaf — renders directly, with no further `build()` in the
/// chain. `TrackingView` above deliberately can't serve this role: its own
/// `build()` returns `self.clone().boxed()`, an intentionally self-
/// referential fixture (never driven through `build_scope` by the tests
/// that use it) that would recurse forever if it were.
#[derive(Clone)]
struct LeafView;

impl RenderView for LeafView {
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

impl View for LeafView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone)]
struct LifecycleTrackingView {
    disposed: Arc<AtomicBool>,
    activated: Arc<AtomicUsize>,
    deactivated: Arc<AtomicUsize>,
}

struct LifecycleTrackingState {
    disposed: Arc<AtomicBool>,
    activated: Arc<AtomicUsize>,
    deactivated: Arc<AtomicUsize>,
}

impl StatefulView for LifecycleTrackingView {
    type State = LifecycleTrackingState;

    fn create_state(&self) -> Self::State {
        LifecycleTrackingState {
            disposed: self.disposed.clone(),
            activated: self.activated.clone(),
            deactivated: self.deactivated.clone(),
        }
    }
}

impl ViewState<LifecycleTrackingView> for LifecycleTrackingState {
    fn build(&self, _view: &LifecycleTrackingView, _ctx: &dyn BuildContext) -> impl IntoView {
        LeafView.boxed()
    }

    fn activate(&mut self) {
        self.activated.fetch_add(1, Ordering::SeqCst);
    }

    fn deactivate(&mut self) {
        self.deactivated.fetch_add(1, Ordering::SeqCst);
    }

    fn dispose(&mut self) {
        self.disposed.store(true, Ordering::SeqCst);
    }
}

impl View for LifecycleTrackingView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

// ============================================================================
// Lifecycle State Tests
// ============================================================================

// ============================================================================
// Element Lifecycle Tests
// ============================================================================

// ============================================================================
// StatefulElement Lifecycle Callbacks
// ============================================================================

pub(crate) fn test_stateful_element_multiple_deactivate_activate_cycles() {
    let activated = Arc::new(AtomicUsize::new(0));
    let deactivated = Arc::new(AtomicUsize::new(0));
    let view = LifecycleTrackingView {
        disposed: Arc::new(AtomicBool::new(false)),
        activated: activated.clone(),
        deactivated: deactivated.clone(),
    };

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());
    // Drive the first build so `init_state` runs before the first
    // `deactivate`/`activate`: `StatefulBehavior::on_activate` is gated on a
    // completed `init_state`, so a mounted but never-built element never runs
    // its `activate` callback.
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    // First cycle
    tree.deactivate(root_id, &mut owner.element_owner_mut());
    tree.activate(root_id, &mut owner.element_owner_mut());

    // Second cycle
    tree.deactivate(root_id, &mut owner.element_owner_mut());
    tree.activate(root_id, &mut owner.element_owner_mut());

    // Third cycle
    tree.deactivate(root_id, &mut owner.element_owner_mut());
    tree.activate(root_id, &mut owner.element_owner_mut());

    assert_eq!(activated.load(Ordering::SeqCst), 3);
    assert_eq!(deactivated.load(Ordering::SeqCst), 3);
}

// ============================================================================
// ElementTree Lifecycle Tests
// ============================================================================

// ============================================================================
// Depth Tests
// ============================================================================

// ============================================================================
// Slot Tests
// ============================================================================

// ============================================================================
// Parent Tracking Tests
// ============================================================================

// ============================================================================
// Lifecycle Memory Layout Tests
// ============================================================================

// ============================================================================
// Thread Safety Tests
// ============================================================================

// ============================================================================
// Characterization Tests for `ElementOwner` threading
// ============================================================================
//
// These tests pin the observable mount/unmount/update invariants that the
// signature-threading refactor must preserve. They were authored BEFORE the
// `&mut ElementOwner` parameter was threaded through `ElementBase` so that any
// regression in the lifecycle FSM during the rewire is caught immediately.
//
// Invariants asserted:
// - After `mount(parent, slot)` the element transitions Initial → Active.
// - After `unmount()` the element transitions to Defunct.
// - A child mounted via `ElementTree::insert` is stored at the recorded
//   parent + slot + (parent_depth + 1).

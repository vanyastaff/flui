//! Integration tests for Element lifecycle.
//!
//! Tests the lifecycle states and transitions: Initial → Active ⇄ Inactive → Defunct

use flui_view::{
    BuildContext, ElementBase, ElementTree, Lifecycle, StatefulElement, StatefulView,
    StatelessElement, StatelessView, View, ViewState,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

// ============================================================================
// Test Views with lifecycle tracking
// ============================================================================

#[derive(Clone)]
struct TrackingView {
    id: u32,
}

impl StatelessView for TrackingView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for TrackingView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self))
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
    fn build(&self, _view: &LifecycleTrackingView, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(TrackingView { id: 0 })
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
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatefulElement::new(self))
    }
}

// ============================================================================
// Lifecycle State Tests
// ============================================================================

#[test]
fn test_lifecycle_initial_state() {
    assert_eq!(Lifecycle::default(), Lifecycle::Initial);
}

#[test]
fn test_lifecycle_state_checks() {
    // Initial state
    let initial = Lifecycle::Initial;
    assert!(!initial.is_active());
    assert!(!initial.is_inactive());
    assert!(!initial.is_defunct());
    assert!(!initial.can_build());

    // Active state
    let active = Lifecycle::Active;
    assert!(active.is_active());
    assert!(!active.is_inactive());
    assert!(active.can_build());
    assert!(active.can_deactivate());
    assert!(!active.can_activate());

    // Inactive state
    let inactive = Lifecycle::Inactive;
    assert!(inactive.is_inactive());
    assert!(!inactive.is_active());
    assert!(!inactive.can_build());
    assert!(inactive.can_activate());
    assert!(!inactive.can_deactivate());

    // Defunct state
    let defunct = Lifecycle::Defunct;
    assert!(defunct.is_defunct());
    assert!(!defunct.can_build());
    assert!(!defunct.can_activate());
}

// ============================================================================
// Element Lifecycle Tests
// ============================================================================

#[test]
fn test_element_initial_lifecycle() {
    let view = TrackingView { id: 1 };
    let element = StatelessElement::new(&view);

    assert_eq!(element.lifecycle(), Lifecycle::Initial);
}

#[test]
fn test_element_mount_transitions_to_active() {
    let view = TrackingView { id: 1 };
    let mut element = StatelessElement::new(&view);

    assert_eq!(element.lifecycle(), Lifecycle::Initial);

    element.mount(None, 0);

    assert_eq!(element.lifecycle(), Lifecycle::Active);
}

#[test]
fn test_element_deactivate_transitions_to_inactive() {
    let view = TrackingView { id: 1 };
    let mut element = StatelessElement::new(&view);

    element.mount(None, 0);
    assert_eq!(element.lifecycle(), Lifecycle::Active);

    element.deactivate();
    assert_eq!(element.lifecycle(), Lifecycle::Inactive);
}

#[test]
fn test_element_activate_transitions_to_active() {
    let view = TrackingView { id: 1 };
    let mut element = StatelessElement::new(&view);

    element.mount(None, 0);
    element.deactivate();
    assert_eq!(element.lifecycle(), Lifecycle::Inactive);

    element.activate();
    assert_eq!(element.lifecycle(), Lifecycle::Active);
}

#[test]
fn test_element_unmount_transitions_to_defunct() {
    let view = TrackingView { id: 1 };
    let mut element = StatelessElement::new(&view);

    element.mount(None, 0);
    element.unmount();

    assert_eq!(element.lifecycle(), Lifecycle::Defunct);
}

// ============================================================================
// StatefulElement Lifecycle Callbacks
// ============================================================================

#[test]
fn test_stateful_element_dispose_called_on_unmount() {
    let disposed = Arc::new(AtomicBool::new(false));
    let view = LifecycleTrackingView {
        disposed: disposed.clone(),
        activated: Arc::new(AtomicUsize::new(0)),
        deactivated: Arc::new(AtomicUsize::new(0)),
    };

    let mut element = StatefulElement::new(&view);
    element.mount(None, 0);

    assert!(!disposed.load(Ordering::SeqCst));

    element.unmount();

    assert!(disposed.load(Ordering::SeqCst));
}

#[test]
fn test_stateful_element_deactivate_callback() {
    let deactivated = Arc::new(AtomicUsize::new(0));
    let view = LifecycleTrackingView {
        disposed: Arc::new(AtomicBool::new(false)),
        activated: Arc::new(AtomicUsize::new(0)),
        deactivated: deactivated.clone(),
    };

    let mut element = StatefulElement::new(&view);
    element.mount(None, 0);

    assert_eq!(deactivated.load(Ordering::SeqCst), 0);

    element.deactivate();

    assert_eq!(deactivated.load(Ordering::SeqCst), 1);
}

#[test]
fn test_stateful_element_activate_callback() {
    let activated = Arc::new(AtomicUsize::new(0));
    let view = LifecycleTrackingView {
        disposed: Arc::new(AtomicBool::new(false)),
        activated: activated.clone(),
        deactivated: Arc::new(AtomicUsize::new(0)),
    };

    let mut element = StatefulElement::new(&view);
    element.mount(None, 0);
    element.deactivate();

    assert_eq!(activated.load(Ordering::SeqCst), 0);

    element.activate();

    assert_eq!(activated.load(Ordering::SeqCst), 1);
}

#[test]
fn test_stateful_element_multiple_deactivate_activate_cycles() {
    let activated = Arc::new(AtomicUsize::new(0));
    let deactivated = Arc::new(AtomicUsize::new(0));
    let view = LifecycleTrackingView {
        disposed: Arc::new(AtomicBool::new(false)),
        activated: activated.clone(),
        deactivated: deactivated.clone(),
    };

    let mut element = StatefulElement::new(&view);
    element.mount(None, 0);

    // First cycle
    element.deactivate();
    element.activate();

    // Second cycle
    element.deactivate();
    element.activate();

    // Third cycle
    element.deactivate();
    element.activate();

    assert_eq!(activated.load(Ordering::SeqCst), 3);
    assert_eq!(deactivated.load(Ordering::SeqCst), 3);
}

// ============================================================================
// ElementTree Lifecycle Tests
// ============================================================================

#[test]
fn test_tree_mount_root_activates_element() {
    let mut tree = ElementTree::new();
    let view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&view);

    let node = tree.get(root_id).unwrap();
    assert_eq!(node.element().lifecycle(), Lifecycle::Active);
}

#[test]
fn test_tree_insert_activates_element() {
    let mut tree = ElementTree::new();
    let root_view = TrackingView { id: 0 };
    let child_view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);

    let child_node = tree.get(child_id).unwrap();
    assert_eq!(child_node.element().lifecycle(), Lifecycle::Active);
}

#[test]
fn test_tree_remove_unmounts_element() {
    let mut tree = ElementTree::new();
    let view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&view);
    let removed_node = tree.remove(root_id).unwrap();

    assert_eq!(removed_node.element().lifecycle(), Lifecycle::Defunct);
}

#[test]
fn test_tree_deactivate_element() {
    let mut tree = ElementTree::new();
    let view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&view);
    tree.deactivate(root_id);

    let node = tree.get(root_id).unwrap();
    assert_eq!(node.element().lifecycle(), Lifecycle::Inactive);
}

#[test]
fn test_tree_activate_element() {
    let mut tree = ElementTree::new();
    let view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&view);
    tree.deactivate(root_id);
    tree.activate(root_id);

    let node = tree.get(root_id).unwrap();
    assert_eq!(node.element().lifecycle(), Lifecycle::Active);
}

// ============================================================================
// Depth Tests
// ============================================================================

#[test]
fn test_root_depth_is_zero() {
    let mut tree = ElementTree::new();
    let view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&view);

    let node = tree.get(root_id).unwrap();
    assert_eq!(node.depth(), 0);
}

#[test]
fn test_child_depth_is_parent_plus_one() {
    let mut tree = ElementTree::new();
    let root_view = TrackingView { id: 0 };
    let child_view = TrackingView { id: 1 };
    let grandchild_view = TrackingView { id: 2 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);
    let grandchild_id = tree.insert(&grandchild_view, child_id, 0);

    assert_eq!(tree.get(root_id).unwrap().depth(), 0);
    assert_eq!(tree.get(child_id).unwrap().depth(), 1);
    assert_eq!(tree.get(grandchild_id).unwrap().depth(), 2);
}

// ============================================================================
// Slot Tests
// ============================================================================

#[test]
fn test_root_slot_is_zero() {
    let mut tree = ElementTree::new();
    let view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&view);

    let node = tree.get(root_id).unwrap();
    assert_eq!(node.slot(), 0);
}

#[test]
fn test_child_slot_assignment() {
    let mut tree = ElementTree::new();
    let root_view = TrackingView { id: 0 };
    let child1 = TrackingView { id: 1 };
    let child2 = TrackingView { id: 2 };
    let child3 = TrackingView { id: 3 };

    let root_id = tree.mount_root(&root_view);
    let child1_id = tree.insert(&child1, root_id, 0);
    let child2_id = tree.insert(&child2, root_id, 1);
    let child3_id = tree.insert(&child3, root_id, 2);

    assert_eq!(tree.get(child1_id).unwrap().slot(), 0);
    assert_eq!(tree.get(child2_id).unwrap().slot(), 1);
    assert_eq!(tree.get(child3_id).unwrap().slot(), 2);
}

// ============================================================================
// Parent Tracking Tests
// ============================================================================

#[test]
fn test_root_has_no_parent() {
    let mut tree = ElementTree::new();
    let view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&view);

    let node = tree.get(root_id).unwrap();
    assert!(node.parent().is_none());
}

#[test]
fn test_child_has_correct_parent() {
    let mut tree = ElementTree::new();
    let root_view = TrackingView { id: 0 };
    let child_view = TrackingView { id: 1 };

    let root_id = tree.mount_root(&root_view);
    let child_id = tree.insert(&child_view, root_id, 0);

    let child_node = tree.get(child_id).unwrap();
    assert_eq!(child_node.parent(), Some(root_id));
}

// ============================================================================
// Lifecycle Memory Layout Tests
// ============================================================================

#[test]
fn test_lifecycle_memory_size() {
    // Lifecycle should be 1 byte (enum with 4 variants)
    assert_eq!(std::mem::size_of::<Lifecycle>(), 1);
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_lifecycle_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<Lifecycle>();
}

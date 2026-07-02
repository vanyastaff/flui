//! Integration tests for StatelessView/StatelessElement and
//! StatefulView/StatefulElement.
//!
//! Tests view creation, element management, state handling, and update cycles.

use std::{
    any::TypeId,
    sync::{
        Arc,
        atomic::{AtomicI32, AtomicUsize, Ordering},
    },
};

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, IntoView, Lifecycle, StatefulBehavior,
    StatefulElement, StatefulView, StatelessBehavior, StatelessElement, StatelessView, View,
    ViewExt, ViewState,
};

// ============================================================================
// StatelessView Tests
// ============================================================================

#[derive(Clone)]
struct SimpleStatelessView {
    #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
    label: String,
}

impl StatelessView for SimpleStatelessView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Return a leaf view, not `self`: a self-returning build
        // describes an infinitely deep element tree and overflows the
        // stack when built.
        LeafView.boxed()
    }
}

impl View for SimpleStatelessView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// A leaf view whose element creates no children — the terminal of a
/// stateless build chain.
#[derive(Clone)]
struct LeafView;

impl flui_view::RenderView for LeafView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}
}

impl View for LeafView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone)]
struct NestedView {
    depth: u32,
}

impl StatelessView for NestedView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Conditional return: each arm has a different concrete type,
        // so each is wrapped with `.boxed()` to land on `BoxedView`.
        // This is the canonical authoring shape for divergent-arm
        // builds documented on `StatelessView::build`.
        if self.depth > 0 {
            NestedView {
                depth: self.depth - 1,
            }
            .boxed()
        } else {
            SimpleStatelessView {
                label: "Leaf".to_string(),
            }
            .boxed()
        }
    }
}

impl View for NestedView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[test]
fn test_stateless_view_create_element() {
    let view = SimpleStatelessView {
        label: "Test".to_string(),
    };
    let element = view.create_element();

    assert_eq!(
        element.element().view_type_id(),
        TypeId::of::<SimpleStatelessView>()
    );
    assert_eq!(element.element().lifecycle(), Lifecycle::Initial);
}

#[test]
fn test_stateless_element_mount() {
    let view = SimpleStatelessView {
        label: "Mount".to_string(),
    };
    let mut element = StatelessElement::new(&view, StatelessBehavior);
    let mut owner = BuildOwner::new();

    element.mount(None, 0, &mut owner.element_owner_mut());

    assert_eq!(element.lifecycle(), Lifecycle::Active);
}

#[test]
fn test_stateless_element_update() {
    let view1 = SimpleStatelessView {
        label: "First".to_string(),
    };
    let view2 = SimpleStatelessView {
        label: "Second".to_string(),
    };

    let mut element = StatelessElement::new(&view1, StatelessBehavior);
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    // Update with new view of same type
    element.update(&view2, &mut owner.element_owner_mut());

    // Element should still be valid
    assert_eq!(element.lifecycle(), Lifecycle::Active);
}

#[test]
fn test_stateless_element_mark_needs_build() {
    let view = SimpleStatelessView {
        label: "Dirty".to_string(),
    };
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);
    assert!(
        !tree.get(root_id).unwrap().element().is_dirty(),
        "initial build_scope clears the dirty flag"
    );

    tree.get_mut(root_id)
        .unwrap()
        .element_mut()
        .mark_needs_build();
    assert!(
        tree.get(root_id).unwrap().element().is_dirty(),
        "mark_needs_build sets the dirty flag again"
    );

    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);
    let element = tree.get(root_id).unwrap().element();
    assert!(!element.is_dirty(), "second build_scope clears dirty again");
    assert_eq!(element.lifecycle(), Lifecycle::Active);
}

#[test]
fn test_nested_stateless_views() {
    let view = NestedView { depth: 3 };
    let element = view.create_element();

    assert_eq!(element.element().view_type_id(), TypeId::of::<NestedView>());
}

// ============================================================================
// StatefulView Tests
// ============================================================================

#[derive(Clone, Debug)]
struct CounterView {
    initial_count: i32,
}

#[derive(Debug)]
struct CounterState {
    count: Arc<AtomicI32>,
    update_count: Arc<AtomicUsize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: Arc::new(AtomicI32::new(self.initial_count)),
            update_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ViewState<CounterView> for CounterState {
    fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> impl IntoView {
        SimpleStatelessView {
            label: format!("Count: {}", self.count.load(Ordering::SeqCst)),
        }
    }

    fn did_update_view(&mut self, _old_view: &CounterView, _new_view: &CounterView) {
        self.update_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl View for CounterView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

#[test]
fn test_stateful_view_create_state() {
    let view = CounterView { initial_count: 10 };
    let element = StatefulElement::new(&view, StatefulBehavior::new(&view));

    assert_eq!(element.state().count.load(Ordering::SeqCst), 10);
}

#[test]
fn test_stateful_element_state_persistence() {
    let view = CounterView { initial_count: 0 };
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    // Modify state
    element.state().count.store(42, Ordering::SeqCst);

    // State should persist
    assert_eq!(element.state().count.load(Ordering::SeqCst), 42);
}

#[test]
fn test_stateful_element_set_state() {
    let view = CounterView { initial_count: 0 };
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    // Use set_state helper
    element.set_state(|state| {
        state.count.store(100, Ordering::SeqCst);
    });

    assert_eq!(element.state().count.load(Ordering::SeqCst), 100);
}

#[test]
fn test_stateful_element_update_calls_did_update_view() {
    let view1 = CounterView { initial_count: 0 };
    let view2 = CounterView { initial_count: 10 };

    let mut element = StatefulElement::new(&view1, StatefulBehavior::new(&view1));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    let update_count = element.state().update_count.clone();
    assert_eq!(update_count.load(Ordering::SeqCst), 0);

    // Update with new view
    element.update(&view2, &mut owner.element_owner_mut());

    assert_eq!(update_count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_stateful_element_multiple_updates() {
    let view = CounterView { initial_count: 0 };
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    let update_count = element.state().update_count.clone();

    // Multiple updates
    for i in 1..=5 {
        let new_view = CounterView { initial_count: i };
        element.update(&new_view, &mut owner.element_owner_mut());
    }

    assert_eq!(update_count.load(Ordering::SeqCst), 5);
}

// ============================================================================
// StatefulElement Lifecycle Callbacks
// ============================================================================

#[derive(Clone)]
struct LifecycleCallbackView;

struct LifecycleCallbackState {
    init_called: Arc<AtomicUsize>,
    dispose_called: Arc<AtomicUsize>,
    activate_called: Arc<AtomicUsize>,
    deactivate_called: Arc<AtomicUsize>,
}

impl StatefulView for LifecycleCallbackView {
    type State = LifecycleCallbackState;

    fn create_state(&self) -> Self::State {
        LifecycleCallbackState {
            init_called: Arc::new(AtomicUsize::new(0)),
            dispose_called: Arc::new(AtomicUsize::new(0)),
            activate_called: Arc::new(AtomicUsize::new(0)),
            deactivate_called: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ViewState<LifecycleCallbackView> for LifecycleCallbackState {
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        self.init_called.fetch_add(1, Ordering::SeqCst);
    }

    fn build(&self, _view: &LifecycleCallbackView, _ctx: &dyn BuildContext) -> impl IntoView {
        SimpleStatelessView {
            label: "Lifecycle".to_string(),
        }
    }

    fn dispose(&mut self) {
        self.dispose_called.fetch_add(1, Ordering::SeqCst);
    }

    fn activate(&mut self) {
        self.activate_called.fetch_add(1, Ordering::SeqCst);
    }

    fn deactivate(&mut self) {
        self.deactivate_called.fetch_add(1, Ordering::SeqCst);
    }
}

impl View for LifecycleCallbackView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

#[test]
fn test_stateful_deactivate_callback_called() {
    let view = LifecycleCallbackView;
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    let deactivate_count = element.state().deactivate_called.clone();
    assert_eq!(deactivate_count.load(Ordering::SeqCst), 0);

    element.deactivate();

    assert_eq!(deactivate_count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_stateful_activate_callback_called() {
    let view = LifecycleCallbackView;
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());
    element.deactivate();

    let activate_count = element.state().activate_called.clone();
    assert_eq!(activate_count.load(Ordering::SeqCst), 0);

    element.activate();

    assert_eq!(activate_count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_stateful_dispose_callback_called_on_unmount() {
    let view = LifecycleCallbackView;
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    let dispose_count = element.state().dispose_called.clone();
    assert_eq!(dispose_count.load(Ordering::SeqCst), 0);

    element.unmount(&mut owner.element_owner_mut());

    assert_eq!(dispose_count.load(Ordering::SeqCst), 1);
}

// ============================================================================
// State Isolation Tests
// ============================================================================

#[test]
fn test_separate_elements_have_separate_state() {
    let view = CounterView { initial_count: 0 };

    let mut element1 = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut element2 = StatefulElement::new(&view, StatefulBehavior::new(&view));

    let mut owner = BuildOwner::new();
    element1.mount(None, 0, &mut owner.element_owner_mut());
    element2.mount(None, 1, &mut owner.element_owner_mut());

    // Modify state of element1
    element1.state().count.store(100, Ordering::SeqCst);

    // element2 should be unaffected
    assert_eq!(element1.state().count.load(Ordering::SeqCst), 100);
    assert_eq!(element2.state().count.load(Ordering::SeqCst), 0);
}

// ============================================================================
// can_update Tests
// ============================================================================

#[test]
fn test_stateless_view_can_update_same_type() {
    let view1 = SimpleStatelessView {
        label: "One".to_string(),
    };
    let view2 = SimpleStatelessView {
        label: "Two".to_string(),
    };

    assert!(view1.can_update(&view2));
    assert!(view2.can_update(&view1));
}

#[test]
fn test_stateless_view_cannot_update_different_type() {
    let stateless = SimpleStatelessView {
        label: "Stateless".to_string(),
    };
    let stateful = CounterView { initial_count: 0 };

    assert!(!stateless.can_update(&stateful));
    assert!(!stateful.can_update(&stateless));
}

#[test]
fn test_stateful_view_can_update_same_type() {
    let view1 = CounterView { initial_count: 0 };
    let view2 = CounterView { initial_count: 100 };

    assert!(view1.can_update(&view2));
    assert!(view2.can_update(&view1));
}

// ============================================================================
// Memory Layout Tests
// ============================================================================

#[test]
fn test_stateless_element_is_small() {
    // StatelessElement should be reasonably sized
    let size = std::mem::size_of::<StatelessElement<SimpleStatelessView>>();
    // Should be less than 256 bytes (view + lifecycle + depth + child + dirty)
    assert!(size < 256, "StatelessElement is too large: {} bytes", size);
}

#[test]
fn test_stateful_element_is_reasonably_sized() {
    // StatefulElement includes state, so it can be larger
    let size = std::mem::size_of::<StatefulElement<CounterView>>();
    // Should be less than 512 bytes
    assert!(size < 512, "StatefulElement is too large: {} bytes", size);
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_stateless_element_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<StatelessElement<SimpleStatelessView>>();
}

#[test]
fn test_stateful_element_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<StatefulElement<CounterView>>();
}

// ============================================================================
// Debug Tests
// ============================================================================

#[test]
fn test_stateless_element_debug() {
    let view = SimpleStatelessView {
        label: "Debug".to_string(),
    };
    let element = StatelessElement::new(&view, StatelessBehavior);

    let debug_str = format!("{:?}", element);
    assert!(debug_str.contains("StatelessElement"));
    assert!(debug_str.contains("lifecycle"));
}

#[test]
fn test_stateful_element_debug() {
    let view = CounterView { initial_count: 42 };
    let element = StatefulElement::new(&view, StatefulBehavior::new(&view));

    let debug_str = format!("{:?}", element);
    assert!(debug_str.contains("StatefulElement"));
    assert!(debug_str.contains("lifecycle"));
}

/// A stateless view that builds a chain of itself `remaining` levels deep,
/// recording the live [`BuildContext::depth`] it sees at each level. The
/// terminal level builds a [`LeafView`].
#[derive(Clone)]
struct DepthProbe {
    remaining: usize,
    seen: Arc<std::sync::Mutex<Vec<usize>>>,
}

impl StatelessView for DepthProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.seen.lock().unwrap().push(ctx.depth());
        if self.remaining == 0 {
            LeafView.boxed()
        } else {
            DepthProbe {
                remaining: self.remaining - 1,
                seen: Arc::clone(&self.seen),
            }
            .boxed()
        }
    }
}

impl View for DepthProbe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// The LIVE `BuildContext` handed to `ViewState::build` during a `build_scope`
/// must report each element's AUTHORITATIVE tree depth (`parent_depth + 1`),
/// not its sibling slot index. Every `DepthProbe` here is an only child (slot
/// 0), so before the live-context depth fix the build saw `0` at every level
/// (`[0, 0, 0]`); the fix makes it report the real chain depth `[0, 1, 2]`.
/// Correct depth is what keeps `depend_on`-registered dependents and
/// `mark_needs_build` rebuilds ordering shallowest-first in the dirty heap.
#[test]
fn live_build_context_reports_authoritative_tree_depth() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let root = DepthProbe {
        remaining: 2,
        seen: Arc::clone(&seen),
    };

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(&root, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);

    assert_eq!(
        *seen.lock().unwrap(),
        vec![0, 1, 2],
        "the live build context must report authoritative tree depth at each \
         chain level, not the sibling slot (which is 0 for every only-child)",
    );
}

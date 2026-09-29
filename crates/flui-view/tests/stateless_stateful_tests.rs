//! Integration tests for StatelessView/StatelessElement and
//! StatefulView/StatefulElement.
//!
//! Tests view creation, element management, state handling, and update cycles.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![expect(clippy::struct_field_names)]

use std::sync::{
    Arc,
    atomic::{AtomicI32, AtomicUsize, Ordering},
};

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildContext, BuildOwner, ElementBase, IntoView, LifecycleContext, StatefulBehavior,
    StatefulElement, StatefulView, StatelessView, View, ViewExt, ViewState,
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
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
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
fn stateful_activate_and_deactivate_require_completed_init_state() {
    let view = LifecycleCallbackView;
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    let deactivate_count = element.state().deactivate_called.clone();
    let activate_count = element.state().activate_called.clone();
    assert_eq!(deactivate_count.load(Ordering::SeqCst), 0);
    assert_eq!(activate_count.load(Ordering::SeqCst), 0);

    element.deactivate(&mut owner.element_owner_mut());
    element.activate(&mut owner.element_owner_mut());

    assert_eq!(
        deactivate_count.load(Ordering::SeqCst),
        0,
        "deactivate must not run before init_state completes"
    );
    assert_eq!(
        activate_count.load(Ordering::SeqCst),
        0,
        "activate must not run before init_state completes"
    );
}

// ============================================================================
// State Isolation Tests
// ============================================================================

// ============================================================================
// can_update Tests
// ============================================================================

// ============================================================================
// Memory Layout Tests
// ============================================================================

// ============================================================================
// Debug Tests
// ============================================================================

//! Integration tests for StatelessView/StatelessElement and
//! StatefulView/StatefulElement.
//!
//! Tests view creation, element management, state handling, and update cycles.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![expect(clippy::struct_field_names)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, IntoView, LifecycleContext, RebuildReason,
    StatefulBehavior, StatefulElement, StatefulView, StatelessView, View, ViewExt, ViewState,
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

/// What a state saw, in order: its creation, initialization and updates.
type HookLog = Arc<Mutex<Vec<String>>>;

#[derive(Clone)]
struct HookOrderView {
    config: &'static str,
    log: HookLog,
}

struct HookOrderState {
    log: HookLog,
}

impl StatefulView for HookOrderView {
    type State = HookOrderState;

    fn create_state(&self) -> Self::State {
        self.log
            .lock()
            .expect("the hook log is never poisoned")
            .push(format!("create {}", self.config));
        HookOrderState {
            log: Arc::clone(&self.log),
        }
    }
}

impl ViewState<HookOrderView> for HookOrderState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        self.log
            .lock()
            .expect("the hook log is never poisoned")
            .push("init".to_owned());
    }

    fn build(&self, _view: &HookOrderView, _ctx: &dyn BuildContext) -> impl IntoView {
        LeafView.boxed()
    }

    fn did_update_view(&mut self, old_view: &HookOrderView, new_view: &HookOrderView) {
        self.log
            .lock()
            .expect("the hook log is never poisoned")
            .push(format!("update {} -> {}", old_view.config, new_view.config));
    }
}

impl View for HookOrderView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// A parent can update an element between its mount and its first build,
/// where `init_state` runs. The state still sees `init_state` first, then one
/// update from the configuration it was created with to the current one.
pub(crate) fn an_update_before_the_first_build_follows_init_state() {
    let log = HookLog::default();
    let view = |config| HookOrderView {
        config,
        log: Arc::clone(&log),
    };
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root = tree.mount_root(&view("a"), &mut owner.element_owner_mut());

    tree.update(root, &view("b"), &mut owner.element_owner_mut());
    tree.update(root, &view("c"), &mut owner.element_owner_mut());
    assert_eq!(
        *log.lock().expect("the hook log is never poisoned"),
        ["create a"],
        "no state hook runs before init_state"
    );

    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(
        *log.lock().expect("the hook log is never poisoned"),
        ["create a", "init", "update a -> c"],
        "the first build initializes the state, then delivers the update"
    );

    tree.update(root, &view("d"), &mut owner.element_owner_mut());
    assert_eq!(
        log.lock()
            .expect("the hook log is never poisoned")
            .last()
            .map(String::as_str),
        Some("update c -> d"),
        "an initialized state is updated at once"
    );
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

pub(crate) fn stateful_activate_and_deactivate_require_completed_init_state() {
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

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
    config: &'static str,
    log: HookLog,
}

impl StatefulView for HookOrderView {
    type State = HookOrderState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        self.log
            .lock()
            .expect("the hook log is never poisoned")
            .push(format!("create {}", self.config));
        HookOrderState {
            config: self.config,
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
        assert_ne!(
            self.config, "fails",
            "the fixture's init_state fails for this view"
        );
    }

    fn apply_view(&mut self, view: &HookOrderView, _ctx: &dyn BuildContext) {
        self.log
            .lock()
            .expect("the hook log is never poisoned")
            .push(format!("apply_view {}", view.config));
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
/// where `create_state` and `init_state` run. The state is created from the
/// newest configuration, so it needs no update for the ones it never saw.
pub(crate) fn an_update_before_the_first_build_creates_the_state_from_the_newest_view() {
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
    assert!(
        log.lock()
            .expect("the hook log is never poisoned")
            .is_empty(),
        "no state exists before the first build"
    );

    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(
        *log.lock().expect("the hook log is never poisoned"),
        ["create c", "init", "apply_view c"],
        "the first build creates the state from the newest view, initializes it, then applies the view"
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

/// A panic in `init_state` leaves the element without a state. An update that
/// arrives before the retry is not lost: the retry creates the state from the
/// newest view, as the first build would have.
pub(crate) fn a_retry_after_a_failed_init_state_creates_the_state_from_the_newest_view() {
    let log = HookLog::default();
    let view = |config| HookOrderView {
        config,
        log: Arc::clone(&log),
    };
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root = tree.mount_root(&view("fails"), &mut owner.element_owner_mut());

    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.build_scope(&mut tree);
    }));
    if let Err(payload) = first {
        flui_foundation::panic::discard_payload(payload);
    }
    assert_eq!(
        *log.lock().expect("the hook log is never poisoned"),
        ["create fails", "init"],
        "the first build created the state and its init_state failed"
    );

    tree.update(root, &view("b"), &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(
        *log.lock().expect("the hook log is never poisoned"),
        ["create fails", "init", "create b", "init", "apply_view b"],
        "the retry creates the state from the newest view instead of reusing the failed one"
    );
}

/// `apply_view` runs before a build that follows a new configuration, once per
/// change: not on a plain state rebuild, and with the view the build then sees.
pub(crate) fn apply_view_runs_once_per_changed_configuration_before_the_build() {
    let log = HookLog::default();
    let view = |config| HookOrderView {
        config,
        log: Arc::clone(&log),
    };
    let entries = || log.lock().expect("the hook log is never poisoned").clone();
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root = tree.mount_root(&view("a"), &mut owner.element_owner_mut());

    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(
        entries(),
        ["create a", "init", "apply_view a"],
        "the first build applies the view after init_state"
    );

    owner.schedule_build_for(root, 0, RebuildReason::StateChange);
    owner.build_scope(&mut tree);
    assert_eq!(
        entries().len(),
        3,
        "a rebuild that changed no configuration does not apply the view again"
    );

    tree.update(root, &view("b"), &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);
    assert_eq!(
        entries()[3..],
        ["update a -> b", "apply_view b"],
        "a new configuration is applied after the update hook, before its build"
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
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
    let mut element = StatefulElement::new(&view, StatefulBehavior::new());
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    assert!(
        element.state().is_none(),
        "the state is created at the first build, not at mount"
    );

    element.deactivate(&mut owner.element_owner_mut());
    element.activate(&mut owner.element_owner_mut());

    assert!(
        element.state().is_none(),
        "deactivate and activate before the first build neither create the state nor reach it"
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

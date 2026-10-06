//! `Router` through the public surface (ADR-0093): a typed stack of route
//! values on a navigator whose facade refuses unaddressable pages.
//!
//! Every page is mounted under a real `Vsync` and the transitions are driven by
//! pumping it, so "on the stack" is checked together with "laid out".
//!
//! # Scenarios
//!
//! An initial route may have gaps in its back stack, and the full initial
//! route has to be matched (`Routable::back_stack`); `go` diffs the page list.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{LaidOut, child_process, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_painting::styling::Color;
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, GestureDetector, NavigatorHandle, NavigatorObserver, PopupRoute, RouteId,
    RouteParseError, RouterError, Text, VsyncScope,
};

const FRAME: Duration = Duration::from_millis(50);

// ============================================================================
// The route type and the pages
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum AppRoute {
    Home,
    Note { id: u32 },
    NoteEdit { id: u32 },
    Tag { name: String },
}

impl Routable for AppRoute {
    fn to_path(&self) -> RoutePath {
        match self {
            Self::Home => RoutePath::root(),
            Self::Note { id } => RoutePath::root().join("note").join(id),
            Self::NoteEdit { id } => RoutePath::root().join("note").join(id).join("edit"),
            Self::Tag { name } => RoutePath::root().join("tag").join(name),
        }
    }

    fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
        let segments: Vec<_> = path.segments().collect();
        let id = |segment: &str| {
            segment.parse().map_err(|_| RouteParseError::Param {
                path: path.clone(),
                field: "id",
                segment: segment.to_owned(),
            })
        };
        match segments.as_slice() {
            [] => Ok(Self::Home),
            [note, n] if note == "note" => Ok(Self::Note { id: id(n)? }),
            [note, n, edit] if note == "note" && edit == "edit" => {
                Ok(Self::NoteEdit { id: id(n)? })
            }
            [tag, name] if tag == "tag" => Ok(Self::Tag {
                name: name.to_string(),
            }),
            _ => Err(RouteParseError::NoMatch { path: path.clone() }),
        }
    }

    fn semantics_label(&self) -> Option<String> {
        match self {
            Self::Note { id } => Some(format!("Note page {id}")),
            _ => None,
        }
    }
}

/// What `Router::handle` answered.
type Acquired = Result<RouterHandle<AppRoute>, RouterError>;

/// What a page's state captured in `init_state`.
#[derive(Clone, Default)]
struct Probe {
    handle: Rc<RefCell<Option<Acquired>>>,
    navigator: Rc<RefCell<Option<NavigatorHandle>>>,
    inits: Rc<Cell<usize>>,
}

impl Probe {
    fn handle(&self) -> RouterHandle<AppRoute> {
        self.handle
            .borrow()
            .clone()
            .expect("the page was mounted")
            .expect("a Router<AppRoute> is above the page")
    }

    fn navigator(&self) -> NavigatorHandle {
        self.navigator
            .borrow()
            .clone()
            .expect("a Navigator is above the page")
    }
}

/// A page that records, in `init_state`, its router handle, its navigator and
/// how often it was initialized, then builds `child`.
#[derive(Clone)]
struct Page {
    probe: Probe,
    child: Rc<dyn Fn() -> BoxedView>,
}

impl View for Page {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Page {
    type State = PageState;
    fn create_state(&self) -> PageState {
        PageState {
            probe: self.probe.clone(),
        }
    }
}

struct PageState {
    probe: Probe,
}

impl ViewState<Page> for PageState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.probe.inits.set(self.probe.inits.get() + 1);
        *self.probe.handle.borrow_mut() = Some(Router::<AppRoute>::handle(cx));
        *self.probe.navigator.borrow_mut() = NavigatorHandle::maybe_of(cx);
    }

    fn build(&self, view: &Page, _cx: &dyn BuildContext) -> impl IntoView {
        (view.child)()
    }
}

fn page(probe: &Probe, child: impl Fn() -> BoxedView + 'static) -> BoxedView {
    Page {
        probe: probe.clone(),
        child: Rc::new(child),
    }
    .boxed()
}

fn text(label: String) -> BoxedView {
    Text::new(label).into_view().boxed()
}

/// The two-screen app: `Home` is a tappable page that pushes `Note 1`.
fn pages(home: &Probe) -> impl Fn(&AppRoute, &dyn BuildContext) -> BoxedView + 'static {
    let home = home.clone();
    move |route, _cx| match route {
        AppRoute::Home => {
            let probe = home.clone();
            page(&home, move || {
                let probe = probe.clone();
                flui_widgets::Semantics::new()
                    .label("Home page")
                    .child(
                        GestureDetector::new()
                            .on_tap(move |_cx| {
                                probe
                                    .handle()
                                    .push(AppRoute::Note { id: 1 })
                                    .expect("the router is mounted");
                            })
                            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
                    )
                    .into_view()
                    .boxed()
            })
        }
        AppRoute::Note { id } => text(format!("Note {id}")),
        AppRoute::NoteEdit { id } => text(format!("Edit {id}")),
        AppRoute::Tag { name } => text(format!("Tag {name}")),
    }
}

fn mount(router: Router<AppRoute>) -> LaidOut {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), router),
        tight(400.0, 400.0),
        vsync,
    );
    laid.enable_semantics();
    laid.pump_for(FRAME);
    laid
}

/// Pump well past the 300 ms default transition.
fn settle(laid: &mut LaidOut) {
    for _ in 0..10 {
        laid.pump_for(FRAME);
    }
}

fn two_screen_app() -> (LaidOut, Probe) {
    let home = Probe::default();
    let laid = mount(Router::new(AppRoute::Home, pages(&home)));
    (laid, home)
}

fn laid_out_text(laid: &LaidOut, label: &str) -> bool {
    laid.find_text(label)
        .is_some_and(|id| laid.try_size(id).is_some_and(|size| size.width > 0.0))
}

fn assert_active_scene(laid: &LaidOut, label: &str) {
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    tree.find_by_label(label)
        .expect("the active page contributes assembled semantics");
    if label != "Home page" {
        assert!(
            tree.find_by_label("Home page").is_err(),
            "covered Home is offstage"
        );
    }
}

// ============================================================================
// Navigation
// ============================================================================

struct OneShotObserver {
    action: RefCell<Option<Rc<dyn Fn()>>>,
}

impl OneShotObserver {
    fn fire(&self) {
        let action = self.action.borrow_mut().take();
        if let Some(action) = action {
            action();
        }
    }
}

impl NavigatorObserver for OneShotObserver {
    fn did_push(&self, _route: RouteId, _previous: Option<RouteId>) {
        self.fire();
    }

    fn did_replace(&self, _new_route: Option<RouteId>, _old_route: Option<RouteId>) {
        self.fire();
    }
}

fn observe_once(navigator: &NavigatorHandle, action: impl Fn() + 'static) {
    // Observers are Arc-owned but the UI capability they observe is owner-local.
    #[expect(clippy::arc_with_non_send_sync)]
    let observer = Arc::new(OneShotObserver {
        action: RefCell::new(Some(Rc::new(action))),
    });
    navigator.add_observer(observer);
}

pub(crate) fn router_push_commits_before_a_reentrant_observer_pop() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    let navigator = home.navigator();
    let home_id = navigator.current().expect("initial page");
    let during = router.clone();
    let pop = navigator.clone();
    observe_once(&navigator, move || {
        assert_eq!(during.current(), AppRoute::Note { id: 1 });
        assert!(pop.pop());
    });
    assert_eq!(router.push(AppRoute::Note { id: 1 }), Ok(()));
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Home);
    assert_eq!(router.location(), RoutePath::root());
    assert!(!router.can_pop());
    assert_eq!(navigator.route_ids().len(), 1);
    assert_eq!(navigator.current(), Some(home_id));
    assert_eq!(home.inits.get(), 1, "the common page retains its state");
    assert!(laid.find_text("Note 1").is_none());
    assert_active_scene(&laid, "Home page");
    assert_eq!(router.push(AppRoute::Note { id: 2 }), Ok(()));
    settle(&mut laid);
    assert!(laid_out_text(&laid, "Note 2"));
    assert_active_scene(&laid, "Note 2");
}

pub(crate) fn router_replace_commits_before_a_reentrant_observer_pop() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    let navigator = home.navigator();
    let home_id = navigator.current().expect("initial page");
    router.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    let during = router.clone();
    let pop = navigator.clone();
    observe_once(&navigator, move || {
        assert_eq!(during.current(), AppRoute::Note { id: 2 });
        assert!(pop.pop());
    });
    assert_eq!(router.replace(AppRoute::Note { id: 2 }), Ok(()));
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Home);
    assert_eq!(router.location(), RoutePath::root());
    assert!(!router.can_pop());
    assert_eq!(navigator.route_ids().len(), 1);
    assert_eq!(navigator.current(), Some(home_id));
    assert!(laid.find_text("Note 2").is_none());
    assert_eq!(home.inits.get(), 1);
    assert_active_scene(&laid, "Home page");
}

pub(crate) fn router_go_commits_before_a_reentrant_observer_pop() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    let navigator = home.navigator();
    let home_id = navigator.current().expect("initial page");
    let during = router.clone();
    let pop = navigator.clone();
    observe_once(&navigator, move || {
        assert_eq!(during.current(), AppRoute::NoteEdit { id: 1 });
        assert!(pop.pop());
    });
    assert_eq!(router.go("/note/1/edit"), Ok(()));
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Note { id: 1 });
    assert_eq!(router.location().as_str(), "/note/1");
    assert!(laid_out_text(&laid, "Note 1"));
    assert_active_scene(&laid, "Note 1");
    assert_eq!(navigator.route_ids().len(), 2);
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Home);
    assert_eq!(navigator.current(), Some(home_id));
    assert_eq!(home.inits.get(), 1);
    assert_active_scene(&laid, "Home page");
}

struct PopupRemovalObserver {
    popup: RouteId,
    action: RefCell<Option<Rc<dyn Fn()>>>,
}

impl NavigatorObserver for PopupRemovalObserver {
    fn did_pop(&self, route: RouteId, _previous: Option<RouteId>) {
        if route == self.popup {
            let action = self.action.borrow_mut().take();
            if let Some(action) = action {
                action();
            }
        }
    }
}

pub(crate) fn router_go_recomputes_its_prefix_after_popup_observer_navigation() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    let navigator = home.navigator();
    let home_id = navigator.current().expect("initial page");
    router.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    let _popup = navigator.push(PopupRoute::<()>::new(|_cx, _a, _s| {
        Text::new("Temporary popup").boxed()
    }));
    settle(&mut laid);
    let pop = navigator.clone();
    #[expect(clippy::arc_with_non_send_sync)]
    let observer = Arc::new(PopupRemovalObserver {
        popup: navigator.current().expect("popup"),
        action: RefCell::new(Some(Rc::new(move || assert!(pop.pop())))),
    });
    navigator.add_observer(observer);
    assert_eq!(router.go("/note/1/edit"), Ok(()));
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::NoteEdit { id: 1 });
    assert_eq!(router.location().as_str(), "/note/1/edit");
    assert_eq!(navigator.route_ids().len(), 3);
    assert_active_scene(&laid, "Edit 1");
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Note { id: 1 });
    assert_active_scene(&laid, "Note 1");
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Home);
    assert_eq!(navigator.current(), Some(home_id));
    assert_eq!(home.inits.get(), 1);
    assert_active_scene(&laid, "Home page");
}

fn observer_panic_recovery(edit: fn(&RouterHandle<AppRoute>), expected: AppRoute) {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    let navigator = home.navigator();
    let home_id = navigator.current().expect("initial page");
    let during = router.clone();
    let observed = expected.clone();
    observe_once(&navigator, move || {
        assert_eq!(during.current(), observed);
        panic!("observer failure");
    });
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| edit(&router)))
        .expect_err("the user observer panicked");
    assert_eq!(
        flui_foundation::panic::payload_text(panic.as_ref()),
        Some("observer failure")
    );
    assert_eq!(router.current(), expected);
    assert!(router.can_pop());
    assert_eq!(router.pop(), Ok(true), "the next operation remains usable");
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Home);
    assert_eq!(router.location(), RoutePath::root());
    assert_eq!(navigator.route_ids().len(), 1);
    assert_eq!(navigator.current(), Some(home_id));
    assert_active_scene(&laid, "Home page");
    assert_eq!(router.push(AppRoute::Note { id: 3 }), Ok(()));
    settle(&mut laid);
    assert!(laid_out_text(&laid, "Note 3"));
    assert_active_scene(&laid, "Note 3");
}

pub(crate) fn router_push_preserves_its_commit_after_an_observer_panic() {
    observer_panic_recovery(
        |router| router.push(AppRoute::Note { id: 1 }).expect("mounted"),
        AppRoute::Note { id: 1 },
    );
}

pub(crate) fn router_go_preserves_its_commit_after_an_observer_panic() {
    observer_panic_recovery(
        |router| router.go("/note/1").expect("known location"),
        AppRoute::Note { id: 1 },
    );
}

/// Only the admitted value owns retirement; page-builder clones are views of
/// it. This lets the test distinguish the Router's outgoing ownership from
/// the navigator's independent page-builder lifetime.
type RouteDropHook = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

struct DropRoute {
    value: u32,
    on_drop: RouteDropHook,
    owns_retirement: bool,
}

thread_local! {
    // Set only in the isolated go-retirement child. The parser constructs
    // opaque original values; their page-builder clones do not own retirement.
    static GO_TARGET_DROP: RefCell<Option<Rc<Cell<bool>>>> = const { RefCell::new(None) };
}

impl DropRoute {
    fn new(value: u32) -> Self {
        Self {
            value,
            on_drop: Rc::default(),
            owns_retirement: true,
        }
    }
}

impl Clone for DropRoute {
    fn clone(&self) -> Self {
        Self {
            value: self.value,
            on_drop: Rc::clone(&self.on_drop),
            owns_retirement: false,
        }
    }
}

impl PartialEq for DropRoute {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Drop for DropRoute {
    fn drop(&mut self) {
        if self.owns_retirement {
            let action = self.on_drop.borrow_mut().take();
            if let Some(action) = action {
                action();
            }
        }
    }
}

impl Routable for DropRoute {
    fn to_path(&self) -> RoutePath {
        RoutePath::root().join(self.value)
    }

    fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
        let value = path
            .segments()
            .last()
            .and_then(|segment| segment.parse().ok())
            .ok_or_else(|| RouteParseError::NoMatch { path: path.clone() })?;
        let route = Self::new(value);
        GO_TARGET_DROP.with(|slot| {
            if let Some(armed) = slot.borrow().clone() {
                *route.on_drop.borrow_mut() = Some(Rc::new(move || {
                    assert!(!armed.get(), "target route retirement");
                }));
            }
        });
        Ok(route)
    }
}

#[derive(Clone, Default)]
struct DropProbe {
    router: Rc<RefCell<Option<RouterHandle<DropRoute>>>>,
    navigator: Rc<RefCell<Option<NavigatorHandle>>>,
}

#[derive(Clone)]
struct DropPage {
    probe: DropProbe,
    value: u32,
}

impl View for DropPage {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for DropPage {
    type State = DropPageState;

    fn create_state(&self) -> Self::State {
        DropPageState(self.probe.clone())
    }
}

struct DropPageState(DropProbe);

impl ViewState<DropPage> for DropPageState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        *self.0.router.borrow_mut() =
            Some(Router::<DropRoute>::handle(cx).expect("router ancestor"));
        *self.0.navigator.borrow_mut() = NavigatorHandle::maybe_of(cx);
    }

    fn build(&self, view: &DropPage, _cx: &dyn BuildContext) -> impl IntoView {
        Text::new(format!("Drop page {}", view.value))
    }
}

fn drop_route_app() -> (LaidOut, RouterHandle<DropRoute>, NavigatorHandle) {
    let probe = DropProbe::default();
    let page_probe = probe.clone();
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Router::new(DropRoute::new(0), move |route: &DropRoute, _cx| {
                DropPage {
                    probe: page_probe.clone(),
                    value: route.value,
                }
                .boxed()
            }),
        ),
        tight(400.0, 400.0),
        vsync,
    );
    laid.enable_semantics();
    laid.pump_for(FRAME);
    let router = probe.router.borrow().clone().expect("mounted page");
    let navigator = probe
        .navigator
        .borrow()
        .clone()
        .expect("navigator ancestor");
    (laid, router, navigator)
}

pub(crate) fn removing_a_router_value_retires_it_after_releasing_the_stack_borrow() {
    let (mut laid, router, navigator) = drop_route_app();
    let home_id = navigator.current().expect("initial page");
    let removed = DropRoute::new(1);
    let hook = removed.on_drop.clone();
    router.push(removed).expect("mounted");
    settle(&mut laid);
    let again = router.clone();
    *hook.borrow_mut() = Some(Rc::new(move || {
        assert_eq!(again.current().value, 0);
        again.push(DropRoute::new(2)).expect("reentrant navigation");
    }));
    assert!(navigator.pop());
    settle(&mut laid);
    assert_eq!(router.current().value, 2);
    assert_eq!(router.location().as_str(), "/2");
    assert!(laid_out_text(&laid, "Drop page 2"));
    assert_active_scene(&laid, "Drop page 2");
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.current().value, 0);
    assert_eq!(navigator.current(), Some(home_id));
}

fn an_observer_failure_retains_competing_router_value_retirement() {
    let (mut laid, router, navigator) = drop_route_app();
    let home_id = navigator.current().expect("initial page");
    let removed = DropRoute::new(1);
    let hook = removed.on_drop.clone();
    router.push(removed).expect("mounted");
    settle(&mut laid);
    let retired = Rc::new(Cell::new(false));
    let retired_in_drop = retired.clone();
    *hook.borrow_mut() = Some(Rc::new(move || {
        retired_in_drop.set(true);
        panic!("competing route retirement");
    }));
    observe_once(&navigator, || panic!("first observer failure"));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.replace(DropRoute::new(2)).expect("mounted");
    }))
    .expect_err("observer failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(panic.as_ref()),
        Some("first observer failure")
    );
    assert!(
        !retired.get(),
        "opaque retirement cannot compete with the first failure"
    );
    assert_eq!(router.current().value, 2);
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.current().value, 0);
    assert_eq!(navigator.route_ids().len(), 1);
    assert_eq!(navigator.current(), Some(home_id));
    router.push(DropRoute::new(3)).expect("recovery");
    settle(&mut laid);
    assert!(laid_out_text(&laid, "Drop page 3"));
    assert_active_scene(&laid, "Drop page 3");
}

fn a_go_observer_failure_retains_competing_temporary_route_values() {
    let (mut laid, router, navigator) = drop_route_app();
    let home_id = navigator.current().expect("initial page");
    let armed = Rc::new(Cell::new(false));
    GO_TARGET_DROP.with(|slot| *slot.borrow_mut() = Some(armed.clone()));
    let arm_in_observer = armed.clone();
    observe_once(&navigator, move || {
        arm_in_observer.set(true);
        panic!("first go observer failure");
    });
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.go("/0/1").expect("known path");
    }))
    .expect_err("observer failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(panic.as_ref()),
        Some("first go observer failure")
    );
    armed.set(false);
    GO_TARGET_DROP.with(|slot| *slot.borrow_mut() = None);
    assert_eq!(router.current().value, 1);
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.current().value, 0);
    assert_eq!(navigator.current(), Some(home_id));
    assert_eq!(navigator.route_ids().len(), 1);
    assert_active_scene(&laid, "Drop page 0");
    router.push(DropRoute::new(3)).expect("recovery");
    settle(&mut laid);
    assert_active_scene(&laid, "Drop page 3");
}

/// A broken retirement guard can abort or deadlock, so this failure competition
/// runs in its own bounded process rather than the ordinary contract table.
#[test]
fn router_observer_failure_and_retirement_competition() {
    if let Some(case) = child_process::selected_case() {
        match case.as_str() {
            "replace" => an_observer_failure_retains_competing_router_value_retirement(),
            "go" => a_go_observer_failure_retains_competing_temporary_route_values(),
            "terminal-factories" => terminal_named_factory_competition(),
            "terminal-observers" => terminal_observer_competition(),
            "terminal-incoming" => terminal_navigator_during_incoming_unwind(),
            "terminal-aliases" => terminal_aliases_preserve_healthy_factories(),
            "terminal-router" => terminal_router_config_retains_factory_after_route_failure(),
            "terminal-route" => terminal_route_result_retains_later_observer(),
            "terminal-record-waker" => terminal_route_record_retains_pending_waker(),
            "terminal-overlay" => terminal_overlay_entries_compete(),
            "terminal-modal-cycle" => terminal_seeded_modal_releases_its_factories(),
            "terminal-modal-alias" => terminal_modal_aliases_remain_usable_until_last_release(),
            "terminal-mounted-router" => terminal_mounted_router_retires_accepted_values(),
            "router-parser" | "router-clone" | "router-state-clone" => {
                terminal_router_construction_matrix(&case);
            }
            "page-healthy" | "page-result" | "page-factory" | "page-compete" | "page-incoming" => {
                terminal_page_route_matrix(&case);
            }
            _ => panic!("unknown child case"),
        }
        child_process::pass();
    }
    child_process::run_rows(
        "router::router_observer_failure_and_retirement_competition",
        &[
            "replace",
            "go",
            "terminal-factories",
            "terminal-observers",
            "terminal-incoming",
            "terminal-aliases",
            "terminal-router",
            "terminal-route",
            "terminal-record-waker",
            "terminal-overlay",
            "terminal-modal-cycle",
            "terminal-modal-alias",
            "terminal-mounted-router",
            "router-parser",
            "router-clone",
            "router-state-clone",
            "page-healthy",
            "page-result",
            "page-factory",
            "page-compete",
            "page-incoming",
        ],
    );
}

// Each callback owns exactly one bomb. These are independent framework fields,
// not an opaque user aggregate with several internally panicking destructors.
#[derive(Clone)]
struct TerminalBomb {
    name: &'static str,
    calls: Rc<Cell<usize>>,
    armed: Rc<Cell<bool>>,
}

impl TerminalBomb {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            calls: Rc::default(),
            armed: Rc::new(Cell::new(true)),
        }
    }
}

impl Drop for TerminalBomb {
    fn drop(&mut self) {
        self.calls.set(self.calls.get() + 1);
        assert!(!self.armed.get(), "{}", self.name);
    }
}

fn assert_terminal_failure(failure: Box<dyn std::any::Any + Send>, expected: &str) {
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some(expected)
    );
    // A fresh owner must remain operational after terminal containment.
    let next = NavigatorHandle::new();
    let calls = Rc::new(Cell::new(0));
    let during = Rc::clone(&calls);
    next.on_generate_route(move |_| {
        during.set(during.get() + 1);
        None
    });
    assert!(next.push_named("/same").is_err());
    assert!(next.push_named("/same").is_err());
    assert_eq!(calls.get(), 2);
    next.clear_routes();
}

fn terminal_named_factory_competition() {
    let navigator = NavigatorHandle::new();
    let first = TerminalBomb::new("first factory");
    let first_calls = Rc::clone(&first.calls);
    let second = TerminalBomb::new("second factory");
    let second_calls = Rc::clone(&second.calls);
    navigator.on_generate_route(move |_| {
        let _ = &first;
        None
    });
    navigator.on_unknown_route(move |_| {
        let _ = &second;
        None
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(navigator)))
        .expect_err("the first terminal factory must fail");
    assert_terminal_failure(failure, "first factory");
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 0, "the competing factory is retained");
}

struct TerminalObserver(
    #[expect(
        dead_code,
        reason = "the observer owns this destructor probe without reading it"
    )]
    TerminalBomb,
);
impl NavigatorObserver for TerminalObserver {}

#[expect(
    clippy::arc_with_non_send_sync,
    reason = "the navigator requires Arc observers; the retirement probe remains on its UI thread"
)]
fn terminal_observer_competition() {
    let navigator = NavigatorHandle::new();
    let first = TerminalBomb::new("first observer");
    let first_calls = Rc::clone(&first.calls);
    let second = TerminalBomb::new("second observer");
    let second_calls = Rc::clone(&second.calls);
    navigator.add_observer(Arc::new(TerminalObserver(first)));
    navigator.add_observer(Arc::new(TerminalObserver(second)));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(navigator)))
        .expect_err("the first terminal observer must fail");
    assert_terminal_failure(failure, "first observer");
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 0);
}

#[expect(
    clippy::arc_with_non_send_sync,
    reason = "the navigator requires Arc observers; the retirement probe remains on its UI thread"
)]
fn terminal_navigator_during_incoming_unwind() {
    let navigator = NavigatorHandle::new();
    let factory = TerminalBomb::new("factory must be retained");
    let factory_calls = Rc::clone(&factory.calls);
    let observer = TerminalBomb::new("observer must be retained");
    let observer_calls = Rc::clone(&observer.calls);
    navigator.on_generate_route(move |_| {
        let _ = &factory;
        None
    });
    navigator.add_observer(Arc::new(TerminalObserver(observer)));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _owner = navigator;
        panic!("incoming failure");
    }))
    .expect_err("the incoming failure reaches the caller");
    assert_terminal_failure(failure, "incoming failure");
    assert_eq!(factory_calls.get(), 0);
    assert_eq!(observer_calls.get(), 0);
}

fn terminal_aliases_preserve_healthy_factories() {
    let navigator = NavigatorHandle::new();
    let alias = navigator.clone();
    let first = TerminalBomb::new("healthy factory");
    first.armed.set(false);
    let first_calls = Rc::clone(&first.calls);
    let second = TerminalBomb::new("healthy fallback");
    second.armed.set(false);
    let second_calls = Rc::clone(&second.calls);
    let deliveries = Rc::new(Cell::new(0));
    let delivered = Rc::clone(&deliveries);
    navigator.on_generate_route(move |_| {
        let _ = &first;
        delivered.set(delivered.get() + 1);
        None
    });
    navigator.on_unknown_route(move |_| {
        let _ = &second;
        None
    });
    drop(navigator);
    assert_eq!(first_calls.get(), 0);
    assert_eq!(second_calls.get(), 0);
    assert!(alias.push_named("/retained").is_err());
    assert_eq!(
        deliveries.get(),
        1,
        "a surviving owner can still invoke its factory"
    );
    drop(alias);
    assert_eq!(
        first_calls.get(),
        1,
        "healthy final retirement runs normally"
    );
    assert_eq!(second_calls.get(), 1);
}

fn terminal_router_config_retains_factory_after_route_failure() {
    let route = DropRoute::new(0);
    *route.on_drop.borrow_mut() = Some(Rc::new(|| panic!("initial route")));
    let page = TerminalBomb::new("page factory");
    let page_calls = Rc::clone(&page.calls);
    let transitions = TerminalBomb::new("transition factory");
    let transition_calls = Rc::clone(&transitions.calls);
    let router = Router::new(route, move |_, _| {
        let _ = &page;
        Text::new("page").boxed()
    })
    .transitions(move |_, _, _, child| {
        let _ = &transitions;
        child
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(router)))
        .expect_err("the initial route must fail retirement");
    assert_terminal_failure(failure, "initial route");
    assert_eq!(page_calls.get(), 0);
    assert_eq!(transition_calls.get(), 0);
}

#[expect(
    clippy::arc_with_non_send_sync,
    reason = "the navigator requires Arc observers; the retirement probe remains on its UI thread"
)]
fn terminal_route_result_retains_later_observer() {
    let navigator = NavigatorHandle::new();
    let result = TerminalResult::new("route result", true);
    let result_calls = Arc::clone(&result.calls);
    navigator
        .seed_initial(SimpleRoute::new(|_| Text::new("route").boxed()).with_current_result(result));
    let observer = TerminalBomb::new("later observer");
    let observer_calls = Rc::clone(&observer.calls);
    navigator.add_observer(Arc::new(TerminalObserver(observer)));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(navigator)))
        .expect_err("the route's retained result must fail retirement");
    assert_terminal_failure(failure, "route result");
    assert_eq!(result_calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(observer_calls.get(), 0);
}

#[derive(Clone)]
struct TerminalResult {
    name: &'static str,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    fail: bool,
}

impl TerminalResult {
    fn new(name: &'static str, fail: bool) -> Self {
        Self {
            name,
            calls: Arc::default(),
            fail,
        }
    }
}

impl Drop for TerminalResult {
    fn drop(&mut self) {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert!(!self.fail, "{}", self.name);
    }
}

struct TerminalWake(
    #[expect(
        dead_code,
        reason = "the waker owns this destructor probe without reading it"
    )]
    TerminalResult,
);
#[expect(
    clippy::manual_noop_waker,
    reason = "this waker owns a hostile destructor probe that Waker::noop cannot carry"
)]
impl std::task::Wake for TerminalWake {
    fn wake(self: Arc<Self>) {}
}

struct TerminalRoute {
    settings: flui_widgets::RouteSettings,
    _retirement: TerminalBomb,
}
impl flui_widgets::Route for TerminalRoute {
    type Output = ();
    fn settings(&self) -> &flui_widgets::RouteSettings {
        &self.settings
    }
}
impl flui_widgets::NavigatorRoute for TerminalRoute {
    fn content_builder(&self) -> flui_widgets::RouteContentBuilder {
        Rc::new(|_| Text::new("terminal route").boxed())
    }
}

fn terminal_route_record_retains_pending_waker() {
    use std::future::Future as _;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    let navigator = NavigatorHandle::new();
    let route = TerminalBomb::new("route owner");
    let route_calls = Rc::clone(&route.calls);
    let mut result = navigator.seed_initial(TerminalRoute {
        settings: flui_widgets::RouteSettings::default(),
        _retirement: route,
    });
    let wake = TerminalResult::new("pending waker", true);
    let wake_calls = Arc::clone(&wake.calls);
    let waker = Waker::from(Arc::new(TerminalWake(wake)));
    assert_eq!(
        Pin::new(&mut result).poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    );
    drop(waker);
    drop(result);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(navigator)))
        .expect_err("the route owner must fail before its pending waiter");
    assert_terminal_failure(failure, "route owner");
    assert_eq!(route_calls.get(), 1);
    assert_eq!(wake_calls.load(std::sync::atomic::Ordering::Relaxed), 0);
}

fn terminal_overlay_entries_compete() {
    use flui_widgets::{InsertPosition, OverlayEntry, OverlayHandle};
    let overlay = OverlayHandle::new();
    let first = TerminalBomb::new("first overlay builder");
    let first_calls = Rc::clone(&first.calls);
    let second = TerminalBomb::new("second overlay builder");
    let second_calls = Rc::clone(&second.calls);
    let first_entry = OverlayEntry::new(move |_| {
        let _ = &first;
        Text::new("first").boxed()
    });
    let second_entry = OverlayEntry::new(move |_| {
        let _ = &second;
        Text::new("second").boxed()
    });
    overlay.insert(&first_entry, &InsertPosition::Top);
    overlay.insert(&second_entry, &InsertPosition::Top);
    drop(first_entry);
    drop(second_entry);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(overlay)))
        .expect_err("first overlay builder must fail terminal retirement");
    assert_terminal_failure(failure, "first overlay builder");
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 0);
}

fn terminal_page_route_matrix(case: &str) {
    let page = TerminalBomb::new("page builder");
    page.armed.set(matches!(
        case,
        "page-factory" | "page-compete" | "page-incoming"
    ));
    let page_calls = Rc::clone(&page.calls);
    let result = TerminalResult::new(
        "page result",
        matches!(case, "page-result" | "page-compete" | "page-incoming"),
    );
    let result_calls = Arc::clone(&result.calls);
    let route = PageRoute::new(move |_, _, _| {
        let _ = &page;
        Text::new("page").boxed()
    })
    .with_current_result(result);
    let incoming = case == "page-incoming";
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let owner = route;
        assert!(!incoming, "incoming route");
        drop(owner);
    }));
    match case {
        "page-healthy" => assert!(outcome.is_ok()),
        "page-factory" => {
            assert_terminal_failure(outcome.expect_err("page builder failure"), "page builder");
        }
        "page-incoming" => assert_terminal_failure(
            outcome.expect_err("incoming route failure"),
            "incoming route",
        ),
        _ => assert_terminal_failure(outcome.expect_err("page result failure"), "page result"),
    }
    assert_eq!(
        result_calls.load(std::sync::atomic::Ordering::Relaxed),
        usize::from(!incoming)
    );
    assert_eq!(
        page_calls.get(),
        usize::from(matches!(case, "page-healthy" | "page-factory"))
    );
}

fn terminal_router_construction_matrix(case: &str) {
    struct CloneRoute {
        second: bool,
        copied: bool,
    }
    impl Clone for CloneRoute {
        fn clone(&self) -> Self {
            assert!(!self.second, "first route clone failure");
            Self {
                second: false,
                copied: true,
            }
        }
    }
    impl PartialEq for CloneRoute {
        fn eq(&self, other: &Self) -> bool {
            self.second == other.second
        }
    }
    impl Drop for CloneRoute {
        fn drop(&mut self) {
            assert!(!self.copied, "competing cloned route retirement");
        }
    }
    impl Routable for CloneRoute {
        fn to_path(&self) -> RoutePath {
            RoutePath::root()
        }
        fn from_path(_: &RoutePath) -> Result<Self, RouteParseError> {
            Ok(Self {
                second: false,
                copied: false,
            })
        }
        fn back_stack(path: &RoutePath) -> Result<Vec<Self>, RouteParseError> {
            assert!(path.as_str() != "/panic", "first route parser failure");
            Ok(vec![
                Self {
                    second: false,
                    copied: false,
                },
                Self {
                    second: true,
                    copied: false,
                },
            ])
        }
    }
    if case == "router-parser" {
        let page = TerminalBomb::new("competing parser page retirement");
        let calls = Rc::clone(&page.calls);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = Router::<CloneRoute>::from_location("/panic", move |_, _| {
                let _ = &page;
                Text::new("page").boxed()
            });
        }))
        .expect_err("custom back-stack panic propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first route parser failure")
        );
        assert_eq!(
            calls.get(),
            0,
            "incoming page ownership survives parser unwind"
        );
    } else {
        let router =
            Router::<CloneRoute>::from_location("/clone", |_, _| Text::new("page").boxed())
                .expect("known path");
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if case == "router-clone" {
                drop(router.clone());
            } else {
                drop(router.create_state());
            }
        }))
        .expect_err("second route clone fails");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first route clone failure")
        );
        drop(router);
    }
    let healthy = Router::new(AppRoute::Home, |_, _| Text::new("recovery").boxed());
    drop(healthy.clone());
    drop(healthy.create_state());
    drop(healthy);
}

fn terminal_seeded_modal_releases_its_factories() {
    let navigator = NavigatorHandle::new();
    let page = TerminalBomb::new("healthy seeded page");
    page.armed.set(false);
    let page_calls = Rc::clone(&page.calls);
    navigator.seed_initial(PageRoute::<()>::new(move |_, _, _| {
        let _ = &page;
        Text::new("seeded page").boxed()
    }));
    drop(navigator);
    assert_eq!(
        page_calls.get(),
        1,
        "a binding cannot keep its own route in the registry alive"
    );
}

fn terminal_modal_aliases_remain_usable_until_last_release() {
    use flui_animation::Animation;
    use flui_widgets::__test_access::{PageRouteProbe, RouteProbe};
    for mounted in [false, true] {
        let navigator = NavigatorHandle::new();
        let page = TerminalBomb::new("healthy aliased page");
        page.armed.set(false);
        let page_calls = Rc::clone(&page.calls);
        let page_builds = Rc::new(Cell::new(0));
        let builds = Rc::clone(&page_builds);
        let route = PageRoute::<()>::new(move |_, _, _| {
            let _ = &page;
            builds.set(builds.get() + 1);
            Text::new("alias page").boxed()
        });
        let modal = route.modal_handle();
        let transition = route.transition_handle();
        assert!(
            transition.controller().is_none(),
            "construction does not install a route"
        );
        navigator.seed_initial(route);
        assert!(
            transition.controller().is_none(),
            "seeding defers install until the mount flush"
        );
        let mut laid = if mounted {
            let vsync = Vsync::new();
            Some(lay_out_animated(
                VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
                tight(400.0, 400.0),
                vsync,
            ))
        } else {
            None
        };
        if let Some(laid) = &mut laid {
            laid.pump_for(FRAME);
            assert!(
                page_builds.get() > 0,
                "the installed route built its actual page"
            );
            assert!(
                laid_out_text(laid, "alias page"),
                "the installed page was laid out"
            );
            let controller = transition
                .controller()
                .expect("mount installs the controller before owner retirement");
            controller.set_value(0.25);
            assert_eq!(controller.value(), 0.25);
        } else {
            assert_eq!(
                page_builds.get(),
                0,
                "the seeded-only page was never mounted"
            );
        }
        drop(navigator);
        if let Some(laid) = &mut laid {
            // Removing the Navigator releases its last shared owner. This is
            // physical terminal retirement, not a route pop/dispose command.
            laid.pump_widget(SizedBox::shrink());
        }
        drop(laid);
        assert_eq!(
            page_calls.get(),
            0,
            "a genuine modal alias still owns its page (mounted={mounted})"
        );
        modal.set_offstage(true);
        assert!(modal.offstage());
        modal.set_offstage(false);
        assert!(!modal.offstage());
        transition.drain_pending_statuses();
        if mounted {
            let controller = transition.controller().expect("the installed transition alias retains its controller after last navigator release");
            controller.set_value(0.5);
            assert_eq!(controller.value(), 0.5);
        } else {
            assert!(
                transition.controller().is_none(),
                "terminal retirement does not install a seeded-only route"
            );
        }
        drop(modal);
        assert_eq!(
            page_calls.get(),
            1,
            "last modal release retires its own factory (mounted={mounted})"
        );
        if mounted {
            let controller = transition.controller().expect(
                "the transition alias still owns its installed animation state after modal release",
            );
            controller.set_value(0.75);
            assert_eq!(controller.value(), 0.75);
        } else {
            assert!(transition.controller().is_none());
        }
        drop(transition);
        assert_eq!(page_calls.get(), 1);
    }
}

fn terminal_mounted_router_retires_accepted_values() {
    let probe = DropProbe::default();
    let page_probe = probe.clone();
    let page = TerminalBomb::new("router page");
    page.armed.set(false);
    let page_armed = Rc::clone(&page.armed);
    let page_calls = Rc::clone(&page.calls);
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Router::new(DropRoute::new(0), move |route: &DropRoute, _| {
                let _ = &page;
                DropPage {
                    probe: page_probe.clone(),
                    value: route.value,
                }
                .boxed()
            }),
        ),
        tight(400.0, 400.0),
        vsync,
    );
    laid.pump_for(FRAME);
    let router = probe.router.borrow_mut().take().expect("mounted router");
    let navigator = probe
        .navigator
        .borrow_mut()
        .take()
        .expect("mounted navigator");
    let route = DropRoute::new(1);
    let retired = Rc::new(Cell::new(0));
    let route_armed = Rc::new(Cell::new(false));
    let during_retired = Rc::clone(&retired);
    let during_armed = Rc::clone(&route_armed);
    *route.on_drop.borrow_mut() = Some(Rc::new(move || {
        during_retired.set(during_retired.get() + 1);
        assert!(!during_armed.get(), "accepted router value");
    }));
    router.push(route).expect("mounted router");
    laid.pump_for(FRAME);
    // Pages may have refreshed the capture slots while mounting. Empty them
    // before unmount so the test does not keep a cycle through its own probes.
    probe.router.borrow_mut().take();
    probe.navigator.borrow_mut().take();
    laid.pump_widget(Text::new("replacement"));
    drop(navigator);
    assert_eq!(
        retired.get(),
        0,
        "the retained router still owns its accepted stack"
    );
    route_armed.set(true);
    page_armed.set(true);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(router)))
        .expect_err("the accepted typed value must fail first");
    assert_terminal_failure(failure, "accepted router value");
    assert_eq!(retired.get(), 1);
    assert_eq!(page_calls.get(), 0, "competing page factory is retained");
}

/// Divergence (ARCHITECTURE.md mapping decision 23): a Router never pops its
/// last page, through its own handle or through the navigator facade.
pub(crate) fn router_never_pops_its_last_page() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    assert_eq!(router.pop(), Ok(false));
    assert!(!home.navigator().pop(), "the facade refuses too");
    assert!(!home.navigator().pop_with(3_u8));
    settle(&mut laid);

    assert_eq!(router.location(), RoutePath::root());
    assert_eq!(home.navigator().route_ids().len(), 1);
    assert!(!router.can_pop());
}

pub(crate) fn router_opens_at_a_location_with_its_back_stack() {
    let home = Probe::default();
    let router = Router::from_location("/note/5", pages(&home)).expect("a known location");
    let mut laid = mount(router);
    settle(&mut laid);

    let router = home.handle();
    assert_eq!(router.location().as_str(), "/note/5");
    assert!(laid_out_text(&laid, "Note 5"));
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.location(), RoutePath::root());

    assert!(matches!(
        Router::<AppRoute>::from_location("/settings/x", pages(&home)),
        Err(RouteParseError::NoMatch { .. })
    ));
}

// ============================================================================
// Restoring a saved stack
// ============================================================================

/// Pages that each record their router handle in `probe`, so whichever page
/// a restored router opens on hands the test a handle.
fn recording_pages(probe: &Probe) -> impl Fn(&AppRoute, &dyn BuildContext) -> BoxedView + 'static {
    let probe = probe.clone();
    move |route, _cx| {
        let label = match route {
            AppRoute::Home => "Home".to_owned(),
            AppRoute::Note { id } => format!("Note {id}"),
            AppRoute::NoteEdit { id } => format!("Edit {id}"),
            AppRoute::Tag { name } => format!("Tag {name}"),
        };
        page(&probe, move || text(label.clone()))
    }
}

/// A stack saved as `Home → Note 5 → Tag x` reopens on `Tag x`, and two
/// Backs lead to `Note 5` and then `Home`.
pub(crate) fn from_stack_restores_back_order() {
    let probe = Probe::default();
    let stack = vec![
        AppRoute::Home,
        AppRoute::Note { id: 5 },
        AppRoute::Tag { name: "x".into() },
    ];
    let router = Router::from_stack(stack, recording_pages(&probe)).expect("a non-empty stack");
    let mut laid = mount(router);
    settle(&mut laid);

    let router = probe.handle();
    assert!(laid_out_text(&laid, "Tag x"), "the restored top is shown");
    assert_eq!(router.pop(), Ok(true), "Back from the restored top");
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Note { id: 5 });
    assert!(laid_out_text(&laid, "Note 5"));
    assert_eq!(router.pop(), Ok(true), "Back from the middle page");
    settle(&mut laid);
    assert_eq!(router.current(), AppRoute::Home);
    assert!(!router.can_pop());
}

/// `stack()` reads the whole stack after each push, replace and pop.
pub(crate) fn stack_reads_every_committed_edit() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    assert_eq!(router.stack(), vec![AppRoute::Home]);

    router
        .push(AppRoute::Note { id: 1 })
        .expect("mounted router");
    assert_eq!(
        router.stack(),
        vec![AppRoute::Home, AppRoute::Note { id: 1 }]
    );
    router
        .push(AppRoute::Tag { name: "x".into() })
        .expect("mounted router");
    router
        .replace(AppRoute::NoteEdit { id: 1 })
        .expect("mounted router");
    assert_eq!(
        router.stack(),
        vec![
            AppRoute::Home,
            AppRoute::Note { id: 1 },
            AppRoute::NoteEdit { id: 1 }
        ]
    );
    settle(&mut laid);
    assert_eq!(router.pop(), Ok(true));
    assert_eq!(
        router.stack(),
        vec![AppRoute::Home, AppRoute::Note { id: 1 }]
    );
}

/// A router cannot open on an empty stack, and says so.
pub(crate) fn empty_stack_is_refused() {
    let home = Probe::default();
    assert!(matches!(
        Router::<AppRoute>::from_stack(Vec::new(), pages(&home)),
        Err(RouterError::EmptyStack)
    ));
}

// ============================================================================
// The facade under a Router
// ============================================================================

pub(crate) fn popup_routes_are_admitted_and_leave_the_location_alone() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    let navigator = home.navigator();

    let _result = navigator.push(PopupRoute::<()>::new(|_cx, _a, _s| {
        Text::new("Dialog").into_view().boxed()
    }));
    settle(&mut laid);
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "the popup is on the navigator"
    );
    assert!(laid_out_text(&laid, "Dialog"));
    assert_eq!(router.location(), RoutePath::root());
    assert!(!router.can_pop(), "a popup is not a page");

    assert_eq!(router.pop(), Ok(true), "pop dismisses the popup first");
    settle(&mut laid);
    assert_eq!(navigator.route_ids().len(), 1);
    assert_eq!(router.location(), RoutePath::root());
    assert!(laid.find_text("Dialog").is_none());
}

// ============================================================================
// Lookup
// ============================================================================

// ============================================================================
// Pages
// ============================================================================

// ============================================================================
// The facade cannot take a Router's pages
// ============================================================================

// ============================================================================
// A parent rebuild
// ============================================================================

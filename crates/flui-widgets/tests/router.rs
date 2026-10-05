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

use crate::common::{LaidOut, lay_out_animated, tight};
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
struct DropRoute {
    value: u32,
    on_drop: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
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
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    const CHILD: &str = "FLUI_ROUTER_RETIREMENT_CHILD";
    if let Ok(case) = std::env::var(CHILD) {
        match case.as_str() {
            "replace" => an_observer_failure_retains_competing_router_value_retirement(),
            "go" => a_go_observer_failure_retains_competing_temporary_route_values(),
            _ => panic!("unknown child case"),
        }
        return;
    }
    let mut failures = Vec::new();
    for case in ["replace", "go"] {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "router::router_observer_failure_and_retirement_competition",
                "--nocapture",
            ])
            .env(CHILD, case)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("router retirement child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("child stdout");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("child stderr");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill deadlocked child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{case}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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

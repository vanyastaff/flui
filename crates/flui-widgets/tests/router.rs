//! `Router` through the public surface (ADR-0093): a typed stack of route
//! values on a navigator whose facade refuses unaddressable pages.
//!
//! Every page is mounted under a real `Vsync` and the transitions are driven by
//! pumping it, so "on the stack" is checked together with "laid out".
//!
//! # Parity oracles
//!
//! Flutter 3.44 `navigator_test.dart` — `'Initial route can have gaps'`,
//! `'The full initial route has to be matched'` (`Routable::back_stack`, unit
//! tests in `router/routable.rs`), and the page-list diff of
//! `NavigatorState._updatePages` for `go`. From memory of that release; not
//! checked against a local clone.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_types::Color;
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, GestureDetector, NamedRouteError, NavigatorHandle, PageRoute, PopupRoute,
    RouteParseError, RouteRequest, RouterError, SimpleRoute, Text, VsyncScope,
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
                GestureDetector::new()
                    .on_tap(move || {
                        probe
                            .handle()
                            .push(AppRoute::Note { id: 1 })
                            .expect("the router is mounted");
                    })
                    .child(ColoredBox::new(Color::rgb(10, 20, 30)))
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
    lay_out_animated(
        VsyncScope::new(vsync.clone(), router),
        tight(400.0, 400.0),
        vsync,
    )
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
        .is_some_and(|id| laid.try_size(id).is_some_and(|size| size.width.get() > 0.0))
}

// ============================================================================
// Navigation
// ============================================================================

#[test]
fn two_screen_app_navigates_by_handle() {
    let (mut laid, home) = two_screen_app();

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    settle(&mut laid);

    let router = home.handle();
    assert!(
        laid_out_text(&laid, "Note 1"),
        "the pushed page is laid out"
    );
    assert_eq!(router.location().as_str(), "/note/1");
    assert_eq!(router.current(), AppRoute::Note { id: 1 });
    assert!(router.can_pop());
}

#[test]
fn two_screen_app_navigates_by_url() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    router.go("/note/7").expect("a known location");
    settle(&mut laid);
    assert!(
        laid_out_text(&laid, "Note 7"),
        "the page is laid out, not only present"
    );
    assert_eq!(router.location().as_str(), "/note/7");

    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.location(), RoutePath::root());
    assert!(laid.find_text("Note 7").is_none());
}

#[test]
fn go_with_an_unknown_location_leaves_the_stack_alone() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    assert!(matches!(
        router.go("/nope"),
        Err(RouterError::Parse(RouteParseError::NoMatch { .. }))
    ));
    assert!(matches!(
        router.go("nope"),
        Err(RouterError::Parse(RouteParseError::Malformed { .. }))
    ));
    settle(&mut laid);
    assert_eq!(router.location(), RoutePath::root());
    assert_eq!(home.navigator().route_ids().len(), 1);
}

#[test]
fn pop_restores_the_previous_route() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    router.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);

    assert_eq!(router.location(), RoutePath::root());
    assert!(
        laid.find_text("Note 1").is_none(),
        "the popped page is gone"
    );
    assert_eq!(
        home.inits.get(),
        1,
        "Home kept its state: it was never remounted"
    );
}

/// Divergence (ARCHITECTURE.md mapping decision 23): a Router never pops its
/// last page, through its own handle or through the navigator facade.
#[test]
fn router_never_pops_its_last_page() {
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

#[test]
fn replace_swaps_the_top_without_growing_the_stack() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    router.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    router.replace(AppRoute::Note { id: 2 }).expect("mounted");
    settle(&mut laid);

    assert_eq!(router.location().as_str(), "/note/2");
    assert!(laid_out_text(&laid, "Note 2"));
    assert_eq!(home.navigator().route_ids().len(), 2);

    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.location(), RoutePath::root());
    assert!(!router.can_pop());
}

#[test]
fn facade_pop_updates_the_router_location() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    router.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    assert!(home.navigator().pop(), "the facade pops a Router page");
    assert_eq!(
        router.location(),
        RoutePath::root(),
        "the observer saw the pop"
    );
    settle(&mut laid);
    assert!(laid.find_text("Note 1").is_none());
}

#[test]
fn go_reconciles_only_the_diverging_tail() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    router.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    router.go("/tag/x").expect("a known location");
    settle(&mut laid);

    assert_eq!(home.inits.get(), 1, "the shared bottom page kept its state");
    assert_eq!(router.location().as_str(), "/tag/x");
    assert!(laid_out_text(&laid, "Tag x"));
    assert!(
        laid.find_text("Note 1").is_none(),
        "the diverging page left"
    );
    assert_eq!(home.navigator().route_ids().len(), 2);

    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.location(), RoutePath::root());
}

#[test]
fn go_adds_the_new_back_stack_beneath_the_new_top() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();

    router.go("/note/3/edit").expect("a known location");
    settle(&mut laid);
    assert_eq!(router.location().as_str(), "/note/3/edit");
    assert!(laid_out_text(&laid, "Edit 3"));
    assert_eq!(home.navigator().route_ids().len(), 3);

    // The page added quietly beneath the new top is a real, poppable page.
    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(router.location().as_str(), "/note/3");
    assert!(laid_out_text(&laid, "Note 3"));

    // A prefix of the current stack pops back to it.
    router.push(AppRoute::NoteEdit { id: 3 }).expect("mounted");
    settle(&mut laid);
    router.go("/").expect("the root");
    settle(&mut laid);
    assert_eq!(router.location(), RoutePath::root());
    assert_eq!(home.navigator().route_ids().len(), 1);
    assert_eq!(home.inits.get(), 1);
}

#[test]
fn router_opens_at_a_location_with_its_back_stack() {
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

#[test]
fn pushing_a_page_route_under_a_router_is_not_addressable() {
    let (mut laid, home) = two_screen_app();
    let router = home.handle();
    let navigator = home.navigator();
    let before = navigator.route_ids();

    let pushed = catch_unwind(AssertUnwindSafe(|| {
        navigator.push(PageRoute::<()>::new(|_cx, _a, _s| {
            Text::new("Stray").into_view().boxed()
        }))
    }));
    if cfg!(debug_assertions) {
        let payload = pushed.expect_err("a debug build asserts on the refusal");
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(
            message.contains("may enter a Router's stack"),
            "the assertion carries the NotAddressable text, got {message:?}"
        );
    } else {
        let result = pushed.expect("a release build returns");
        assert_eq!(result.try_take(), Some(None), "completed with None");
    }

    let simple = catch_unwind(AssertUnwindSafe(|| {
        navigator.push(SimpleRoute::<()>::new(|_cx| {
            Text::new("Stray").into_view().boxed()
        }))
    }));
    assert_eq!(simple.is_err(), cfg!(debug_assertions));

    navigator.route("/stray", |_request: &RouteRequest<'_>| {
        Some(PageRoute::<()>::new(|_cx, _a, _s| {
            Text::new("Stray").into_view().boxed()
        }))
    });
    assert_eq!(
        navigator.push_named("/stray"),
        Err(NamedRouteError::NotAddressable {
            name: "/stray".to_owned()
        })
    );

    settle(&mut laid);
    assert_eq!(navigator.route_ids(), before, "nothing entered the stack");
    assert_eq!(router.location(), RoutePath::root());
    assert!(laid.find_text("Stray").is_none());
}

#[test]
fn popup_routes_are_admitted_and_leave_the_location_alone() {
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

#[test]
fn nested_router_handle_targets_the_nearest_router() {
    let outer = Probe::default();
    let inner = Probe::default();
    let router = {
        let (outer, inner) = (outer.clone(), inner.clone());
        Router::new(AppRoute::Home, move |route: &AppRoute, _cx| match route {
            AppRoute::Home => {
                let inner = inner.clone();
                page(&outer, move || {
                    let inner = inner.clone();
                    Router::new(AppRoute::Home, move |route: &AppRoute, _cx| match route {
                        AppRoute::Home => page(&inner, || text("Inner home".to_owned())),
                        other => text(format!("Inner {}", other.to_path())),
                    })
                    .boxed()
                })
            }
            other => text(format!("Outer {}", other.to_path())),
        })
    };
    let mut laid = mount(router);
    settle(&mut laid);

    let (outer, inner) = (outer.handle(), inner.handle());
    inner.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    assert_eq!(inner.location().as_str(), "/note/1");
    assert_eq!(
        outer.location(),
        RoutePath::root(),
        "the outer router is untouched"
    );
    assert!(laid_out_text(&laid, "Inner /note/1"));

    outer
        .push(AppRoute::Tag { name: "t".into() })
        .expect("mounted");
    settle(&mut laid);
    assert_eq!(outer.location().as_str(), "/tag/t");
    assert_eq!(inner.location().as_str(), "/note/1");
    assert!(laid_out_text(&laid, "Outer /tag/t"));
}

#[test]
fn router_handle_without_a_router_is_no_router() {
    let probe = Probe::default();
    let _laid = lay_out(
        Page {
            probe: probe.clone(),
            child: Rc::new(|| SizedBox::new(10.0, 10.0).into_view().boxed()),
        },
        tight(100.0, 100.0),
    );
    let acquired = probe.handle.borrow().clone().expect("init_state ran");
    assert!(
        matches!(acquired, Err(RouterError::NoRouter { route_type }) if route_type.contains("AppRoute"))
    );
}

// ============================================================================
// Pages
// ============================================================================

#[test]
fn router_pages_run_their_page_transition() {
    let home = Probe::default();
    let seen: Rc<RefCell<Vec<f32>>> = Rc::default();
    let router = {
        let seen = Rc::clone(&seen);
        Router::new(AppRoute::Home, pages(&home)).transitions(
            move |_cx, animation, _secondary, child| {
                seen.borrow_mut().push(animation.value());
                child
            },
        )
    };
    let mut laid = mount(router);
    settle(&mut laid);

    seen.borrow_mut().clear();
    home.handle()
        .push(AppRoute::Note { id: 1 })
        .expect("mounted");
    for _ in 0..4 {
        laid.pump_for(FRAME);
    }
    assert!(
        seen.borrow()
            .iter()
            .any(|&value| value > 0.0 && value < 1.0),
        "a page's entrance runs through the router's transition, got {:?}",
        seen.borrow()
    );
}

#[test]
fn router_pages_scope_and_name_a_semantics_route() {
    let (mut laid, home) = two_screen_app();
    laid.enable_semantics();
    let router = home.handle();

    router.push(AppRoute::Note { id: 1 }).expect("mounted");
    settle(&mut laid);
    let tree = laid.a11y_tree().expect("semantics is on");
    assert!(
        tree.find_by_label("Note page 1").is_ok(),
        "the page names its route: {}",
        tree.describe()
    );
    let wrappers = laid.find_semantics_wrappers();
    assert!(
        wrappers
            .iter()
            .any(|&id| laid.render_property(id, "scopes_route").is_some()),
        "a page wrapper scopes a route"
    );
    assert!(
        wrappers
            .iter()
            .any(|&id| laid.render_property(id, "names_route").is_some()),
        "the labelled page names its route"
    );

    assert_eq!(router.pop(), Ok(true));
    settle(&mut laid);
    let tree = laid.a11y_tree().expect("semantics is on");
    assert!(
        tree.find_by_label("Note page 1").is_err(),
        "the popped page's route is gone"
    );
}

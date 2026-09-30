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
use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_painting::styling::Color;
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, GestureDetector, NavigatorHandle, PopupRoute, RouteParseError, RouterError, Text,
    VsyncScope,
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
                    .on_tap(move |_cx| {
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
        .is_some_and(|id| laid.try_size(id).is_some_and(|size| size.width > 0.0))
}

// ============================================================================
// Navigation
// ============================================================================

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

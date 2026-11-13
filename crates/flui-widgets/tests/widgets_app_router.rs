//! [`WidgetsApp::router`] (ADR-0330 §2): the app's routing subtree is a typed
//! `Router`, whose navigator is the app's only one and whose stack survives a
//! rebuild of the app.
//!
//! # Shape
//!
//! The router form mounts `Router` as the routing subtree, bare, below
//! `Localizations` and the `builder` hook; the home form wraps its one page's
//! navigator in a `FocusScope`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_interaction::focus::FocusScopeNode;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_platform::Locale;
use flui_view::environment::VsyncScope;
use flui_widgets::__test_access::NavigatorHandle;

use crate::widgets_app::OnNavigator;
use flui_widgets::interaction::Focus;
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, Directionality, FocusScope, Localizations, RouterError, Row, SizedBox, Text,
    WidgetsApp,
};

use crate::common::{LaidOut, lay_out_animated, tight};

const FRAME: Duration = Duration::from_millis(50);

#[derive(Routable, Debug, Clone, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: u32 },
}

/// What `Router::of` answered.
type Acquired = Result<RouterHandle<AppRoute>, RouterError>;

/// What the Home page's state saw in `init_state` and `build`.
#[derive(Clone, Default)]
struct Probe {
    handle: Rc<RefCell<Option<Acquired>>>,
    nearest: Rc<RefCell<Option<NavigatorHandle>>>,
    root: Rc<RefCell<Option<NavigatorHandle>>>,
    inits: Rc<Cell<usize>>,
    direction: Rc<Cell<Option<TextDirection>>>,
    locale: Rc<RefCell<Option<Locale>>>,
    focus_scope: Rc<RefCell<Option<Rc<FocusScopeNode>>>>,
}

impl Probe {
    fn handle(&self) -> RouterHandle<AppRoute> {
        self.handle
            .borrow()
            .clone()
            .expect("the Home page was mounted")
            .expect("a Router<AppRoute> is above the Home page")
    }
}

/// The Home page: a button over the whole page that pushes `Note 1`.
#[derive(Clone)]
struct Home {
    probe: Probe,
}

impl View for Home {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Home {
    type State = HomeState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> HomeState {
        HomeState {
            probe: self.probe.clone(),
        }
    }
}

struct HomeState {
    probe: Probe,
}

impl ViewState<Home> for HomeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let probe = &self.probe;
        probe.inits.set(probe.inits.get() + 1);
        *probe.handle.borrow_mut() = Some(Router::<AppRoute>::of(ctx));
        *probe.nearest.borrow_mut() = NavigatorHandle::maybe_of(ctx);
        *probe.root.borrow_mut() = NavigatorHandle::maybe_of_root(ctx);
    }

    fn build(&self, _view: &Home, ctx: &dyn BuildContext) -> impl IntoView {
        self.probe.direction.set(Directionality::maybe_of(ctx));
        *self.probe.locale.borrow_mut() = Localizations::maybe_locale_of(ctx);
        *self.probe.focus_scope.borrow_mut() = Some(FocusScope::of(ctx));
        let router = self.probe.handle();
        RawButton::new(ColoredBox::new(Color::rgb(10, 20, 30))).on_pressed(move |_ctx| {
            router
                .push(AppRoute::Note { id: 1 })
                .expect("BUG: Home's button fires only while its Router is mounted");
        })
    }
}

/// The page builder; `version` tags the Note page's text.
fn router(probe: &Probe, version: u32, initial: &str) -> Router<AppRoute> {
    let probe = probe.clone();
    Router::from_location(initial, move |route: &AppRoute, _ctx| match route {
        AppRoute::Home => Home {
            probe: probe.clone(),
        }
        .boxed(),
        AppRoute::Note { id } => Text::new(format!("v{version} Note {id}"))
            .into_view()
            .boxed(),
    })
    .expect("a known location")
}

fn mount(app: WidgetsApp, vsync: &Vsync) -> LaidOut {
    lay_out_animated(
        VsyncScope::new(vsync.clone(), app),
        tight(400.0, 400.0),
        vsync.clone(),
    )
}

/// Pump well past the 300 ms default transition.
fn settle(laid: &mut LaidOut) {
    for _ in 0..10 {
        laid.pump_for(FRAME);
    }
}

fn laid_out_text(laid: &LaidOut, label: &str) -> bool {
    laid.find_text(label)
        .is_some_and(|id| laid.try_size(id).is_some_and(|size| size.width > 0.0))
}

fn tap(laid: &mut LaidOut) {
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    settle(laid);
}

pub(crate) fn widgets_app_router_navigates_by_handle_and_the_url_follows() {
    let probe = Probe::default();
    let vsync = Vsync::new();
    let mut laid = mount(WidgetsApp::router(router(&probe, 1, "/")), &vsync);
    settle(&mut laid);
    let handle = probe.handle();
    assert_eq!(handle.location().as_str(), "/");

    tap(&mut laid);
    assert_eq!(handle.location().as_str(), "/note/1");
    assert!(laid_out_text(&laid, "v1 Note 1"));

    // Home is covered: a second tap at the same point pushes nothing.
    tap(&mut laid);
    assert_eq!(handle.pop(), Ok(true));
    settle(&mut laid);
    assert_eq!(handle.location().as_str(), "/", "one Note page was pushed");
    assert!(!handle.can_pop());
    assert!(laid.find_text("v1 Note 1").is_none());
    assert_eq!(probe.inits.get(), 1, "Home kept its state under the Note");
}

/// The pushed page's controls, in reading order.
const CONTROLS: [&str; 4] = ["Back", "Save", "Share", "Reload"];

/// A page whose focusable controls sit in a row, each a few single-child
/// levels below the route like a real button's `Focus`.
fn controls_router(probe: &Probe) -> Router<AppRoute> {
    let probe = probe.clone();
    Router::from_location("/", move |route: &AppRoute, _ctx| match route {
        AppRoute::Home => Home {
            probe: probe.clone(),
        }
        .boxed(),
        AppRoute::Note { .. } => Row::new(
            CONTROLS
                .iter()
                .map(|&label| {
                    SizedBox::new(40.0, 40.0)
                        .child(
                            ColoredBox::new(Color::rgb(1, 2, 3))
                                .child(Focus::new(SizedBox::new(30.0, 30.0)).debug_label(label)),
                        )
                        .into_view()
                        .boxed()
                })
                .collect::<Vec<_>>(),
        )
        .into_view()
        .boxed(),
    })
    .expect("a known location")
}

/// The control focused once `controls_router`'s page is pushed in a fresh
/// mount.
fn first_focus_of_pushed_page() -> Option<String> {
    let probe = Probe::default();
    let vsync = Vsync::new();
    let mut laid = mount(WidgetsApp::router(controls_router(&probe)), &vsync);
    settle(&mut laid);
    tap(&mut laid);
    assert_eq!(probe.handle().location().as_str(), "/note/1");
    laid.focus_manager()
        .primary_focus()
        .and_then(|node| node.debug_label().map(str::to_owned))
}

/// A pushed route focuses its first control in reading order, the same one
/// in every mount, whatever the process mounted before.
///
/// The route requests focus before its page builds, and the first control
/// to attach takes it. The controls attach as their elements first build;
/// the build drain ordered same-depth elements by depth alone, so which
/// sibling subtree built first followed the heap's arrangement of
/// everything else queued (hash-ordered inherited dependents among them) and
/// changed between mounts and runs.
pub(crate) fn a_pushed_route_focuses_its_first_control_in_every_mount() {
    let first = first_focus_of_pushed_page();
    // Process history: an unrelated app mounted, navigated and dropped
    // advances the global focus-node counter and the hashers' seeds.
    {
        let probe = Probe::default();
        let vsync = Vsync::new();
        let mut laid = mount(WidgetsApp::router(router(&probe, 1, "/")), &vsync);
        settle(&mut laid);
        tap(&mut laid);
        assert_eq!(probe.handle().pop(), Ok(true));
        settle(&mut laid);
    }
    let second = first_focus_of_pushed_page();
    assert_eq!(
        first.as_deref(),
        Some(CONTROLS[0]),
        "the pushed page focuses its first control"
    );
    assert_eq!(second, first, "a later mount focuses the same control");
}

// The home and router forms are one view type, so the switch updates the
// shell in place: its state lets go of the home page's navigator, and the
// home form coming back makes a fresh one.
pub(crate) fn switching_widgets_app_from_home_to_router_releases_the_navigator() {
    let vsync = Vsync::new();
    let first = OnNavigator(Rc::default());
    let mut laid = mount(WidgetsApp::new(first.clone()), &vsync);
    let handle = first.handle();
    assert!(handle.is_mounted());

    let probe = Probe::default();
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        WidgetsApp::router(router(&probe, 1, "/")),
    ));
    settle(&mut laid);
    assert!(!handle.is_mounted(), "the shell's navigator left the tree");
    assert_eq!(
        probe.handle().location().as_str(),
        "/",
        "the Router mounted"
    );
    let nearest = probe.nearest.borrow().clone().expect("a navigator");
    assert!(!nearest.is_same(&handle));

    let next = OnNavigator(Rc::default());
    laid.pump_widget(VsyncScope::new(vsync, WidgetsApp::new(next.clone())));
    settle(&mut laid);
    let again = next.handle();
    assert!(
        again.is_mounted(),
        "the home form's navigator is in the tree"
    );
    assert!(
        !again.is_same(&handle),
        "the home form made a fresh navigator"
    );
    assert!(!nearest.is_mounted(), "the Router's navigator left with it");
}

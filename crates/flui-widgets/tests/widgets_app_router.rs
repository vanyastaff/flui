//! [`WidgetsApp::router`] (ADR-0093 §2): the app's routing subtree is a typed
//! `Router`, whose navigator is the app's only one and whose stack survives a
//! rebuild of the app.
//!
//! # Parity oracle
//!
//! Flutter's `widgets/app.dart` `_WidgetsAppState.build`: the router forms
//! mount `Router`/`Router.withConfig` as the routing subtree, bare, below
//! `Localizations` and the `builder` hook; only the navigator form wraps its
//! `Navigator` in a `FocusScope`. `WidgetsApp.router` asserts that
//! `navigatorKey` and `navigatorObservers` are not given; here the router
//! form has no such builders (the `routable_ui` compile-fail suite).

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::Vsync;
use flui_interaction::routing::FocusScopeNode;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_platform_api::Locale;
use flui_widgets::prelude::*;
use flui_widgets::{
    AppForm, ColoredBox, Directionality, FocusScope, Localizations, NavigatorHandle,
    NavigatorObserver, PageRoute, RouterError, SizedBox, Text, VsyncScope, WidgetsApp,
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

/// What `Router::handle` answered.
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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Home {
    type State = HomeState;
    fn create_state(&self) -> HomeState {
        HomeState {
            probe: self.probe.clone(),
        }
    }
}

struct HomeState {
    probe: Probe,
}

impl ViewState<Home> for HomeState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let probe = &self.probe;
        probe.inits.set(probe.inits.get() + 1);
        *probe.handle.borrow_mut() = Some(Router::<AppRoute>::handle(cx));
        *probe.nearest.borrow_mut() = NavigatorHandle::maybe_of(cx);
        *probe.root.borrow_mut() = NavigatorHandle::maybe_of_root(cx);
    }

    fn build(&self, _view: &Home, cx: &dyn BuildContext) -> impl IntoView {
        self.probe.direction.set(Directionality::maybe_of(cx));
        *self.probe.locale.borrow_mut() = Localizations::maybe_locale_of(cx);
        *self.probe.focus_scope.borrow_mut() = Some(FocusScope::of(cx));
        let router = self.probe.handle();
        RawButton::new(ColoredBox::new(Color::rgb(10, 20, 30))).on_press(move |_cx| {
            router
                .push(AppRoute::Note { id: 1 })
                .expect("BUG: Home's button fires only while its Router is mounted");
        })
    }
}

/// The page builder; `version` tags the Note page's text.
fn router(probe: &Probe, version: u32, initial: &str) -> Router<AppRoute> {
    let probe = probe.clone();
    Router::from_location(initial, move |route: &AppRoute, _cx| match route {
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

fn mount<F: AppForm>(app: WidgetsApp<F>, vsync: &Vsync) -> LaidOut {
    lay_out_animated(
        VsyncScope::new(vsync.clone(), app),
        tight(400.0, 400.0),
        vsync.clone(),
    )
}

/// How many focus scopes enclose the Home page's route scope.
fn scopes_above_home(probe: &Probe) -> usize {
    let scope = probe
        .focus_scope
        .borrow()
        .clone()
        .expect("the Home page was built");
    scope
        .as_focus_node()
        .ancestors()
        .filter(|node| node.as_scope().is_some())
        .count()
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

#[test]
fn widgets_app_router_roots_the_app_in_its_router() {
    let probe = Probe::default();
    let vsync = Vsync::new();
    let mut laid = mount(WidgetsApp::router(router(&probe, 1, "/note/3")), &vsync);
    settle(&mut laid);

    let handle = probe.handle();
    assert_eq!(handle.location().as_str(), "/note/3");
    assert!(
        laid_out_text(&laid, "v1 Note 3"),
        "the top page is laid out"
    );

    // One navigator: the Router's. `WidgetsApp::new(router)` would put the
    // Router on a page of a navigator of the app's own.
    let nearest = probe.nearest.borrow().clone().expect("a navigator");
    let root = probe.root.borrow().clone().expect("a root navigator");
    assert!(
        root.is_same(&nearest),
        "the app's root navigator is the Router's"
    );

    // Its facade refuses pages that are not route values.
    let before = root.route_ids();
    let pushed = catch_unwind(AssertUnwindSafe(|| {
        root.push(PageRoute::<()>::new(|_cx, _a, _s| {
            Text::new("Stray").into_view().boxed()
        }))
    }));
    assert_eq!(pushed.is_err(), cfg!(debug_assertions));
    settle(&mut laid);
    assert_eq!(root.route_ids(), before, "nothing entered the stack");
    assert_eq!(handle.location().as_str(), "/note/3");
    assert!(laid.find_text("Stray").is_none());
}

#[test]
fn widgets_app_router_navigates_by_handle_and_the_url_follows() {
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

#[test]
fn widgets_app_router_pages_see_localizations_and_builder() {
    let probe = Probe::default();
    let vsync = Vsync::new();
    let builder_child = Rc::new(Cell::new(None::<bool>));
    let seen = Rc::clone(&builder_child);
    let app = WidgetsApp::router(router(&probe, 1, "/"))
        .supported_locales(vec![Locale::ja_jp()])
        .builder(move |_cx, child| {
            seen.set(Some(child.is_some()));
            child.expect("the router form hands the builder its routing subtree")
        });
    let mut laid = mount(app, &vsync);
    settle(&mut laid);

    assert_eq!(builder_child.get(), Some(true), "the builder receives Some");
    assert_eq!(probe.direction.get(), Some(TextDirection::Ltr));
    assert_eq!(*probe.locale.borrow(), Some(Locale::ja_jp()));
    assert_eq!(
        probe.handle().location().as_str(),
        "/",
        "the handle resolves through the builder"
    );
}

#[test]
fn widgets_app_router_adds_no_focus_scope_above_the_router() {
    // A Router mounted with no app around it: the scopes above its page are
    // the realm's and the page's own route scope's ancestors only.
    let bare = Probe::default();
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), router(&bare, 1, "/")),
        tight(400.0, 400.0),
        vsync.clone(),
    );
    settle(&mut laid);
    let bare_depth = scopes_above_home(&bare);
    drop(laid);

    let probe = Probe::default();
    let vsync = Vsync::new();
    let mut laid = mount(WidgetsApp::router(router(&probe, 1, "/")), &vsync);
    settle(&mut laid);
    assert_eq!(
        scopes_above_home(&probe),
        bare_depth,
        "the app mounts its Router bare, as the oracle does; a FocusScope around it would \
         add a scope"
    );
}

#[test]
fn rebuilt_widgets_app_router_keeps_its_stack() {
    let probe = Probe::default();
    let vsync = Vsync::new();
    let mut laid = mount(WidgetsApp::router(router(&probe, 1, "/")), &vsync);
    settle(&mut laid);
    tap(&mut laid);
    assert!(laid_out_text(&laid, "v1 Note 1"));

    // A new `Router<AppRoute>` value, opening elsewhere, with a new builder.
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        WidgetsApp::router(router(&probe, 2, "/note/9")),
    ));
    settle(&mut laid);

    let handle = probe.handle();
    assert_eq!(
        handle.location().as_str(),
        "/note/1",
        "the Router updated in place: its stack is not the new initial one"
    );
    assert!(
        laid_out_text(&laid, "v2 Note 1"),
        "the new builder reached the page"
    );
    assert!(laid.find_text("v1 Note 1").is_none());
    assert_eq!(probe.inits.get(), 1, "Home was not remounted");
}

/// Counts attachments, the one observer callback this suite reads.
#[derive(Debug, Default)]
struct AttachCounter {
    attaches: Mutex<u32>,
}

impl NavigatorObserver for AttachCounter {
    fn did_attach(&self, _navigator: NavigatorHandle) {
        *self.attaches.lock().expect("test mutex poisoned") += 1;
    }
}

impl AttachCounter {
    fn attaches(&self) -> u32 {
        *self.attaches.lock().expect("test mutex poisoned")
    }
}

#[test]
// The two forms are two view types, so the switch remounts the shell: the
// navigator form's state is disposed, releasing its navigator and observers.
fn switching_widgets_app_from_home_to_router_releases_the_navigator() {
    let handle = NavigatorHandle::new();
    let observer = Arc::new(AttachCounter::default());
    let vsync = Vsync::new();
    let mut laid = mount(
        WidgetsApp::new(SizedBox::shrink())
            .navigator(handle.clone())
            .observer(observer.clone()),
        &vsync,
    );
    assert_eq!(observer.attaches(), 1);

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

    // The first shell let go of its navigator: switching back to the navigator
    // form with another handle mounts that one, not the first.
    let next = NavigatorHandle::new();
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        WidgetsApp::new(SizedBox::shrink()).navigator(next.clone()),
    ));
    settle(&mut laid);
    assert!(
        next.is_mounted(),
        "the new handle's navigator is in the tree"
    );
    assert!(!handle.is_mounted());
    drop(laid);

    // The shell's registration left the first handle with it: mounting that
    // handle again, with no observer configured, attaches nobody.
    let _again = mount(
        WidgetsApp::new(SizedBox::shrink()).navigator(handle.clone()),
        &Vsync::new(),
    );
    assert!(handle.is_mounted());
    assert_eq!(
        observer.attaches(),
        1,
        "a registration left behind would attach the observer again"
    );
}

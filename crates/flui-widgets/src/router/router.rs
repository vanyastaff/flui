//! [`Router`], [`RouterState`], and the state they share with a
//! [`RouterHandle`].

use std::any::type_name;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_foundation::BuildPhase;
use flui_foundation::notifier::ChangeNotifier;
use flui_view::__runtime::{UrlOwnerRegistration, UrlOwnerScope, UrlOwnerSlot};
use flui_view::element::ElementKind;
use flui_view::navigation::{
    NavigationIntent, NavigationOp, NavigationRefused, RouteParseError, RoutePath, UrlOwner,
};
use flui_view::prelude::*;
use flui_view::{AsyncDriver, BoxedView, BuildContextExt, TaskToken};

use crate::navigator::lifecycle::{RetiredValues, Terminal};

use super::handle::{RouterError, RouterHandle};
use super::routable::Routable;
use crate::navigator::{
    Navigator, NavigatorHandle, NavigatorObserver, PageRoute, RouteAnimation, RouteId,
    RouteTransitionsBuilder,
};
use crate::semantics::Semantics;

/// Builds the page for one route value.
type PageBuilder<R> = Rc<dyn Fn(&R, &dyn BuildContext) -> BoxedView>;

/// Hears an inbound path no route matches.
type UnknownHook = Rc<dyn Fn(&RoutePath)>;

/// Navigation state as a value: a stack of `R`, whose top, printed through
/// [`Routable::to_path`], is the current location.
///
/// A `Router` builds a [`Navigator`] and places one [`PageRoute`] on it per
/// value in its stack, so page transitions, heroes, `PopScope` and local
/// history work as they do on any navigator. Only `R` values enter that
/// navigator as pages; a dialog or menu is a pageless popup a page pushes
/// through its [`PageHandle`](crate::PageHandle) (ADR-0330 §2).
///
/// Drive it with the [`RouterHandle`] that [`Router::of`] returns from a
/// descendant's build context.
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::{Routable, RouteParseError, RoutePath, Router, Text};
///
/// #[derive(Clone, PartialEq)]
/// enum AppRoute { Home, About }
///
/// impl Routable for AppRoute {
///     fn to_path(&self) -> RoutePath {
///         match self {
///             Self::Home => RoutePath::root(),
///             Self::About => RoutePath::root().join("about"),
///         }
///     }
///     fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
///         match path.as_str() {
///             "/" => Ok(Self::Home),
///             "/about" => Ok(Self::About),
///             _ => Err(RouteParseError::NoMatch { path: path.clone() }),
///         }
///     }
/// }
///
/// let router = Router::new(AppRoute::Home, |route: &AppRoute, _ctx| match route {
///     AppRoute::Home => Text::new("Home").into_view().boxed(),
///     AppRoute::About => Text::new("About").into_view().boxed(),
/// });
/// # let _ = router;
/// ```
///
/// # The presentation's URL
///
/// The outermost `Router` of a presentation's tree owns its URL (ADR-0330
/// §5): a location addressed to the presentation from outside its widgets,
/// such as a URL the platform opens for its window, is parsed with
/// [`Routable::from_path`] and applied to this stack, as Flutter's Router
/// applies what its route information provider reports. A path no route
/// matches is reported to [`on_unknown`](Self::on_unknown) and leaves the
/// stack unchanged. A `Router` inside another's page owns no URL: its stack
/// belongs to that page.
pub struct Router<R: Routable> {
    /// The stack the router opens with, bottom to top; never empty.
    initial: Vec<R>,
    page: Terminal<PageBuilder<R>>,
    transitions: Option<RouteTransitionsBuilder>,
    transition_duration: Option<Duration>,
    on_unknown: Option<UnknownHook>,
}

impl<R: Routable> Drop for Router<R> {
    fn drop(&mut self) {
        let initial = RetiredValues(std::mem::take(&mut self.initial));
        let page = self.page.withdraw();
        let transitions = Terminal::new(self.transitions.take());
        let on_unknown = Terminal::new(self.on_unknown.take());
        drop((initial, page, transitions, on_unknown));
    }
}

impl<R: Routable> Clone for Router<R> {
    fn clone(&self) -> Self {
        Self {
            initial: clone_routes(self.initial.iter()),
            page: Terminal::new(Rc::clone(&self.page)),
            transitions: self.transitions.clone(),
            transition_duration: self.transition_duration,
            on_unknown: self.on_unknown.clone(),
        }
    }
}

/// Clones of `routes`, in order. A clone that panics retires the clones
/// already made instead of unwind-dropping them.
pub(super) fn clone_routes<'a, R: Clone + 'a>(
    routes: impl ExactSizeIterator<Item = &'a R>,
) -> Vec<R> {
    let mut cloned = RetiredValues(Vec::with_capacity(routes.len()));
    for route in routes {
        cloned.0.push(route.clone());
    }
    std::mem::take(&mut cloned.0)
}

/// The printed path of each of `routes`, in order: how `Debug` shows a stack.
pub(super) fn paths<'a, R: Routable + 'a>(routes: impl Iterator<Item = &'a R>) -> Vec<String> {
    routes.map(|route| route.to_path().to_string()).collect()
}

impl<R: Routable> fmt::Debug for Router<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Router")
            .field("route_type", &type_name::<R>())
            .field("initial", &paths(self.initial.iter()))
            .finish_non_exhaustive()
    }
}

impl<R: Routable> Router<R> {
    /// A router opening on `initial` alone; `page` builds each route's page.
    #[must_use]
    pub fn new(initial: R, page: impl Fn(&R, &dyn BuildContext) -> BoxedView + 'static) -> Self {
        Self {
            initial: vec![initial],
            page: Terminal::new(Rc::new(page)),
            transitions: None,
            transition_duration: None,
            on_unknown: None,
        }
    }

    /// A router opening at `location`, with the back-stack
    /// [`Routable::back_stack`] derives from it — so `/note/1` opens on `Home`
    /// with `Note 1` above it.
    ///
    /// # Errors
    ///
    /// The [`RouteParseError`] `location` produced; no router is built.
    pub fn from_location(
        location: &str,
        page: impl Fn(&R, &dyn BuildContext) -> BoxedView + 'static,
    ) -> Result<Self, RouteParseError> {
        let page: Terminal<PageBuilder<R>> = Terminal::new(Rc::new(page));
        let initial = R::back_stack(&RoutePath::parse(location)?)?;
        Ok(Self {
            initial,
            page,
            transitions: None,
            transition_duration: None,
            on_unknown: None,
        })
    }

    /// A router opening on `stack`, bottom to top: the stack
    /// [`RouterHandle::stack`] read when the application last ran, so Back
    /// leads through the same pages it did then.
    ///
    /// Every route becomes a page, bottom to top, without a transition; the
    /// top is the location the router opens at.
    ///
    /// The cost grows with the stack: the first frame builds all `N` pages,
    /// and every covered page then stays mounted with its state, since a
    /// router's pages keep their state while covered
    /// ([`PageRoute::maintain_state`]). An application that saves deep
    /// stacks may trim what it restores.
    ///
    /// # Errors
    ///
    /// [`RouterError::EmptyStack`] for an empty `stack`; no router is built.
    pub fn from_stack(
        stack: Vec<R>,
        page: impl Fn(&R, &dyn BuildContext) -> BoxedView + 'static,
    ) -> Result<Self, RouterError> {
        let page: Terminal<PageBuilder<R>> = Terminal::new(Rc::new(page));
        if stack.is_empty() {
            return Err(RouterError::EmptyStack);
        }
        Ok(Self {
            initial: stack,
            page,
            transitions: None,
            transition_duration: None,
            on_unknown: None,
        })
    }

    /// Wrap each page in its entrance and exit transition — the
    /// [`PageRoute::transitions`] of every page on this router's stack. A
    /// parent rebuild with new transitions reaches the pages placed with
    /// transitions. Without them, a page adopts the ambient
    /// [`PageTransitions`](crate::PageTransitions) page builder, and keeps it.
    #[must_use]
    pub fn transitions(
        mut self,
        transitions: impl Fn(
            &dyn BuildContext,
            &RouteAnimation,
            &RouteAnimation,
            BoxedView,
        ) -> BoxedView
        + 'static,
    ) -> Self {
        self.transitions = Some(Rc::new(transitions));
        self
    }

    /// The [`PageRoute::transition_duration`] of every page this router places.
    /// A page keeps the duration it was placed with: a parent rebuild with a
    /// new one reaches only the pages placed after it.
    #[must_use]
    pub fn transition_duration(mut self, duration: Duration) -> Self {
        self.transition_duration = Some(duration);
        self
    }

    /// Hear an inbound path no route matches: a URL the platform opened for
    /// the presentation whose URL this router owns. The stack is left as it
    /// is and the navigation is refused; the hook may navigate itself, to a
    /// not-found page for example. A parent rebuild with a new hook replaces
    /// it.
    #[must_use]
    pub fn on_unknown(mut self, hook: impl Fn(&RoutePath) + 'static) -> Self {
        self.on_unknown = Some(Rc::new(hook));
        self
    }

    /// A handle to the nearest ancestor `Router<R>`, Flutter's
    /// `GoRouter.of(context)` (ADR-0330 §3).
    ///
    /// Call it in `build` and move the handle into the callbacks that
    /// navigate: a callback runs after the build, with no position in the
    /// tree, so it captures the owned handle where a Flutter callback keeps
    /// its context. The lookup registers a dependency on the router, so the
    /// widget rebuilds with the right handle if it moves under another one. A
    /// state that keeps a handle resolves it in `did_change_dependencies`.
    ///
    /// ```
    /// # use flui_widgets::prelude::*; use flui_widgets::{Router, Routable};
    /// fn open<R: Routable>(ctx: &dyn BuildContext, route: R) -> impl Fn() + 'static {
    ///     let router = Router::<R>::of(ctx);
    ///     move || {
    ///         if let Ok(router) = &router {
    ///             let _ = router.push(route.clone());
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// [`RouterError::NoRouter`] when no `Router<R>` is above `ctx`.
    pub fn of(ctx: &dyn BuildContext) -> Result<RouterHandle<R>, RouterError> {
        ctx.depend_on::<RouterScope<R>, _>(|scope| RouterHandle::new(Rc::clone(&scope.shared)))
            .ok_or(RouterError::NoRouter {
                route_type: type_name::<R>(),
            })
    }
}

/// Provides a router's shared state to [`Router::of`] lookups below it.
struct RouterScope<R: Routable> {
    shared: Rc<RouterShared<R>>,
    child: Terminal<BoxedView>,
}

impl<R: Routable> Clone for RouterScope<R> {
    fn clone(&self) -> Self {
        Self {
            shared: Rc::clone(&self.shared),
            child: Terminal::new(self.child.clone()),
        }
    }
}

impl<R: Routable> Drop for RouterScope<R> {
    fn drop(&mut self) {
        let child = self.child.withdraw();
        drop(child);
    }
}

impl<R: Routable> InheritedView for RouterScope<R> {
    type Data = Rc<RouterShared<R>>;

    fn data(&self) -> &Self::Data {
        &self.shared
    }

    fn child(&self) -> &dyn View {
        &*self.child
    }

    /// A router's element keeps one shared state for its life, so this
    /// compares the same state to itself; identity, not structure, is what a
    /// dependent's handle names.
    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.shared, &old.shared)
    }
}

impl<R: Routable> View for RouterScope<R> {
    fn create_element(self: Rc<Self>) -> ElementKind {
        ElementKind::inherited(self)
    }
}

impl<R: Routable> View for Router<R> {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl<R: Routable> StatefulView for Router<R> {
    type State = RouterState<R>;

    /// The state is born with its build phase and, when this is the
    /// presentation's outermost router, its place in the presentation's
    /// URL-owner slot.
    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        RouterState::new(self, ctx.build_phase(), UrlOwnerScope::maybe_of(ctx))
    }
}

impl<R: Routable> RouterState<R> {
    /// The state of `router`: its shared stack, observer and a copy of its
    /// initial routes, registered as the URL owner of `url_slot` when there
    /// is one.
    fn new(router: &Router<R>, build_phase: BuildPhase, url_slot: Option<UrlOwnerSlot>) -> Self {
        let shared = Rc::new(RouterShared {
            navigator: Terminal::new(NavigatorHandle::addressed(
                type_name::<R>(),
                build_phase.clone(),
            )),
            stack: RefCell::new(Vec::new()),
            page: Terminal::new(Rc::new(RefCell::new(Rc::clone(&router.page)))),
            transitions: Terminal::new(Rc::new(RefCell::new(router.transitions.clone()))),
            transition_duration: Cell::new(router.transition_duration),
            mounted: Cell::new(false),
            editing: Cell::new(0),
            build_phase,
            announcer: StackAnnouncer::default(),
            on_unknown: RefCell::new(router.on_unknown.clone()),
        });
        let observer: Rc<dyn NavigatorObserver> = Rc::new(RouterObserver {
            shared: Rc::downgrade(&shared),
        });
        let url_owner: Rc<dyn UrlOwner> = Rc::new(RouterUrlOwner {
            shared: Rc::downgrade(&shared),
        });
        let url = url_slot.map(|slot| slot.register(&url_owner));
        RouterState {
            shared: Terminal::new(shared),
            observer: Terminal::new(observer),
            url_owner,
            url,
            initial: clone_routes(router.initial.iter()),
        }
    }
}

/// Persistent state for [`Router`]. Opaque: it is public only because
/// [`StatefulView::State`] names it.
pub struct RouterState<R: Routable> {
    shared: Terminal<Rc<RouterShared<R>>>,
    observer: Terminal<Rc<dyn NavigatorObserver>>,
    /// What the presentation's URL-owner slot reaches this router through.
    url_owner: Rc<dyn UrlOwner>,
    /// This router's place in its presentation's URL-owner slot; `None` for a
    /// router nested in another's page, or one mounted outside a runtime.
    url: Option<UrlOwnerRegistration>,
    /// Seeded in `init_state`, which receives no view.
    initial: Vec<R>,
}

impl<R: Routable> fmt::Debug for RouterState<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let routes = self.shared.routes();
        f.debug_struct("RouterState")
            .field("stack", &paths(routes.0.iter().map(|route| &**route)))
            .finish_non_exhaustive()
    }
}

impl<R: Routable> Drop for RouterState<R> {
    fn drop(&mut self) {
        let shared = self.shared.withdraw();
        let observer = self.observer.withdraw();
        let initial = RetiredValues(std::mem::take(&mut self.initial));
        drop((shared, observer, initial));
    }
}

impl<R: Routable> RouterState<R> {
    /// Seed each initial value as a page, bottom to top.
    ///
    /// Pages seeded before a failure (a route-identity refusal, a panicking
    /// page builder) stay committed; the value being seeded and the unseeded
    /// tail are destroyed normally by the unwind (ADR-0281).
    fn seed_initial_pages(&mut self) {
        for route in std::mem::take(&mut self.initial) {
            let page = self.shared.page_route::<()>(&route);
            let id = self.shared.navigator.seed_page(page);
            self.shared
                .stack
                .borrow_mut()
                .push(RouterEntry::new(id, route));
        }
    }
}

impl<R: Routable> ViewState<Router<R>> for RouterState<R> {
    /// Observe the navigator before it mounts, so every pop it makes — from
    /// this router's handle, the facade, a back gesture or a barrier — reaches
    /// the stack; then seed the initial pages, which the navigator flushes
    /// once when it mounts.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shared.mounted.set(true);
        self.shared
            .navigator
            .add_observer(Rc::clone(&self.observer));
        self.seed_initial_pages();
        // The stack it opens on is not a change.
        let announcer = &self.shared.announcer;
        *announcer.driver.borrow_mut() = ctx.async_driver();
        *announcer.told.borrow_mut() = self.shared.page_ids();
        // An unmounted router tells nobody again. Its queued delivery and its
        // listeners go at the unmount, not with the shared state: a listener
        // that holds a handle would keep that state, and so itself, alive.
        // The delivery goes first, so a listener capture's `Drop` that panics
        // leaves nothing queued.
        let shared = Rc::downgrade(&*self.shared);
        ctx.on_unmount(Box::new(move || {
            let Some(shared) = shared.upgrade() else {
                return;
            };
            let delivery = shared.announcer.delivery.borrow_mut().take();
            drop(delivery);
            // The concrete owner's clear: the router's listeners go at the
            // unmount, and the channel stays open for a later remount.
            shared.announcer.notifier.remove_all_listeners();
        }))
        .forget();
    }

    /// Follow the presentation's URL-owner slot if the router moved: leave
    /// the old slot, then join the new one.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let slot = UrlOwnerScope::maybe_of(ctx);
        let unchanged = match (&self.url, &slot) {
            (Some(registration), Some(slot)) => registration.is_in(slot),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        drop(self.url.take());
        self.url = slot.map(|slot| slot.register(&self.url_owner));
    }

    /// The pages sit below a scope with no URL-owner slot: a router inside
    /// one of them owns no URL.
    fn build(&self, _view: &Router<R>, _ctx: &dyn BuildContext) -> impl IntoView {
        RouterScope {
            shared: Rc::clone(&self.shared),
            child: Terminal::new(
                UrlOwnerScope::none(Navigator::new(self.shared.navigator.clone())).boxed(),
            ),
        }
    }

    /// Take the new page builder and transitions, and rebuild every page so a
    /// parent rebuild or hot reload reaches the pages already on the stack. A
    /// new transition duration reaches only the pages placed afterwards: a
    /// page's animation controller is made with its duration. A changed
    /// initial stack is ignored after mount.
    fn did_update_view(&mut self, _old_view: &Router<R>, new_view: &Router<R>) {
        *self.shared.page.borrow_mut() = Rc::clone(&new_view.page);
        self.shared
            .transitions
            .borrow_mut()
            .clone_from(&new_view.transitions);
        self.shared
            .transition_duration
            .set(new_view.transition_duration);
        let old_hook = self.shared.on_unknown.replace(new_view.on_unknown.clone());
        drop(old_hook);
        let ids: Vec<RouteId> = self.shared.stack.borrow().iter().map(|e| e.id).collect();
        for id in ids {
            self.shared.navigator.mark_route_needs_build(id);
        }
    }

    /// Leave the URL-owner slot with the tree, so a sibling router still
    /// mounted takes the URL at once.
    fn dispose(&mut self) {
        self.shared.mounted.set(false);
        drop(self.url.take());
        self.shared.navigator.remove_observer(&self.observer);
        // The hook may hold a handle to this router, which would keep its
        // shared state alive past the unmount; it is retired here, out of
        // its cell, rather than with the shared state.
        drop(Terminal::new(self.shared.on_unknown.take()));
    }
}

/// One page on the router's stack.
pub(super) struct RouterEntry<R> {
    pub(super) id: RouteId,
    /// Shared so a reader can hold the route past the stack's borrow and run
    /// user code on it (`Clone`, `to_path`) there: see
    /// [`RouterShared::routes`].
    pub(super) route: Rc<R>,
}

impl<R> RouterEntry<R> {
    pub(super) fn new(id: RouteId, route: R) -> Self {
        Self {
            id,
            route: Rc::new(route),
        }
    }
}

/// Outgoing route values are retired without a stack borrow, each under
/// containment: a failing destructor does not skip the rest, and the first
/// failure is raised (ADR-0281 §3).
pub(super) struct RouterRetiredValues<T>(pub(super) Vec<T>);

impl<T> Drop for RouterRetiredValues<T> {
    fn drop(&mut self) {
        crate::support::retirement::retire_values(std::mem::take(&mut self.0));
    }
}

/// What a [`RouterState`] and every [`RouterHandle`] to it share.
pub(super) struct RouterShared<R: Routable> {
    /// The navigator the pages live on, addressed so its facade refuses pages
    /// that are not `R`.
    pub(super) navigator: Terminal<NavigatorHandle>,
    /// One entry per page on `navigator`, bottom to top; never empty while
    /// mounted. Popups are not here.
    pub(super) stack: RefCell<Vec<RouterEntry<R>>>,
    /// Shared with every page's builder, so a new builder reaches pages
    /// already on the stack.
    page: Terminal<Rc<RefCell<PageBuilder<R>>>>,
    /// Shared with every page's transitions, as `page` is.
    transitions: Terminal<Rc<RefCell<Option<RouteTransitionsBuilder>>>>,
    /// Read when a page is placed; a page keeps the duration it was placed
    /// with.
    transition_duration: Cell<Option<Duration>>,
    pub(super) mounted: Cell<bool>,
    /// The UI runtime's build phase, bound when the router's state is born: an edit
    /// made while it builds is refused (ADR-0330 §3).
    pub(super) build_phase: BuildPhase,
    /// How many edits are running: a navigation addressed to the presentation
    /// that arrives inside one is refused as busy rather than nested in it.
    pub(super) editing: Cell<usize>,
    /// Tells the listeners of every [`RouterHandle`] of stack changes.
    pub(super) announcer: StackAnnouncer,
    /// Hears an inbound path no route matches.
    on_unknown: RefCell<Option<UnknownHook>>,
}

/// One running edit of a router, counted while it lives: see
/// [`RouterShared::editing`]. It ends on every exit, an unwind included.
pub(super) struct Editing<'a>(&'a Cell<usize>);

impl<'a> Editing<'a> {
    pub(super) fn begin(count: &'a Cell<usize>) -> Self {
        // One count per nested edit on this thread's stack, so it cannot
        // reach `usize::MAX`.
        count.set(count.get() + 1);
        Self(count)
    }
}

impl Drop for Editing<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

/// What a presentation's URL-owner slot reaches a router through: it parses
/// the intent's path into `R` and applies it as the router's own handle does.
struct RouterUrlOwner<R: Routable> {
    shared: Weak<RouterShared<R>>,
}

impl<R: Routable> RouterUrlOwner<R> {
    /// Tell the router's `on_unknown` hook of `path` and refuse it. The hook
    /// runs with nothing borrowed, so it may navigate.
    fn unknown(shared: &RouterShared<R>, path: RoutePath) -> NavigationRefused {
        let hook = shared.on_unknown.borrow().clone();
        if let Some(hook) = hook {
            hook(&path);
        }
        NavigationRefused::NoMatch(path)
    }

    /// The refusal for an edit the handle refused.
    fn refused(error: RouterError) -> NavigationRefused {
        match error {
            RouterError::DuringBuild => NavigationRefused::Busy,
            // A handle edit parses nothing but `go`'s location, which
            // `navigate` reports as unknown before it gets here.
            RouterError::Parse(_)
            | RouterError::Disposed
            | RouterError::NoRouter { .. }
            | RouterError::EmptyStack => NavigationRefused::NoUrlOwner,
        }
    }
}

/// Why an intent was not applied, before the unknown-path hook is told.
enum Refusal {
    /// No route matches the path: `on_unknown` hears of it.
    Unknown(RoutePath),
    /// The handle refused the edit.
    Edit(RouterError),
    /// Refused as it stands.
    Refused(NavigationRefused),
}

impl<R: Routable> UrlOwner for RouterUrlOwner<R> {
    fn navigate(&self, intent: NavigationIntent) -> Result<(), NavigationRefused> {
        let Some(shared) = self.shared.upgrade() else {
            return Err(NavigationRefused::NoUrlOwner);
        };
        if shared.editing.get() > 0 {
            return Err(NavigationRefused::Busy);
        }
        let handle = RouterHandle::new(Rc::clone(&shared));
        let NavigationIntent { op, .. } = intent;
        let applied = {
            // One edit covers the parse and the change, so a parser that
            // navigates again is `Busy`; it ends before `on_unknown` runs, so
            // the hook may redirect.
            let _editing = handle.editable().map_err(Self::refused)?;
            match op {
                NavigationOp::Push(path) => match R::from_path(&path) {
                    Ok(route) => handle.push(route).map_err(Refusal::Edit),
                    Err(_) => Err(Refusal::Unknown(path)),
                },
                NavigationOp::Replace(path) => match R::from_path(&path) {
                    Ok(route) => handle.replace(route).map_err(Refusal::Edit),
                    Err(_) => Err(Refusal::Unknown(path)),
                },
                NavigationOp::Go(path) => handle.go_path(&path).map_err(|error| match error {
                    RouterError::Parse(_) => Refusal::Unknown(path),
                    other => Refusal::Edit(other),
                }),
                // The Back key's path: a `PopScope` that refuses keeps its
                // page and the pop counts as handled; the last page is
                // left to the platform.
                NavigationOp::Pop => {
                    if shared.navigator.maybe_pop() {
                        Ok(())
                    } else {
                        Err(Refusal::Refused(NavigationRefused::NothingToPop))
                    }
                }
                // An operation added after this router was written has
                // nothing here that applies it.
                _ => Err(Refusal::Refused(NavigationRefused::NoUrlOwner)),
            }
        };
        applied.map_err(|refusal| match refusal {
            Refusal::Unknown(path) => Self::unknown(&shared, path),
            Refusal::Edit(error) => Self::refused(error),
            Refusal::Refused(refused) => refused,
        })
    }
}

/// Tells a router's listeners of committed stack changes, one delivery at a
/// time on the UI runtime's tasks.
#[derive(Default)]
pub(super) struct StackAnnouncer {
    pub(super) notifier: ChangeNotifier,
    /// The pages the listeners were last told of, bottom to top.
    told: RefCell<Vec<RouteId>>,
    /// Whether a delivery is queued and has not started yet.
    queued: Cell<bool>,
    /// Where deliveries run; set when the router mounts.
    driver: RefCell<Option<AsyncDriver>>,
    /// The latest delivery's task.
    delivery: RefCell<Option<TaskToken>>,
}

impl<R: Routable> Drop for RouterShared<R> {
    fn drop(&mut self) {
        self.mounted.set(false);
        let navigator = self.navigator.withdraw();
        let stack = RetiredValues(std::mem::take(self.stack.get_mut()));
        let page = self.page.withdraw();
        let transitions = self.transitions.withdraw();
        let delivery = self.announcer.delivery.get_mut().take();
        let on_unknown = Terminal::new(self.on_unknown.get_mut().take());
        drop((navigator, stack, page, transitions, delivery, on_unknown));
    }
}

impl<R: Routable> RouterShared<R> {
    /// The page route for `route`: a [`PageRoute`] named with its path, whose
    /// page scopes a semantics route (and names it, when `route` has a label).
    pub(super) fn page_route<T: 'static>(&self, route: &R) -> PageRoute<T> {
        let builder = Terminal::new(Rc::clone(&self.page));
        let value = Terminal::new(route.clone());
        let label = route.semantics_label();
        let mut page = PageRoute::<T>::new(move |ctx, _animation, _secondary| {
            let page = builder.borrow().clone();
            let mut semantics = Semantics::new()
                .scopes_route(true)
                .explicit_child_nodes(true);
            if let Some(label) = &label {
                semantics = semantics.names_route(true).label(label.clone());
            }
            semantics.child(page(&value, ctx)).boxed()
        })
        .named(route.to_path().as_str());
        // A router with transitions gives every page it places a builder
        // that reads them at each build, so new transitions reach this page.
        // A router with none leaves the page to adopt the ambient
        // `PageTransitions` page builder when its navigator installs it, as
        // any page given no transitions does (ADR-0221 D2): under a
        // `MaterialApp` a Router's pages run the theme's transition.
        if self.transitions.borrow().is_some() {
            let transitions = Terminal::new(Rc::clone(&self.transitions));
            page = page.transitions(move |ctx, animation, secondary, child| {
                let current = transitions.borrow().clone();
                match current {
                    Some(transitions) => transitions(ctx, animation, secondary, child),
                    None => child,
                }
            });
        }
        if let Some(duration) = self.transition_duration.get() {
            page = page.transition_duration(duration);
        }
        page
    }

    /// The routes on the stack, bottom to top, taken under a borrow that is
    /// released before this returns: a route's `Clone` or `to_path` run on
    /// them may drive this router. Retired without unwind-dropping a route.
    pub(super) fn routes(&self) -> RouterRetiredValues<Rc<R>> {
        RouterRetiredValues(
            self.stack
                .borrow()
                .iter()
                .map(|entry| Rc::clone(&entry.route))
                .collect(),
        )
    }

    /// Each page's id and route, bottom to top, taken as
    /// [`routes`](Self::routes) takes the routes.
    pub(super) fn pages(&self) -> RouterRetiredValues<(RouteId, Rc<R>)> {
        RouterRetiredValues(
            self.stack
                .borrow()
                .iter()
                .map(|entry| (entry.id, Rc::clone(&entry.route)))
                .collect(),
        )
    }

    /// The top route's path; `None` only before the first page is seeded.
    pub(super) fn location(&self) -> Option<RoutePath> {
        let routes = self.routes();
        routes.0.last().map(|route| route.to_path())
    }

    /// Drop the page `id` names, if it is one of this router's.
    fn forget(self: &Rc<Self>, id: RouteId) {
        let _told = self.announce_on_exit();
        let retired = {
            let mut stack = self.stack.borrow_mut();
            stack
                .iter()
                .position(|entry| entry.id == id)
                .map(|index| stack.remove(index))
        };
        drop(RouterRetiredValues(retired.into_iter().collect()));
    }

    /// The ids of the pages on the stack, bottom to top.
    fn page_ids(&self) -> Vec<RouteId> {
        self.stack.borrow().iter().map(|entry| entry.id).collect()
    }

    /// Queue a delivery to the listeners if the stack is no longer the one
    /// they were told of and none is queued. Runs no listener: they run in
    /// the delivery, on a later turn of the runtime's tasks.
    ///
    /// It raises only a panic of the frame hook its spawn wakes, and never
    /// while a panic unwinds, so it may run then: a state it cannot read now
    /// is mid-edit, and that edit announces when it ends.
    pub(super) fn announce(self: &Rc<Self>) {
        let announcer = &self.announcer;
        if announcer.queued.get() {
            return;
        }
        let (Ok(told), Ok(stack)) = (announcer.told.try_borrow(), self.stack.try_borrow()) else {
            return;
        };
        if told.iter().copied().eq(stack.iter().map(|entry| entry.id)) {
            return;
        }
        drop((told, stack));
        let Some(driver) = announcer
            .driver
            .try_borrow()
            .ok()
            .and_then(|driver| driver.clone())
        else {
            return;
        };
        let Ok(mut delivery) = announcer.delivery.try_borrow_mut() else {
            return;
        };
        let shared = Rc::downgrade(self);
        // Spawning wakes the frame hook, which may panic; the task is then
        // cancelled, and nothing stays queued, so the next announcement tries
        // again. While a panic already unwinds, the hook's is kept, not raised.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the frame hook woken by spawn runs contained; nothing stays queued"
        )]
        let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            driver.spawn_local(Box::pin(async move { deliver(&shared) }))
        }));
        let token = match spawned {
            Ok(token) => token,
            Err(payload) => {
                drop(delivery);
                if std::thread::panicking() {
                    flui_foundation::panic::discard_payload(payload);
                    return;
                }
                std::panic::resume_unwind(payload);
            }
        };
        announcer.queued.set(true);
        // The task it replaces has run, or is the delivery running now,
        // whose cancellation is honoured once its poll returns.
        let finished = delivery.replace(token);
        drop(delivery);
        drop(finished);
    }

    /// A guard that [announces](Self::announce) the stack when it drops,
    /// whether the edit it covers returns or unwinds: an edit that commits
    /// and then runs user code that panics (an observer, a retired route's
    /// `Drop`) is still told.
    pub(super) fn announce_on_exit(self: &Rc<Self>) -> AnnounceOnExit<'_, R> {
        AnnounceOnExit(self)
    }
}

/// See [`RouterShared::announce_on_exit`].
pub(super) struct AnnounceOnExit<'a, R: Routable>(&'a Rc<RouterShared<R>>);

impl<R: Routable> Drop for AnnounceOnExit<'_, R> {
    fn drop(&mut self) {
        self.0.announce();
    }
}

/// Tell `shared`'s listeners of the stack, if it still differs from the one
/// they were last told of.
fn deliver<R: Routable>(shared: &Weak<RouterShared<R>>) {
    let Some(shared) = shared.upgrade() else {
        return;
    };
    let announcer = &shared.announcer;
    announcer.queued.set(false);
    let now = shared.page_ids();
    let changed = *announcer.told.borrow() != now;
    *announcer.told.borrow_mut() = now;
    // Recorded before the listeners run: a navigation they make queues the
    // next delivery, and a panic in one leaves the next delivery to come.
    if changed && shared.mounted.get() {
        announcer.notifier.notify_listeners();
    }
}

/// Keeps a router's stack in step with every pop its navigator makes.
struct RouterObserver<R: Routable> {
    shared: Weak<RouterShared<R>>,
}

impl<R: Routable> NavigatorObserver for RouterObserver<R> {
    fn did_pop(&self, route: RouteId, _previous: Option<RouteId>) {
        if let Some(shared) = self.shared.upgrade() {
            shared.forget(route);
        }
    }

    fn did_remove(&self, route: RouteId, _previous: Option<RouteId>) {
        if let Some(shared) = self.shared.upgrade() {
            shared.forget(route);
        }
    }
}

#[cfg(test)]
mod seed_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::navigator::RouteId;
    use crate::support::child_process::child_test;

    /// A route value that counts its destruction once armed, unless it is the
    /// base page that seeds successfully.
    #[derive(Clone)]
    struct InitialValue {
        index: usize,
        armed: Arc<AtomicBool>,
        drops: Arc<AtomicUsize>,
    }

    impl PartialEq for InitialValue {
        fn eq(&self, other: &Self) -> bool {
            self.index == other.index
        }
    }

    impl Drop for InitialValue {
        fn drop(&mut self) {
            if self.armed.load(Ordering::SeqCst) && self.index > 0 {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl Routable for InitialValue {
        fn to_path(&self) -> RoutePath {
            RoutePath::root().join(self.index)
        }

        fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
            Err(RouteParseError::NoMatch { path: path.clone() })
        }
    }

    child_test! {
        /// Route-identity refusal while seeding the initial stack keeps the
        /// pages already seeded, and destroys the value being seeded and the
        /// unseeded tail (ADR-0281).
        fn seeding_refusal_destroys_the_initial_tail() {
            let armed = Arc::new(AtomicBool::new(false));
            let drops = Arc::new(AtomicUsize::new(0));
            let value = |index| InitialValue {
                index,
                armed: Arc::clone(&armed),
                drops: Arc::clone(&drops),
            };
            let mut router = Router::new(value(0), |_, _| crate::Text::new("page").boxed());
            router.initial.extend([value(1), value(2)]);
            let mut state = RouterState::new(&router, BuildPhase::default(), None);
            drop(router);
            RouteId::leave_process_identities(1);
            armed.store(true, Ordering::SeqCst);
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                state.seed_initial_pages();
            }))
            .expect_err("the second seed is refused");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("BUG: route identity capacity exhausted"),
                "the capacity failure stays authoritative"
            );
            // The refused value, the clone its page holds, and the tail.
            assert_eq!(drops.load(Ordering::SeqCst), 3, "the refused value and the tail are destroyed");
            armed.store(false, Ordering::SeqCst);
            assert_eq!(state.shared.stack.borrow().len(), 1, "the seeded page stays committed");
            assert_eq!(state.shared.navigator.route_ids().len(), 1);
            drop(state);
        }
    }

    /// A route whose copy fails for the second route of a stack, and counts
    /// the destruction of the copies made before it.
    struct CloneFails {
        second: bool,
        copied: bool,
        copies_dropped: Arc<AtomicUsize>,
    }

    impl Clone for CloneFails {
        fn clone(&self) -> Self {
            assert!(!self.second, "second route clone failure");
            Self {
                second: false,
                copied: true,
                copies_dropped: Arc::clone(&self.copies_dropped),
            }
        }
    }

    impl Drop for CloneFails {
        fn drop(&mut self) {
            if self.copied {
                self.copies_dropped.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl PartialEq for CloneFails {
        fn eq(&self, other: &Self) -> bool {
            self.second == other.second
        }
    }

    impl Routable for CloneFails {
        fn to_path(&self) -> RoutePath {
            RoutePath::root()
        }

        fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
            Err(RouteParseError::NoMatch { path: path.clone() })
        }
    }

    child_test! {
        /// A copy of the initial stack that fails while the state is made
        /// destroys the copies made before it (ADR-0281), and the router
        /// keeps its own stack.
        fn state_construction_destroys_the_copies_before_a_failing_clone() {
            let copies_dropped = Arc::new(AtomicUsize::new(0));
            let route = |second| CloneFails {
                second,
                copied: false,
                copies_dropped: Arc::clone(&copies_dropped),
            };
            let mut router = Router::new(route(false), |_, _| crate::Text::new("page").boxed());
            router.initial.push(route(true));
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drop(RouterState::new(&router, BuildPhase::default(), None));
            }))
            .expect_err("the second route's copy fails");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("second route clone failure")
            );
            assert_eq!(
                copies_dropped.load(Ordering::SeqCst),
                1,
                "the copy made before the failing clone is destroyed by its unwind"
            );
            assert_eq!(router.initial.len(), 2, "the router keeps its own stack");
        }
    }
}

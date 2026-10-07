//! [`Router`], [`RouterState`], and the state they share with a
//! [`RouterHandle`].

use std::any::type_name;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, BuildContextExt};

use crate::navigator::lifecycle::{RetiredValues, Terminal};

use super::handle::{RouterError, RouterHandle};
use super::path::{RouteParseError, RoutePath};
use super::routable::Routable;
use crate::navigator::{
    Navigator, NavigatorHandle, NavigatorObserver, PageRoute, RouteAnimation, RouteId,
    RouteTransitionsBuilder,
};
use crate::semantics::Semantics;

/// Builds the page for one route value.
type PageBuilder<R> = Rc<dyn Fn(&R, &dyn BuildContext) -> BoxedView>;

/// Navigation state as a value: a stack of `R`, whose top, printed through
/// [`Routable::to_path`], is the current location.
///
/// A `Router` builds a [`Navigator`] and places one [`PageRoute`] on it per
/// value in its stack, so page transitions, heroes, `PopScope` and local
/// history work as they do on any navigator. Only `R` values enter that
/// navigator as pages: its facade ([`NavigatorHandle`]) refuses any other page
/// and admits only pageless popups such as dialogs (ADR-0093 §4).
///
/// Drive it with the [`RouterHandle`] that [`Router::handle`] returns from a
/// descendant's `init_state`.
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
/// let router = Router::new(AppRoute::Home, |route: &AppRoute, _cx| match route {
///     AppRoute::Home => Text::new("Home").into_view().boxed(),
///     AppRoute::About => Text::new("About").into_view().boxed(),
/// });
/// # let _ = router;
/// ```
pub struct Router<R: Routable> {
    /// The stack the router opens with, bottom to top; never empty.
    initial: Vec<R>,
    page: Terminal<PageBuilder<R>>,
    transitions: Option<RouteTransitionsBuilder>,
    transition_duration: Option<Duration>,
}

impl<R: Routable> Drop for Router<R> {
    fn drop(&mut self) {
        let initial = RetiredValues(std::mem::take(&mut self.initial));
        let page = self.page.withdraw();
        let transitions = Terminal::new(self.transitions.take());
        drop((initial, page, transitions));
    }
}

impl<R: Routable> Clone for Router<R> {
    fn clone(&self) -> Self {
        Self {
            initial: clone_routes(&self.initial),
            page: Terminal::new(Rc::clone(&self.page)),
            transitions: self.transitions.clone(),
            transition_duration: self.transition_duration,
        }
    }
}

fn clone_routes<R: Clone>(routes: &[R]) -> Vec<R> {
    let mut cloned = RetiredValues(Vec::with_capacity(routes.len()));
    for route in routes {
        cloned.0.push(route.clone());
    }
    std::mem::take(&mut cloned.0)
}

impl<R: Routable> fmt::Debug for Router<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Router")
            .field("route_type", &type_name::<R>())
            .field(
                "initial",
                &self.initial.last().map(|route| route.to_path().to_string()),
            )
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
        })
    }

    /// A router opening on `stack`, bottom to top: the stack
    /// [`RouterHandle::stack`] read when the application last ran, so Back
    /// leads through the same pages it did then.
    ///
    /// Not yet the whole stack: the router opens on the top route alone.
    ///
    /// # Errors
    ///
    /// [`RouterError::EmptyStack`] for an empty `stack`; no router is built.
    pub fn from_stack(
        stack: Vec<R>,
        page: impl Fn(&R, &dyn BuildContext) -> BoxedView + 'static,
    ) -> Result<Self, RouterError> {
        let page: Terminal<PageBuilder<R>> = Terminal::new(Rc::new(page));
        let mut stack = RetiredValues(stack);
        let top = stack.0.pop().ok_or(RouterError::EmptyStack)?;
        drop(stack);
        Ok(Self {
            initial: vec![top],
            page,
            transitions: None,
            transition_duration: None,
        })
    }

    /// Wrap each page in its entrance and exit transition — the
    /// [`PageRoute::transitions`] of every page on this router's stack. A
    /// parent rebuild with new transitions reaches the pages already there.
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

    /// A handle to the nearest ancestor `Router<R>`.
    ///
    /// Lifecycle-only (ADR-0078): acquire it in `init_state` or
    /// `did_change_dependencies` and keep it; callbacks use the handle their
    /// state captured. A `build` context does not compile:
    ///
    /// ```compile_fail,E0308
    /// # use flui_widgets::prelude::*; use flui_widgets::{Router, Routable};
    /// fn acquire_handle<R: Routable>(cx: &dyn BuildContext) { let _ = Router::<R>::handle(cx); }
    /// ```
    ///
    /// while the same call from a lifecycle context does:
    ///
    /// ```
    /// # use flui_widgets::prelude::*; use flui_widgets::{Router, Routable};
    /// fn acquire_handle<R: Routable>(cx: &dyn LifecycleContext) { let _ = Router::<R>::handle(cx); }
    /// ```
    ///
    /// # Errors
    ///
    /// [`RouterError::NoRouter`] when no `Router<R>` is above `cx`.
    pub fn handle(cx: &dyn LifecycleContext) -> Result<RouterHandle<R>, RouterError> {
        cx.find_state::<RouterState<R>, _>(|state| RouterHandle::new(Rc::clone(&state.shared)))
            .ok_or(RouterError::NoRouter {
                route_type: type_name::<R>(),
            })
    }
}

impl<R: Routable> View for Router<R> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl<R: Routable> StatefulView for Router<R> {
    type State = RouterState<R>;

    fn create_state(&self) -> Self::State {
        let shared = Rc::new(RouterShared {
            navigator: Terminal::new(NavigatorHandle::addressed(type_name::<R>())),
            stack: RefCell::new(Vec::new()),
            page: Terminal::new(Rc::new(RefCell::new(Rc::clone(&self.page)))),
            transitions: Terminal::new(Rc::new(RefCell::new(self.transitions.clone()))),
            transition_duration: Cell::new(self.transition_duration),
            mounted: Cell::new(false),
        });
        let observer: Arc<dyn NavigatorObserver> = Arc::new(RouterObserver {
            shared: Rc::downgrade(&shared),
        });
        RouterState {
            shared: Terminal::new(shared),
            observer: Terminal::new(observer),
            initial: clone_routes(&self.initial),
        }
    }
}

/// Persistent state for [`Router`]. Opaque: it is public only because
/// [`StatefulView::State`] names it.
pub struct RouterState<R: Routable> {
    shared: Terminal<Rc<RouterShared<R>>>,
    observer: Terminal<Arc<dyn NavigatorObserver>>,
    /// Seeded in `init_state`, which receives no view.
    initial: Vec<R>,
}

impl<R: Routable> fmt::Debug for RouterState<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouterState")
            .field("location", &self.shared.location().map(|p| p.to_string()))
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
    /// The taken values stay in terminal slots — the unseeded tail and the
    /// value being seeded — until each one's page commits, so a failure while
    /// seeding (a route-identity refusal, a panicking page builder) retains
    /// them rather than dropping user values during unwind (ADR-0127). Pages
    /// seeded before the failure stay committed.
    fn seed_initial_pages(&mut self) {
        let mut remaining = Terminal::new(std::mem::take(&mut self.initial).into_iter());
        for route in &mut *remaining {
            let mut route = Terminal::new(route);
            let page = self.shared.page_route(&route);
            let id = self.shared.navigator.seed_page(page);
            self.shared.stack.borrow_mut().push(RouterEntry {
                id,
                route: route.take_value(),
            });
        }
    }
}

impl<R: Routable> ViewState<Router<R>> for RouterState<R> {
    /// Observe the navigator before it mounts, so every pop it makes — from
    /// this router's handle, the facade, a back gesture or a barrier — reaches
    /// the stack; then seed the initial pages, which the navigator flushes
    /// once when it mounts.
    fn init_state(&mut self, _cx: &dyn LifecycleContext) {
        self.shared.mounted.set(true);
        self.shared
            .navigator
            .add_observer(Arc::clone(&self.observer));
        self.seed_initial_pages();
    }

    fn build(&self, _view: &Router<R>, _cx: &dyn BuildContext) -> impl IntoView {
        Navigator::new(self.shared.navigator.clone())
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
        let ids: Vec<RouteId> = self.shared.stack.borrow().iter().map(|e| e.id).collect();
        for id in ids {
            self.shared.navigator.mark_route_needs_build(id);
        }
    }

    fn dispose(&mut self) {
        self.shared.mounted.set(false);
        self.shared.navigator.remove_observer(&self.observer);
    }
}

/// One page on the router's stack.
pub(super) struct RouterEntry<R> {
    pub(super) id: RouteId,
    pub(super) route: R,
}

/// Outgoing route values are retired without a stack borrow. An earlier
/// observer failure owns unwind: retain these opaque values rather than
/// invoking a competing destructor. On a healthy path, retire individually
/// and retain the unprocessed tail if one destructor fails. A route value
/// whose own fields double-panic before the catch remains outside containment.
pub(super) struct RouterRetiredValues<T>(pub(super) Vec<T>);

impl<T> Drop for RouterRetiredValues<T> {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.0);
        if std::thread::panicking() {
            std::mem::forget(entries);
            return;
        }
        let mut entries = entries.into_iter();
        while let Some(entry) = entries.next() {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(entry))) {
                std::mem::forget(entries);
                resume_unwind(payload);
            }
        }
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
}

impl<R: Routable> Drop for RouterShared<R> {
    fn drop(&mut self) {
        self.mounted.set(false);
        let navigator = self.navigator.withdraw();
        let stack = RetiredValues(std::mem::take(self.stack.get_mut()));
        let page = self.page.withdraw();
        let transitions = self.transitions.withdraw();
        drop((navigator, stack, page, transitions));
    }
}

impl<R: Routable> RouterShared<R> {
    /// The page route for `route`: a [`PageRoute`] named with its path, whose
    /// page scopes a semantics route (and names it, when `route` has a label).
    pub(super) fn page_route(&self, route: &R) -> PageRoute<()> {
        let builder = Terminal::new(Rc::clone(&self.page));
        let value = Terminal::new(route.clone());
        let label = route.semantics_label();
        let mut page = PageRoute::<()>::new(move |cx, _animation, _secondary| {
            let page = builder.borrow().clone();
            let mut semantics = Semantics::new()
                .scopes_route(true)
                .explicit_child_nodes(true);
            if let Some(label) = &label {
                semantics = semantics.names_route(true).label(label.clone());
            }
            semantics.child(page(&value, cx)).boxed()
        })
        .named(route.to_path().as_str());
        // Read at each build, so new transitions reach this page; with none,
        // the page's own default, a jump cut.
        let transitions = Terminal::new(Rc::clone(&self.transitions));
        page = page.transitions(move |cx, animation, secondary, child| {
            let current = transitions.borrow().clone();
            match current {
                Some(transitions) => transitions(cx, animation, secondary, child),
                None => child,
            }
        });
        if let Some(duration) = self.transition_duration.get() {
            page = page.transition_duration(duration);
        }
        page
    }

    /// The top route's path; `None` only before the first page is seeded.
    pub(super) fn location(&self) -> Option<RoutePath> {
        self.stack
            .borrow()
            .last()
            .map(|entry| entry.route.to_path())
    }

    /// Drop the page `id` names, if it is one of this router's.
    fn forget(&self, id: RouteId) {
        let retired = {
            let mut stack = self.stack.borrow_mut();
            stack
                .iter()
                .position(|entry| entry.id == id)
                .map(|index| stack.remove(index))
        };
        drop(RouterRetiredValues(retired.into_iter().collect()));
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::navigator::RouteId;
    use crate::support::child_process::child_test;

    /// A route value whose destructor panics once armed, unless it is the
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
                panic!("competing initial route destruction");
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
        /// pages already seeded, and retains the value being seeded and the
        /// unseeded tail instead of dropping them during unwind.
        fn seeding_refusal_retains_the_initial_tail() {
            let armed = Arc::new(AtomicBool::new(false));
            let drops = Arc::new(AtomicUsize::new(0));
            let value = |index| InitialValue {
                index,
                armed: Arc::clone(&armed),
                drops: Arc::clone(&drops),
            };
            let mut router = Router::new(value(0), |_, _| crate::Text::new("page").boxed());
            router.initial.extend([value(1), value(2)]);
            let mut state = router.create_state();
            drop(router);
            RouteId::leave_process_identities(1);
            armed.store(true, Ordering::SeqCst);
            let failure = catch_unwind(AssertUnwindSafe(|| state.seed_initial_pages()))
                .expect_err("the second seed is refused");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("BUG: route identity capacity exhausted"),
                "the capacity failure stays authoritative"
            );
            assert_eq!(drops.load(Ordering::SeqCst), 0, "no initial value dropped during unwind");
            armed.store(false, Ordering::SeqCst);
            assert_eq!(state.shared.stack.borrow().len(), 1, "the seeded page stays committed");
            assert_eq!(state.shared.navigator.route_ids().len(), 1);
            drop(state);
        }
    }
}

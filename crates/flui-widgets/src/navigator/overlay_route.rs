//! [`NavigatorRoute`] and [`SimpleRoute`] — the bridge from the pure route stack
//! to the overlay.
//!
//! Private; nothing here is exported.
//!
//! # Where the overlay entry lives
//!
//! A route does **not** own its overlay entries: the route lives behind
//! `Box<dyn ErasedRoute>` inside `RouteHistory`, and exposing overlay entries there
//! would break the route stack's pure-data invariant, which
//! `route_stack_flush_is_pure_data` enforces.
//!
//! So the `NavigatorState` keeps the entries, in a `RouteId -> OverlayEntry` map
//! it maintains alongside the stack. The route only supplies the *builder*. The
//! entries are flattened in history order, which is exactly what the map lookup
//! over `RouteHistory::ids()` produces.
//!
//! One consequence, recorded rather than hidden: the route is disposed inside the
//! flush and its overlay entry is removed just after. Nothing observes the
//! order, because a route holds no reference to its overlay entry.
//!
//! # `SimpleRoute` is the floor, not the ceiling
//!
//! [`SimpleRoute`] pushes and pops instantly: no animation, no barrier, no
//! `offstage`. `PageRoute` and `PopupRoute` add on top of the
//! private `TransitionRoute` / `ModalRoute`; they reach their navigator through
//! [`NavigatorRoute::binding_slot`].

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use flui_animation::Animation;
use flui_view::{BoxedView, BuildContext};

use super::binding::RouteBindingSlot;
use super::route::{Route, RouteSettings};

/// Builds a route's subtree, on demand and possibly many times.
///
/// Structurally identical to the private `overlay::OverlayBuilder`; named here so
/// that the public [`NavigatorRoute`] surface does not mention `Overlay`, which
/// stays private until its surface is settled.
pub type RouteContentBuilder = Rc<dyn Fn(&dyn BuildContext) -> BoxedView>;

/// One of a route's two animations, as seen by a page or transitions builder.
///
/// The **primary** animation runs 0 → 1 as the route enters and 1 → 0 as it
/// leaves. The **secondary** animation is the primary animation of the route
/// *above* this one, when the two coordinate.
pub type RouteAnimation = Arc<dyn Animation<f64>>;

/// Builds a route's page.
pub type RoutePageBuilder =
    Rc<dyn Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation) -> BoxedView>;

/// Wraps a route's page in its entrance/exit transition. The default is a jump
/// cut — `child` unchanged.
pub type RouteTransitionsBuilder =
    Rc<dyn Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation, BoxedView) -> BoxedView>;

/// A route that can be shown in the navigator's overlay.
///
/// The split from [`Route`] is the `OverlayRoute` layer: [`Route`] is lifecycle
/// and result, this adds "what to show".
pub trait NavigatorRoute: Route {
    /// Builds this route's subtree.
    ///
    /// Returned as a shared closure, not `&self`, because the navigator installs
    /// it into an `OverlayEntry` that outlives any borrow of the route.
    fn content_builder(&self) -> RouteContentBuilder;

    /// The cell this route wants its navigator capability delivered into, filled
    /// by `NavigatorHandle::push` / `seed_initial` **before** [`Route::install`].
    ///
    /// A route that neither animates nor drives its own lifecycle returns `None`
    /// — the default, and what [`SimpleRoute`] does. `PageRoute` and `PopupRoute`
    /// return `Some`, because a transition finishing has to tell the navigator to
    /// settle the route, and a pop's exit animation has to finalize it.
    ///
    /// The slot is opaque: nothing public can read the binding back out of it.
    fn binding_slot(&self) -> Option<&RouteBindingSlot> {
        None
    }
}

/// The floor: a route with content, an instant transition, and a typed result.
///
/// There is no transition, so a pop finalizes synchronously.
pub struct SimpleRoute<T> {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    /// The `??` fallback for a pop that carries no value.
    current_result: Option<T>,
    /// Set by tests / `LocalHistoryRoute`-shaped routes.
    handles_pop_internally: bool,
    /// When `false`, `did_pop` refuses, as a route with local history entries
    /// does while any remain.
    consents_to_pop: bool,
}

impl<T> fmt::Debug for SimpleRoute<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimpleRoute")
            .field("name", &self.settings.name())
            .finish_non_exhaustive()
    }
}

impl<T> SimpleRoute<T> {
    /// A route showing whatever `builder` builds.
    pub fn new(builder: impl Fn(&dyn BuildContext) -> BoxedView + 'static) -> Self {
        Self {
            settings: RouteSettings::default(),
            builder: Rc::new(builder),
            current_result: None,
            handles_pop_internally: false,
            consents_to_pop: true,
        }
    }

    /// Give the route a name.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.settings = RouteSettings::named(name);
        self
    }

    /// The `result ?? currentResult` fallback: what a `pop` with no value delivers.
    #[must_use]
    pub fn with_current_result(mut self, result: T) -> Self {
        self.current_result = Some(result);
        self
    }

    /// Make `did_pop` refuse, modelling `LocalHistoryRoute`.
    ///
    /// Test-only: a real refusing route implements [`Route::did_pop`] itself, and
    /// `LocalHistoryRoute` is deferred. Not part of the signed-off public surface;
    /// reached through `crate::__test_access::SimpleRouteProbe`.
    pub(crate) fn refusing_pop(mut self) -> Self {
        self.consents_to_pop = false;
        self
    }

    /// Make `will_handle_pop_internally` true, modelling `LocalHistoryRoute`.
    ///
    /// Test-only, for the same reason as [`SimpleRoute::refusing_pop`].
    pub(crate) fn handling_pop_internally(mut self) -> Self {
        self.handles_pop_internally = true;
        self
    }
}

impl<T: Send + Clone + 'static> Route for SimpleRoute<T> {
    type Output = T;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<T> {
        self.current_result.clone()
    }

    fn will_handle_pop_internally(&self) -> bool {
        self.handles_pop_internally
    }

    fn did_pop(&mut self) -> bool {
        self.consents_to_pop
    }
}

impl<T: Send + Clone + 'static> NavigatorRoute for SimpleRoute<T> {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }
}

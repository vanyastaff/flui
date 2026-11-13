//! The route stack. **Private, and pure data.**
//!
//! There is no `Navigator` here, and no widget. This module is the layer beneath
//! one: a `Vec<RouteEntry>`, the lifecycle state machine, the flush algorithm,
//! the observer queues, and the pop-result channel. The central
//! observation is that all of it is a pure function over route entries —
//! the flush never touches the element tree, and its only tree-visible effect
//! is the overlay rearrange at the very end, which the `Navigator` view
//! performs. So this layer is testable with no element tree, no build owner, no
//! render pipeline, and no overlay.
//!
//! The `Navigator` view, `NavigatorState` and the owned
//! `NavigatorHandle` sit on top: the `navigator` and `overlay_route` modules are the
//! only files here that may touch the widget tree or the overlay.
//!
//! The pure route-stack internals stay private, and the `Box<dyn Any>`
//! pop-result boundary remains an implementation detail behind typed public
//! methods (`pop_with`, `remove_route_with`, `maybe_pop_with`).
//!
//! A page an application navigates to is a value of its `Router`'s route type
//! (ADR-0330): there are no named routes, and the stack a location opens with
//! is `Routable::back_stack`. Restoration is the Router's (ADR-0262 §6);
//! `LocalHistoryRoute` stays `pub(crate)` (the doc-hidden, temporary
//! `__test_access` names its handles for tests, ADR-0083 §4), and there is no
//! per-route focus scope.

// `pub(crate)` only so `crate::__test_access` can re-export the items the
// integration tests still reach (ADR-0083 §4); nothing here is public.
pub(crate) mod back_gesture;
pub(crate) mod binding;
pub(crate) mod flight_seat;
pub(crate) mod hero;
pub(crate) mod hero_controller;
mod hero_controller_scope;
pub(crate) mod hero_flight;
mod hero_tags;
mod history;
pub(crate) mod lifecycle;
pub(crate) mod local_history;
pub(crate) mod modal_route;
#[expect(clippy::module_inception)]
pub(crate) mod navigator;
pub(crate) mod observer;
pub(crate) mod overlay_route;
mod page_handle;
mod page_route;
mod page_transitions;
mod parallax_transition;
mod pop_scope;
mod result;
mod route;
mod route_keys;
pub(crate) mod subtree;
pub(crate) mod transition_route;

pub use binding::RouteBindingSlot;
// `PushCompletion::Animating` carries one of these (ADR-0064): a third-party
// `Route` implementation needs to name the type without taking a direct
// `flui-scheduler` dependency of its own. `RunCompleter`/`RunDelivery`
// are deliberately not re-exported — nothing in this crate's public contract
// takes or returns them.
pub use flui_animation::{AnimationRunFuture, RunCanceled};
pub use hero::{Hero, HeroMode};
pub use hero_controller::{FlightDirection, HeroController};
pub use hero_controller_scope::HeroControllerScope;
pub(crate) use navigator::{Navigator, NavigatorHandle};
pub(crate) use observer::NavigatorObserver;
pub use overlay_route::{
    RouteAnimation, RouteContentBuilder, RoutePageBuilder, RouteTransitionsBuilder,
};
pub use page_handle::{PageError, PageHandle};
pub use page_route::{PageRoute, PopupRoute};
pub use page_transitions::{PageTransitionBuilder, PageTransitions};
pub use parallax_transition::ParallaxPageTransition;
pub use pop_scope::{PopInvokedCallback, PopScope};
pub use result::{RouteResult, RouteResultError};
pub use route::{PushCompletion, Route, RouteId, RouteSettings};

// The harness-driven navigator suites live in `crates/flui-widgets/tests/`
// (ADR-0083 §4); these are the unit tests that need no mounted tree.
#[cfg(test)]
mod tests;

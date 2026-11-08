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
//! The signed-off baseline surface is exported from the crate root and prelude.
//! The pure route-stack internals stay private, and the `Box<dyn Any>`
//! pop-result boundary remains an implementation detail behind typed public
//! methods (`pop_with`, `remove_route_with`, `maybe_pop_with`).
//!
//! # Typed routing and presentation labels
//!
//! The typed `Router` owns page placement and back-stack synthesis
//! (`Routable::back_stack`, ADR-0093). Named routes are not implemented: there
//! is no name-based registry, route generation, or named navigation API.
//!
//! [`RouteSettings::name`] remains a presentation label. It does not select a
//! route or affect the Router's stack.

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
mod navigator;
mod observer;
mod overlay_route;
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
pub use hero::{Hero, HeroMode};
pub use hero_controller::{FlightDirection, HeroController};
pub use hero_controller_scope::HeroControllerScope;
pub use navigator::{Navigator, NavigatorHandle, NavigatorState};
pub use observer::NavigatorObserver;
pub use overlay_route::{
    NavigatorRoute, RouteAnimation, RouteContentBuilder, RoutePageBuilder, RouteTransitionsBuilder,
    SimpleRoute,
};
pub use page_route::{PageRoute, PopupRoute};
pub use page_transitions::{PageTransitionBuilder, PageTransitions};
pub use parallax_transition::ParallaxPageTransition;
pub use pop_scope::{PopInvokedCallback, PopScope};
pub use result::{RouteResult, RouteResultError};
pub use route::{PushCompletion, Route, RouteId, RouteSettings};

#[cfg(test)]
mod tests;

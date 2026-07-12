//! The route stack. **Private, and pure data.**
//!
//! There is no `Navigator` here, and no widget. This module is the layer beneath
//! one: a `Vec<RouteEntry>`, the lifecycle state machine, the flush algorithm,
//! the observer queues, and the pop-result channel. The central
//! observation is that all of it is a pure function over route entries —
//! `_flushHistoryUpdates` never touches Flutter's element tree, and its only
//! tree-visible effect is the `overlay.rearrange` at the very end, which the
//! `Navigator` view performs. So this layer is testable with no element tree, no
//! build owner, no render pipeline, and no overlay, and
//! `route_stack_flush_is_pure_data` enforces that mechanically rather than on
//! trust.
//!
//! The `Navigator` view, `NavigatorState` and the owned
//! `NavigatorHandle` sit on top: the `navigator` and `overlay_route` modules are the
//! only files here that may touch the widget tree or the overlay.
//!
//! The signed-off baseline surface is exported from the crate root and prelude.
//! The pure route-stack internals stay private, and the `Box<dyn Any + Send>`
//! pop-result boundary remains an implementation detail behind typed public
//! methods (`pop_with`, `remove_route_with`, `maybe_pop_with`).
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/navigator.dart`, Flutter master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`.
//!
//! # Not implemented, and not claimed
//!
//! The public baseline now includes `Navigator`, `PageRoute` / `PopupRoute`,
//! `Hero` / `HeroController` / `HeroControllerScope` / `HeroMode`, and the Hero
//! customization hooks. Still deferred: Navigator 2.0,
//! restoration, named-route generation, `PopScope`, `LocalHistoryRoute`, per-route
//! focus scope, and nested-navigator hero flights.

mod binding;
mod hero;
mod hero_controller;
mod hero_controller_scope;
mod hero_flight;
mod history;
mod lifecycle;
mod local_history;
mod modal_route;
#[allow(clippy::module_inception)]
mod navigator;
mod observer;
mod overlay_route;
mod page_route;
mod pop_scope;
mod result;
mod route;
mod subtree;
mod transition_route;

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
pub use pop_scope::{PopInvokedCallback, PopScope};
pub use result::RouteResult;
pub use route::{PushCompletion, Route, RouteId, RouteSettings};
pub(crate) use subtree::AnchoredBox;

#[cfg(test)]
mod export_guard;
#[cfg(test)]
mod hero_controller_tests;
#[cfg(test)]
mod hero_flight_tests;
#[cfg(test)]
mod hero_seam_tests;
#[cfg(test)]
mod hero_tests;
#[cfg(test)]
mod modal_route_tests;
#[cfg(test)]
mod navigator_tests;
#[cfg(test)]
mod offstage_measurement_tests;
#[cfg(test)]
mod offstage_proxy_tests;
#[cfg(test)]
mod page_route_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transition_route_tests;

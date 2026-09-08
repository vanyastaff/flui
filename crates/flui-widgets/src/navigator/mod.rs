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
//! `Hero` / `HeroController` / `HeroControllerScope` / `HeroMode`, the Hero
//! customization hooks, cross-navigator hero flights, gesture-driven
//! (`transitionOnUserGestures`) flights, and named-route generation
//! (`GeneratedRoute`, `NavigatorHandle::route` / `on_generate_route` /
//! `on_unknown_route`, six untyped `*_named` entry points answering
//! `Result<RouteId, NamedRouteError>`, one typed `push_named_typed::<T>`, and
//! the typed-key path — `RouteKey<T>` / `route_keyed` / `push_keyed`, where the
//! route's result type is checked against its name by the compiler at the
//! registration site. Factories receive a `RouteRequest` carrying only the
//! request's settings, name and arguments — a factory is a builder, and a
//! redirect is expressed by returning a different route rather than by
//! navigating. `RouteRequest::navigator()` existed briefly and was withdrawn
//! (ADR-0024 §7.10); navigation from a factory now requires an explicitly
//! captured handle, is **survivable rather than supported**, and carries the
//! consequences `ARCHITECTURE.md` §5 records.
//! Still deferred: Navigator 2.0, restoration, `LocalHistoryRoute` (its module
//! is entirely `pub(crate)` — crate-private until the first consumer), and
//! per-route focus scope. `PopScope` is **not** deferred: it shipped
//! 2026-07-10 and is exported below.
//!
//! **Typed siblings of the other five entry points are deliberately not
//! offered.** `push_named_typed` exists because a caller who wants a pushed
//! screen's result has no other way to name its type; a
//! `push_replacement_named_typed` or `push_named_and_remove_until_typed` has no
//! consumer yet, and each is purely additive later. Going the other way — making
//! all six typed — is what the original design did: under *that* surface every
//! entry point took a `T`, so reaching a `PageRoute<i32>` screen from a caller
//! who wrote `::<()>` was a refusal to navigate rather than a push, for a caller
//! with no reason to know the route's result type at all. (No entry point takes
//! a turbofish today; the shape is described here only because it is the
//! rejected one.)
//!
//! Deferred **by decision**, inside the feature that just landed: Flutter's
//! `Navigator.initialRoute` / `Navigator.defaultRouteName` /
//! `Navigator.defaultGenerateInitialRoutes` — the initial-route back-stack
//! synthesis. It is ADR-0024 U3, whose §7.1 gate reaffirmed the deferral.
//!
//! The reason U3 originally gave for itself — "no consumer until deep links
//! exist" — is **false**, and is corrected in ADR-0024 §7.6. Read
//! `Navigator.defaultGenerateInitialRoutes`: **any** initial name other than
//! `/` takes the expansion branch, and that branch seeds `/` *first*. So
//! `initialRoute: "/settings"` yields `["/", "/settings"]` — a two-deep stack
//! whose back button returns home, not a one-deep stack that exits the app.
//! The consumer is `MaterialApp(initialRoute:)`, not deep linking.
//!
//! What the gap owes when it is built: seed `/` first, build the prefix chain
//! segment by segment, drop the segments the registry does not resolve, and
//! treat an unmatched *final* segment as an error that disposes every route
//! generated so far and seeds `/` alone. Its upstream oracles are
//! `'Initial route can have gaps'` and `'The full initial route has to be
//! matched'`. Until then FLUI bootstraps through
//! `NavigatorHandle::seed_initial`, one call per route.
//!
//! `restorablePushNamed` (restoration is unbuilt) and `replaceNamed` (`replace`
//! itself is private) are absent for their own reasons.

mod back_gesture;
mod binding;
mod hero;
mod hero_controller;
mod hero_controller_scope;
mod hero_flight;
mod history;
mod lifecycle;
mod local_history;
mod modal_route;
mod named_route;
#[expect(clippy::module_inception)]
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
pub use named_route::{GeneratedRoute, KeyedSettings, NamedRouteError, RouteKey, RouteRequest};
pub use navigator::{
    Navigator, NavigatorCommand, NavigatorCommandError, NavigatorCommandOutcome,
    NavigatorCommandTarget, NavigatorHandle, NavigatorState,
};
pub use observer::NavigatorObserver;
pub use overlay_route::{
    NavigatorRoute, RouteAnimation, RouteContentBuilder, RoutePageBuilder, RouteTransitionsBuilder,
    SimpleRoute,
};
pub use page_route::{PageRoute, PopupRoute};
pub use pop_scope::{PopInvokedCallback, PopScope};
pub use result::RouteResult;
pub use route::{PushCompletion, Route, RouteArguments, RouteId, RouteSettings};
pub(crate) use subtree::AnchoredBox;

#[cfg(test)]
mod export_guard;
#[cfg(test)]
mod hero_controller_tests;
#[cfg(test)]
mod hero_flight_tests;
#[cfg(test)]
mod hero_gesture_tests;
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

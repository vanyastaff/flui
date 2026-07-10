//! The route stack — ADR-0019 U2. **Private, and pure data.**
//!
//! There is no `Navigator` here, and no widget. This module is the layer beneath
//! one: a `Vec<RouteEntry>`, the lifecycle state machine, the flush algorithm,
//! the observer queues, and the pop-result channel. ADR-0019's central
//! observation is that all of it is a pure function over route entries —
//! `_flushHistoryUpdates` never touches Flutter's element tree, and its only
//! tree-visible effect is the `overlay.rearrange` at the very end, which U3 will
//! perform. So this layer is testable with no element tree, no build owner, no
//! render pipeline, and no overlay, and `route_stack_flush_is_pure_data`
//! enforces that mechanically rather than on trust.
//!
//! U3 added the `Navigator` view, `NavigatorState` and the owned
//! `NavigatorHandle` on top: the `navigator` and `overlay_route` modules are the
//! only files here that may touch the widget tree or the overlay.
//!
//! U4 exported the signed-off baseline surface from the crate root and prelude.
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
//! No animation (`TransitionRoute`), no barrier or focus (`ModalRoute`), no
//! `Hero`, no page-based routing, no restoration, no named-route generation, no
//! `PopScope`, no `LocalHistoryRoute`. ADR-0019 §5–§6 owns the sequence and the
//! deferrals. `can_pop` / `maybe_pop` landed in U3.

mod binding;
mod history;
mod lifecycle;
mod modal_route;
#[allow(clippy::module_inception)]
mod navigator;
mod observer;
mod overlay_route;
mod page_route;
mod result;
mod route;
mod transition_route;

pub use binding::RouteBindingSlot;
pub use navigator::{Navigator, NavigatorHandle, NavigatorState};
pub use observer::NavigatorObserver;
pub use overlay_route::{
    NavigatorRoute, RouteAnimation, RouteContentBuilder, RoutePageBuilder, RouteTransitionsBuilder,
    SimpleRoute,
};
pub use page_route::{PageRoute, PopupRoute};
pub use result::RouteResult;
pub use route::{PushCompletion, Route, RouteId, RouteSettings};

#[cfg(test)]
mod export_guard;
#[cfg(test)]
mod modal_route_tests;
#[cfg(test)]
mod navigator_tests;
#[cfg(test)]
mod offstage_measurement_tests;
#[cfg(test)]
mod page_route_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transition_route_tests;

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
//! U3 added the private `Navigator` view, `NavigatorState` and the owned
//! `NavigatorHandle` on top: [`navigator`] and [`overlay_route`] are the only
//! files here that may touch the widget tree or the overlay.
//!
//! Nothing here is exported from the crate root or the prelude. U4 owns the parity
//! and sign-off gate that decides what — if anything — becomes public. In
//! particular, the `Box<dyn Any + Send>` pop-result boundary is **not** authorized
//! merely by existing here.
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

// U4's public export is what finally gives this module a non-test consumer.
// Until then `Navigator` is reachable only from its own tests, so **everything
// here is dead code from rustc's reachability view** — including the parts U3
// genuinely wired together. The `unused_imports` allows U2 needed are gone (the
// re-export block they covered was itself unused, and has been deleted); this one
// must survive until U4, and must go with it.
#![allow(dead_code)]

mod history;
mod lifecycle;
#[allow(clippy::module_inception)]
mod navigator;
mod observer;
mod overlay_route;
mod result;
mod route;

#[cfg(test)]
mod navigator_tests;
#[cfg(test)]
mod tests;

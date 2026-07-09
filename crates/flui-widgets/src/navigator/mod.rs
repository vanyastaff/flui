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
//! Nothing here is exported from the crate root or the prelude. U3 adds the
//! private `Navigator` view; U4 owns the parity + sign-off gate that decides what
//! — if anything — becomes public. In particular the `Box<dyn Any + Send>`
//! pop-result boundary in [`route`] is **not** authorized by its existence here.
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
//! `canPop` / `maybePop`, no `LocalHistoryRoute`. ADR-0019 §5–§6 owns the
//! sequence and the deferrals.

// U3's `Navigator` is the intended consumer; until it lands, nothing in
// `flui-widgets` calls this module, and nothing outside can. Same posture as the
// `overlay` module (U1) and ADR-0018's `FutureBuilder` between U4 and U6 — and,
// like them, this attribute must be **deleted** when the consumer arrives.
#![allow(dead_code)]

mod history;
mod lifecycle;
mod observer;
mod result;
mod route;

#[cfg(test)]
mod tests;

// The surface U3's `Navigator` will consume. `#[allow(unused_imports)]` rather
// than deletion, so the intended shape is visible now and `unused_imports` does
// not force U3 to reconstruct it — the `#![allow(dead_code)]` above does not
// cover re-exports. Both attributes go when U3 lands.
#[allow(unused_imports)]
pub(crate) use history::{FlushOutcome, RouteHistory};
#[allow(unused_imports)]
pub(crate) use lifecycle::RouteLifecycle;
#[allow(unused_imports)]
pub(crate) use observer::{NavigatorObserver, Observation};
#[allow(unused_imports)]
pub(crate) use result::RouteResult;
#[allow(unused_imports)]
pub(crate) use route::{AnyResult, PushCompletion, Route, RouteId, RouteSettings};

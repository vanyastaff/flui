//! The typed [`Router`]: navigation state as a stack of route values whose
//! top is the current location (ADR-0093).
//!
//! An application declares its locations as a [`Routable`] type — a value per
//! screen, printed to and parsed from a [`RoutePath`] — and builds a
//! `Router<R>` that places one page per value on a [`Navigator`]. A
//! descendant acquires a [`RouterHandle`] in `init_state` and drives the stack
//! with values (`push`, `replace`, `pop`) or with a location (`go`), and reads
//! the location back.
//!
//! The navigator stays the page stack underneath (ADR-0093 §4): transitions,
//! heroes, `PopScope` and local history are properties of the pages the
//! router places. Its facade refuses pages that are not route values, keeps
//! admitting pageless popups such as dialogs, and every pop it makes reaches
//! the router's stack through an internal observer.
//!
//! This module sits above `navigator` and below `app` (`cargo xtask
//! module-dag`): it drives the navigator, and the app builds on it.
//!
//! [`Navigator`]: crate::Navigator

mod handle;
mod path;
mod routable;
#[expect(clippy::module_inception)]
mod router;

pub use handle::{RouterError, RouterHandle};
pub use path::{RouteParseError, RoutePath};
pub use routable::Routable;
pub use router::{Router, RouterState};

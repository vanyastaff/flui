//! Owner-local nested scrolling, independent from gesture-arena arbitration.

mod ballistic;
mod connection;
mod dispatcher;
mod geometry;
mod link;
mod origin;
mod run;
mod scope;
mod types;

pub use scope::{NestedScroll, NestedScrollState};
pub use types::{
    FlingHandoff, FlingStep, NestedScrollConnection, OverscrollBehavior, ScrollCause, ScrollDevice,
    ScrollEndReason, ScrollStep,
};

pub(crate) use ballistic::{BallisticMotion, ScrollContinuation};
pub(crate) use dispatcher::NestedScrollDispatcher;

//! The flui-semantics integration tests, one `semantics_it` binary: each file
//! is a module of it.

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
#[path = "actionability.rs"]
mod actionability;
#[path = "announce.rs"]
mod announce;
#[path = "assistive_focus.rs"]
mod assistive_focus;
#[path = "delta.rs"]
mod delta;
#[path = "fixture.rs"]
mod fixture;
#[path = "identifier.rs"]
mod identifier;
#[path = "incremental_flush.rs"]
mod incremental_flush;
#[path = "locate.rs"]
mod locate;
#[path = "reading_order.rs"]
mod reading_order;
#[path = "route_change.rs"]
mod route_change;
#[path = "text_runs.rs"]
mod text_runs;
#[path = "wait.rs"]
mod wait;

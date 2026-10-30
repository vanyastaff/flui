//! The flui-semantics integration tests, one `semantics_it` binary: each file
//! is a module of it.

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
#[path = "text_runs.rs"]
mod text_runs;
#[path = "wait.rs"]
mod wait;

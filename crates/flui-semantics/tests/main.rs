//! The flui-semantics integration tests, one `semantics_it` binary: each file
//! is a module of it.

#[path = "actionability.rs"]
mod actionability;
#[path = "announce.rs"]
mod announce;
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
#[path = "text_runs.rs"]
mod text_runs;

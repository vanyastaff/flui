//! What a perf scenario measures and what it is held to (ADR-0396).
//!
//! A perf scenario is a test that drives a tree through fixed steps and
//! records what they cost in counts: the work of each phase and what the
//! steps asked of the heap. [`Baselines`] holds those counts to a checked-in
//! file beside the scenario, so a change of cost is a line of the pull
//! request's diff.

mod baseline;

pub use baseline::{BLESS_COMMAND, Baselines, Held};

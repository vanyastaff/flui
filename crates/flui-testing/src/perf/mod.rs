//! What a perf scenario measures and what it is held to (ADR-0396).
//!
//! A perf scenario is a test that drives a tree through fixed steps and
//! records what they cost in counts: the work of each phase, what the steps
//! asked of the heap, and, under callgrind, the instructions they ran.
//! [`measure`] is the window those steps run in, and [`Baselines`] holds
//! their counts to a checked-in file beside the scenario, so a change of cost
//! is a line of the pull request's diff.

mod baseline;
mod instructions;
mod window;

pub use baseline::{BLESS_COMMAND, Baselines, Held};
pub use window::{Cost, measure};

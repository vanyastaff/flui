//! flui-animation's integration tests, through the public API only.
//!
//! One binary for the crate (`animation_it`): each suite is a module of this
//! file. `tick_allocation` is a target of its own because it installs a
//! counting `#[global_allocator]`.

#![allow(
    clippy::unwrap_used,
    reason = "scenario functions are rows of the contract tables, so clippy no longer sees them as test functions"
)]

#[path = "support/child_process.rs"]
mod child_process;

#[path = "contracts/builder.rs"]
mod builder;

#[path = "contracts/controller_sources.rs"]
mod controller_sources;

#[path = "contracts/controller_robustness.rs"]
mod controller_robustness;

#[path = "contracts/frame_path.rs"]
mod frame_path;

#[path = "contracts/ownership.rs"]
mod ownership;

#[path = "contracts/status_delivery.rs"]
mod status_delivery;

#[path = "contracts/curve.rs"]
mod curve;

#[path = "contracts/curved.rs"]
mod curved;

#[path = "contracts/curves.rs"]
mod curves;

#[path = "contracts/keyframes.rs"]
mod keyframes;

mod derive_animatable;

#[path = "contracts/proxy.rs"]
mod proxy;

#[path = "contracts/simulation.rs"]
mod simulation;

#[path = "contracts/tween.rs"]
mod tween;

/// Execute every named scenario before reporting failures. Keep opaque panic
/// payloads alive until all rows complete, without invoking arbitrary Drop.
pub(crate) fn run_table(cases: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            failures.push((name, payload));
        }
    }
    if !failures.is_empty() {
        let names: Vec<_> = failures.iter().map(|(name, _)| *name).collect();
        std::mem::forget(failures);
        panic!("failed cases: {names:?}");
    }
}

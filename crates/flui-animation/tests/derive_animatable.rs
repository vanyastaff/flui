//! Consumer integration tests for animation derives and simulations.
//!
//! These live in `tests/` (a separate crate that depends on flui-animation) so
//! the derive's `::flui_animation::TwoWayConverter` path resolves the same way
//! it does for a real downstream user.

// The derive copies fields verbatim and the asserted values are exactly
// representable in f64, so exact-equality round-trip assertions are correct.

use flui_animation::{Animatable, TwoWayConverter};

#[path = "support/child_process.rs"]
mod child_process;

#[path = "contracts/simulation.rs"]
mod simulation;

#[path = "contracts/proxy.rs"]
mod proxy;

#[path = "contracts/curve.rs"]
mod curve;

#[path = "contracts/tween.rs"]
mod tween;

#[derive(Clone, Animatable)]
struct Translation {
    x: f64,
    y: f64,
    z: f64,
}

#[test]
fn named_struct_round_trips_through_vector() {
    let t = Translation {
        x: 1.0,
        y: 2.0,
        z: 3.0,
    };
    assert_eq!(t.to_vector(), [1.0, 2.0, 3.0]);

    let back = Translation::from_vector([4.0, 5.0, 6.0]);
    assert_eq!((back.x, back.y, back.z), (4.0, 5.0, 6.0));
}

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
        panic!("failed numerical cases: {names:?}");
    }
}

#[path = "contracts/controller_sources.rs"]
mod controller_sources;

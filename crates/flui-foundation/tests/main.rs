//! Consumer contracts share one native integration test binary.

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
#[path = "notifier.rs"]
mod notifier;

#[path = "claim_slot.rs"]
mod claim_slot;

#[path = "generation.rs"]
mod generation;

#[path = "slot_index.rs"]
mod slot_index;

#[path = "sequence.rs"]
mod sequence;

#[path = "panic_recovery.rs"]
mod panic_recovery;

#[path = "commit.rs"]
mod commit;

#[path = "reclaim_memory.rs"]
mod reclaim_memory;

#[path = "geometry/transform.rs"]
mod transform;

#[path = "geometry/matrix.rs"]
mod matrix;

#[path = "geometry/scalar.rs"]
mod scalar;

#[path = "geometry/lerp.rs"]
mod lerp;

#[path = "geometry/matrix4_lerp.rs"]
mod matrix4_lerp;

#[path = "geometry/angle.rs"]
mod angle;

/// Execute every named scenario before reporting failures. A failed row's
/// payload is discarded under containment (ADR-0281).
pub(crate) fn run_table(cases: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            failures.push(name);
            flui_foundation::panic::discard_payload(payload);
        }
    }
    assert!(failures.is_empty(), "failed numerical cases: {failures:?}");
}

#[path = "inspect.rs"]
mod inspect;

#[path = "geometry/window_metrics.rs"]
mod window_metrics;

// The `testing` feature's wakers.
mod testing_kit;

// The owner thread's seams carry no thread bound (ADR-0323 §4).
mod owner_local;

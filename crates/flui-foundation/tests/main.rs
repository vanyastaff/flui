//! Consumer contracts share one native integration test binary.

#[path = "notifier.rs"]
mod notifier;

#[path = "claim_slot.rs"]
mod claim_slot;

#[path = "geometry/transform.rs"]
mod transform;

#[path = "geometry/matrix.rs"]
mod matrix;

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

//! Public numerical geometry at finite extremes.
use flui_rendering::constraints::BoxConstraints;

fn diagonal(width: f64, height: f64, expected: f64) {
    let measured = BoxConstraints::new(0.0, width, 0.0, height).max_diagonal();
    assert!(
        (measured / expected - 1.0).abs() < 2e-15,
        "diagonal {measured}, expected {expected}"
    );
}
// Scaled 3-4-5 triangles are an independent exact geometry oracle.
fn ordinary() {
    diagonal(3.0, 4.0, 5.0);
}
fn large() {
    diagonal(3e200, 4e200, 5e200);
}
fn tiny() {
    diagonal(3e-200, 4e-200, 5e-200);
}
fn unbounded() {
    assert_eq!(BoxConstraints::UNCONSTRAINED.max_diagonal(), f64::INFINITY);
}
fn zero() {
    assert_eq!(BoxConstraints::new(0.0, 0.0, 0.0, 0.0).max_diagonal(), 0.0);
}
#[test]
fn maximum_diagonal_retains_representable_extreme_lengths() {
    crate::run_table(&[
        ("ordinary", ordinary),
        ("large", large),
        ("tiny", tiny),
        ("unbounded", unbounded),
        ("zero", zero),
    ]);
}

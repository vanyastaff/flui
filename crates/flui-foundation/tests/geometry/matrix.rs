//! Finite matrix admission for geometry optimizations.
use flui_foundation::geometry::{Matrix4, Transform};

fn finite_boundary_is_inclusive() {
    let mut matrix = Matrix4::identity();
    matrix.m[12] = 0.125;
    assert!(matrix.is_identity_with_epsilon(0.125));
    assert!(matrix.approx_eq_eps(&Matrix4::identity(), 0.125));
    assert!(!matrix.is_identity_with_epsilon(0.0625));
    assert!(!matrix.approx_eq_eps(&Matrix4::identity(), 0.0625));
}
fn zero_tolerance_accepts_exact_identity() {
    let identity = Matrix4::identity();
    assert!(identity.is_identity_with_epsilon(0.0));
    assert!(identity.approx_eq_eps(&identity, 0.0));
}
fn negative_tolerance_is_refused() {
    refused_tolerance(-1.0);
}
fn nan_tolerance_is_refused() {
    refused_tolerance(f64::NAN);
}
fn infinite_tolerance_is_refused() {
    refused_tolerance(f64::INFINITY);
}
fn refused_tolerance(epsilon: f64) {
    let identity = Matrix4::identity();
    assert!(!identity.is_identity_with_epsilon(epsilon));
    assert!(!identity.approx_eq_eps(&identity, epsilon));
}
fn nan_matrix_is_not_discarded_as_identity() {
    let mut matrix = Matrix4::identity();
    matrix.m[12] = f64::NAN;
    assert!(!matrix.is_identity());
    assert!(!matrix.approx_eq(&Matrix4::identity()));
    assert!(!matrix.approx_eq(&matrix));
    let converted: Matrix4 = Transform::from(matrix).into();
    assert!(
        converted.m[12].is_nan(),
        "conversion discarded the admitted matrix"
    );
}
fn infinite_matrix_is_refused() {
    let mut matrix = Matrix4::identity();
    matrix.m[12] = f64::INFINITY;
    assert!(!matrix.is_identity());
    assert!(!matrix.approx_eq(&Matrix4::identity()));
    assert!(!matrix.approx_eq(&matrix));
}
#[test]
fn matrix_tolerance_requires_finite_values() {
    crate::run_table(&[
        ("inclusive finite boundary", finite_boundary_is_inclusive),
        (
            "exact zero tolerance",
            zero_tolerance_accepts_exact_identity,
        ),
        ("negative tolerance", negative_tolerance_is_refused),
        ("nan tolerance", nan_tolerance_is_refused),
        ("infinite tolerance", infinite_tolerance_is_refused),
        (
            "nan matrix preserved",
            nan_matrix_is_not_discarded_as_identity,
        ),
        ("infinite matrix refused", infinite_matrix_is_refused),
    ]);
}

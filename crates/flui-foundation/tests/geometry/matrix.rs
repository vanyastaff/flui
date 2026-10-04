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

fn assert_inverse_coordinates(matrix: Matrix4, transformed: (f64, f64)) {
    assert!(matrix.is_invertible());
    let inverse = matrix.try_inverse().expect("finite inverse is admitted");
    let point = inverse.transform_point(transformed.0, transformed.1);
    assert!((point.0 - 5.0).abs() < 1e-10 && (point.1 - 7.0).abs() < 1e-10);
    let mut in_place = matrix;
    assert!(in_place.invert());
    let point = in_place.transform_point(transformed.0, transformed.1);
    assert!((point.0 - 5.0).abs() < 1e-10 && (point.1 - 7.0).abs() < 1e-10);
}

fn tiny_scale_keeps_its_finite_inverse() {
    assert_inverse_coordinates(Matrix4::scaling(1e-9, 1e-9, 1.0), (5e-9, 7e-9));
}

fn ordinary_composed_inverse_keeps_coordinates() {
    assert_inverse_coordinates(
        Matrix4::translation(3.0, 5.0, 0.0) * Matrix4::scaling(2.0, 3.0, 1.0),
        (13.0, 26.0),
    );
}

fn assert_inverse_refused(matrix: Matrix4) {
    assert!(!matrix.is_invertible());
    assert!(matrix.try_inverse().is_none());
    let original = matrix.m.map(f64::to_bits);
    let mut in_place = matrix;
    assert!(!in_place.invert());
    assert_eq!(
        in_place.m.map(f64::to_bits),
        original,
        "refusal preserves input coordinates"
    );
}

fn singular_scale_refuses_inversion() {
    assert_inverse_refused(Matrix4::scaling(0.0, 1.0, 1.0));
}

fn nan_input_refuses_inversion() {
    assert_inverse_refused(Matrix4::translation(f64::NAN, 0.0, 0.0));
}

fn infinite_input_refuses_inversion() {
    assert_inverse_refused(Matrix4::scaling(f64::INFINITY, 1.0, 1.0));
}

fn negative_infinite_input_refuses_inversion() {
    assert_inverse_refused(Matrix4::scaling(f64::NEG_INFINITY, 1.0, 1.0));
}

fn overflowing_inverse_refuses_inversion() {
    assert_inverse_refused(Matrix4::scaling(f64::from_bits(1), 1.0, 1.0));
}

fn overflowing_computed_determinant_refuses_inversion() {
    let mut matrix = Matrix4::scaling(1e100, 1e100, 1e100);
    matrix.m[15] = 1e100;
    assert_inverse_refused(matrix);
}

fn underflowing_computed_determinant_refuses_inversion() {
    assert_inverse_refused(Matrix4::scaling(1e-200, 1e-200, 1.0));
}

#[test]
fn matrix_inverse_requires_a_finite_computed_result() {
    crate::run_table(&[
        (
            "tiny_scale_keeps_its_finite_inverse",
            tiny_scale_keeps_its_finite_inverse,
        ),
        (
            "ordinary_composed_inverse_keeps_coordinates",
            ordinary_composed_inverse_keeps_coordinates,
        ),
        (
            "singular_scale_refuses_inversion",
            singular_scale_refuses_inversion,
        ),
        ("nan_input_refuses_inversion", nan_input_refuses_inversion),
        (
            "infinite_input_refuses_inversion",
            infinite_input_refuses_inversion,
        ),
        (
            "negative_infinite_input_refuses_inversion",
            negative_infinite_input_refuses_inversion,
        ),
        (
            "overflowing_inverse_refuses_inversion",
            overflowing_inverse_refuses_inversion,
        ),
        (
            "overflowing_computed_determinant_refuses_inversion",
            overflowing_computed_determinant_refuses_inversion,
        ),
        (
            "underflowing_computed_determinant_refuses_inversion",
            underflowing_computed_determinant_refuses_inversion,
        ),
    ]);
}

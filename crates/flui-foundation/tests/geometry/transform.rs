//! Public transform decomposition at finite numerical extremes.

use flui_foundation::geometry::{Matrix4, Transform};

fn scales(x: f64, y: f64, expected_y: f64) {
    let (tx, ty, rotation, sx, sy) = Transform::scale_xy(x, y).decompose();
    assert_eq!((tx, ty, rotation), (0.0, 0.0, 0.0));
    assert_eq!(sx, x, "first scale");
    assert_eq!(sy, expected_y, "second scale");
}
fn ordinary() {
    scales(3.0, 4.0, 4.0);
}
fn reflection() {
    scales(3.0, -4.0, -4.0);
}
fn large() {
    scales(1e200, 2e200, 2e200);
}
fn large_reflection() {
    scales(1e200, -2e200, -2e200);
}
fn tiny_columns() {
    scales(1e-200, 2e-200, 2e-200);
}
fn degenerate_first_column() {
    scales(0.0, 1e200, 1e200);
}
fn anisotropic_shear(sign: f64) {
    let mut matrix = Matrix4::identity();
    matrix.m[0] = sign * 1e-200;
    matrix.m[1] = 1e150;
    matrix.m[4] = 0.0;
    matrix.m[5] = 1e150;
    let (_, _, _, sx, sy) = Transform::Matrix(matrix).decompose();
    assert_eq!(sx, 1e150);
    assert!(
        (sy / (sign * 1e-200) - 1.0).abs() < 1e-15,
        "signed scale {sy}"
    );
}
fn anisotropic_shear_preserves_small_scale() {
    anisotropic_shear(1.0);
}
fn anisotropic_shear_preserves_small_reflection() {
    anisotropic_shear(-1.0);
}
fn determinant_underflow_preserves_reflection() {
    let (_, _, _, sx, sy) = Transform::scale_xy(1e-10, -1e-320).decompose();
    assert_eq!(sx, 1e-10);
    assert_eq!(sy, -1e-320);
}
fn ordinary_rotation() {
    let (_, _, rotation, sx, sy) = Transform::rotate(std::f64::consts::FRAC_PI_4).decompose();
    assert!((rotation - std::f64::consts::FRAC_PI_4).abs() < 1e-15);
    assert!((sx - 1.0).abs() < 1e-15);
    assert!((sy - 1.0).abs() < 1e-15);
}
#[test]
fn affine_decomposition_retains_finite_extreme_scales() {
    crate::run_table(&[
        ("ordinary", ordinary),
        ("reflection", reflection),
        ("large", large),
        ("large reflection", large_reflection),
        ("tiny columns", tiny_columns),
        ("degenerate first column", degenerate_first_column),
        ("ordinary rotation", ordinary_rotation),
        (
            "anisotropic shear preserves small scale",
            anisotropic_shear_preserves_small_scale,
        ),
        (
            "anisotropic shear preserves small reflection",
            anisotropic_shear_preserves_small_reflection,
        ),
        (
            "determinant underflow preserves reflection",
            determinant_underflow_preserves_reflection,
        ),
    ]);
}

fn assert_point(matrix: Matrix4, input: (f64, f64), expected: (f64, f64)) {
    let actual = matrix.transform_point(input.0, input.1);
    assert!(
        (actual.0 - expected.0).abs() < 1e-12,
        "x: {actual:?} != {expected:?}"
    );
    assert!(
        (actual.1 - expected.1).abs() < 1e-12,
        "y: {actual:?} != {expected:?}"
    );
}
fn horizontal_shear() {
    assert_point(
        Transform::skew(0.2, 0.0).into(),
        (2.0, 3.0),
        (2.0 + 3.0 * 0.2_f64.tan(), 3.0),
    );
}
fn vertical_shear() {
    assert_point(
        Transform::skew(0.0, 0.3).into(),
        (2.0, 3.0),
        (2.0, 3.0 + 2.0 * 0.3_f64.tan()),
    );
}
fn two_axis_shear_inverse() {
    let forward = Transform::skew(0.2, 0.3);
    let inverse = forward.inverse().expect("finite shear is invertible");
    assert_point(forward.then(inverse).into(), (2.0, 3.0), (2.0, 3.0));
}
fn then_applies_translation_before_scale() {
    assert_point(
        Transform::translate(10.0, 5.0)
            .then(Transform::scale(2.0))
            .into(),
        (2.0, 3.0),
        (24.0, 16.0),
    );
}
fn nested_compositions_keep_application_order() {
    let first = Transform::translate(10.0, 5.0).then(Transform::scale(2.0));
    let second = Transform::translate(-4.0, 3.0).then(Transform::scale_xy(3.0, 4.0));
    assert_point(first.then(second).into(), (2.0, 3.0), (60.0, 76.0));
}
#[test]
fn affine_composition_and_shear_follow_coordinate_contract() {
    crate::run_table(&[
        ("horizontal shear", horizontal_shear),
        ("vertical shear", vertical_shear),
        ("two axis shear inverse", two_axis_shear_inverse),
        (
            "translation before scale",
            then_applies_translation_before_scale,
        ),
        (
            "nested application order",
            nested_compositions_keep_application_order,
        ),
    ]);
}

//! `Matrix4::lerp`: decomposition into translation, scale, skew, perspective and a
//! quaternion, interpolated part by part. Every expected value below is written
//! from the definition of the transform it names, not computed by the decomposition.

use flui_foundation::geometry::Matrix4;

fn assert_close(actual: Matrix4, expected: Matrix4, tolerance: f64, what: &str) {
    for (index, (got, want)) in actual.m.iter().zip(expected.m.iter()).enumerate() {
        assert!(
            (got - want).abs() <= tolerance,
            "{what}: element {index} is {got}, expected {want}\n actual: {actual:?}"
        );
    }
}

fn zero_scale_axis_interpolates_finitely() {
    let mid = Matrix4::scaling(0.0, 0.0, 1.0).lerp(Matrix4::IDENTITY, 0.5);
    assert!(mid.m.iter().all(|v| v.is_finite()), "non-finite: {mid:?}");
    assert_close(mid, Matrix4::scaling(0.5, 0.5, 1.0), 1e-12, "scale 0 -> 1");
}

fn zero_scale_end_borrows_the_other_ends_rotation() {
    let collapsed = Matrix4::scaling(0.0, 1.0, 1.0);
    let rotated = Matrix4::rotation_z(1.0);
    let expected = Matrix4::rotation_z(1.0) * Matrix4::scaling(0.5, 1.0, 1.0);
    assert_close(
        collapsed.lerp(rotated, 0.5),
        expected,
        1e-12,
        "collapsed -> rotated",
    );
    assert_close(
        rotated.lerp(collapsed, 0.5),
        expected,
        1e-12,
        "rotated -> collapsed",
    );
}

fn rotation_slerps_by_the_half_angle() {
    let mid = Matrix4::rotation_z(0.0).lerp(Matrix4::rotation_z(0.6), 0.5);
    assert_close(mid, Matrix4::rotation_z(0.3), 1e-12, "rotation 0 -> 0.6");
}

fn skew_is_kept_and_interpolated() {
    let angle: f64 = 0.5;
    let mid = Matrix4::skew_2d(angle, 0.0).lerp(Matrix4::IDENTITY, 0.5);
    // x' = x + k·y: the shear coefficient sits in row 0, column 1.
    let mut expected = Matrix4::IDENTITY;
    *expected.get_mut(0, 1) = angle.tan() / 2.0;
    assert_close(mid, expected, 1e-12, "skew tan(a) -> 0");
}

fn perspective_is_kept_and_interpolated() {
    let distance = 200.0;
    // CSS `perspective(d)`: w' = w − z/d, row 3, column 2.
    let mut perspective = Matrix4::IDENTITY;
    *perspective.get_mut(3, 2) = -1.0 / distance;
    let mid = perspective.lerp(Matrix4::IDENTITY, 0.5);
    let mut expected = Matrix4::IDENTITY;
    *expected.get_mut(3, 2) = -1.0 / (2.0 * distance);
    assert_close(mid, expected, 1e-15, "perspective -1/d -> 0");
}

fn translation_and_scale_interpolate_linearly() {
    let begin = Matrix4::translation(10.0, -4.0, 0.0) * Matrix4::scaling(2.0, 3.0, 1.0);
    let end = Matrix4::translation(30.0, 4.0, 0.0) * Matrix4::scaling(4.0, 1.0, 1.0);
    let expected = Matrix4::translation(20.0, 0.0, 0.0) * Matrix4::scaling(3.0, 2.0, 1.0);
    assert_close(begin.lerp(end, 0.5), expected, 1e-12, "translate + scale");
}

fn singular_projection_switches_at_the_midpoint() {
    let mut singular = Matrix4::IDENTITY;
    *singular.get_mut(3, 3) = 0.0;
    *singular.get_mut(3, 2) = 1.0;
    for (t, expected) in [
        (-0.5, singular),
        (0.25, singular),
        (0.4999, singular),
        (0.5, Matrix4::IDENTITY),
        (0.75, Matrix4::IDENTITY),
        (1.5, Matrix4::IDENTITY),
    ] {
        let got = singular.lerp(Matrix4::IDENTITY, t);
        assert_eq!(got.m, expected.m, "m33 = 0 at t = {t}");
    }
}

fn perspective_over_a_collapsed_axis_switches_at_the_midpoint() {
    // A perspective row cannot be solved for when the linear part is singular.
    let mut collapsed = Matrix4::scaling(0.0, 1.0, 1.0);
    *collapsed.get_mut(3, 0) = 0.01;
    assert_eq!(collapsed.lerp(Matrix4::IDENTITY, 0.25).m, collapsed.m);
    assert_eq!(
        collapsed.lerp(Matrix4::IDENTITY, 0.5).m,
        Matrix4::IDENTITY.m
    );
}

fn normalisation_overflow_switches_at_the_midpoint() {
    // Dividing by a subnormal m33 turns the finite translation into infinity.
    let mut tiny = Matrix4::translation(1.0, 0.0, 0.0);
    *tiny.get_mut(3, 3) = f64::from_bits(1);
    let quarter = tiny.lerp(Matrix4::IDENTITY, 0.25);
    assert_eq!(quarter.m, tiny.m, "before the midpoint: {quarter:?}");
    let half = tiny.lerp(Matrix4::IDENTITY, 0.5);
    assert_eq!(half.m, Matrix4::IDENTITY.m, "from the midpoint: {half:?}");
}

#[test]
fn matrix4_lerp_decomposes_like_css_transforms() {
    crate::run_table(&[
        (
            "zero scale axis interpolates finitely",
            zero_scale_axis_interpolates_finitely,
        ),
        (
            "zero scale end borrows the other end's rotation",
            zero_scale_end_borrows_the_other_ends_rotation,
        ),
        (
            "rotation slerps by the half angle",
            rotation_slerps_by_the_half_angle,
        ),
        (
            "skew is kept and interpolated",
            skew_is_kept_and_interpolated,
        ),
        (
            "perspective is kept and interpolated",
            perspective_is_kept_and_interpolated,
        ),
        (
            "translation and scale interpolate linearly",
            translation_and_scale_interpolate_linearly,
        ),
        (
            "singular projection switches at the midpoint",
            singular_projection_switches_at_the_midpoint,
        ),
        (
            "perspective over a collapsed axis switches at the midpoint",
            perspective_over_a_collapsed_axis_switches_at_the_midpoint,
        ),
        (
            "normalisation overflow switches at the midpoint",
            normalisation_overflow_switches_at_the_midpoint,
        ),
    ]);
}

#[cfg(not(target_arch = "wasm32"))]
mod properties {
    use flui_foundation::geometry::Matrix4;
    use proptest::prelude::*;

    /// A non-zero scale factor of either sign.
    fn scale_factor() -> impl Strategy<Value = f64> {
        (0.05f64..20.0, any::<bool>()).prop_map(|(s, negative)| if negative { -s } else { s })
    }

    /// Translate · rotate (about z, then x) · skew · scale, with no axis collapsed.
    fn srt_skew() -> impl Strategy<Value = Matrix4> {
        (
            (-1e3f64..1e3, -1e3f64..1e3, -50.0f64..50.0),
            (-7.0f64..7.0, -1.5f64..1.5),
            (-1.2f64..1.2, -1.2f64..1.2),
            (scale_factor(), scale_factor(), scale_factor()),
        )
            .prop_map(|((tx, ty, tz), (rz, rx), (kx, ky), (sx, sy, sz))| {
                Matrix4::translation(tx, ty, tz)
                    * Matrix4::rotation_z(rz)
                    * Matrix4::rotation_x(rx)
                    * Matrix4::skew_2d(kx, ky)
                    * Matrix4::scaling(sx, sy, sz)
            })
    }

    fn bits(matrix: Matrix4) -> [u64; 16] {
        matrix.m.map(f64::to_bits)
    }

    #[test]
    fn matrix4_lerp_endpoints_and_finiteness() {
        proptest!(|(begin in srt_skew(), end in srt_skew(), t in -0.5f64..1.5)| {
            prop_assert_eq!(bits(begin.lerp(end, 0.0)), bits(begin));
            prop_assert_eq!(bits(begin.lerp(end, 1.0)), bits(end));
            let mid = begin.lerp(end, t);
            prop_assert!(mid.m.iter().all(|v| v.is_finite()), "t = {}: {:?}", t, mid);
        });
    }
}

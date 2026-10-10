//! Shared interpolation keeps representable values and authored endpoints.

use flui_foundation::geometry::{EdgeInsets, Lerp, Offset, Radius, Rect, Size};

fn opposite_extremes_have_finite_intermediate_values() {
    for magnitude in [1e308, f64::MAX] {
        for (t, expected) in [
            (0.0, magnitude),
            (0.25, magnitude * 0.5),
            (0.5, 0.0),
            (1.0, -magnitude),
        ] {
            for (begin, end, expected) in [
                (magnitude, -magnitude, expected),
                (-magnitude, magnitude, -expected),
            ] {
                let actual = begin.lerp_to(&end, t);
                assert!(actual.is_finite(), "t={t}, magnitude={magnitude}: {actual}");
                // Weighted products can round once before their sum; the
                // normalized error remains within one scalar epsilon.
                assert!((actual / magnitude - expected / magnitude).abs() <= f64::EPSILON);
            }
        }
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_eq!(magnitude.lerp_to(&magnitude, t), magnitude);
        }
    }
}

fn composite_geometry_preserves_representable_components() {
    let magnitude = 1e308;
    let opposite = -magnitude;
    assert_eq!(
        Offset::new(magnitude, opposite).lerp_to(&Offset::new(opposite, magnitude), 0.5),
        Offset::ZERO
    );
    assert_eq!(
        Size::new(magnitude, opposite).lerp_to(&Size::new(opposite, magnitude), 0.5),
        Size::ZERO
    );
    assert_eq!(
        Radius::new(magnitude, opposite).lerp_to(&Radius::new(opposite, magnitude), 0.5),
        Radius::ZERO
    );
    assert_eq!(
        EdgeInsets::new(magnitude, opposite, magnitude, opposite).lerp_to(
            &EdgeInsets::new(opposite, magnitude, opposite, magnitude),
            0.5
        ),
        EdgeInsets::ZERO
    );
    assert_eq!(
        Rect::from_ltrb(opposite, opposite, magnitude, magnitude).lerp_to(
            &Rect::from_ltrb(magnitude, magnitude, opposite, opposite),
            0.5
        ),
        Rect::ZERO
    );
}

fn endpoint_samples_keep_authored_bits() {
    let nan = f64::from_bits(0x7ff8_0000_0000_0042);
    for (begin, end) in [(-0.0, 0.0), (f64::MAX, -f64::MAX), (nan, 2.0), (1.0, nan)] {
        assert_eq!(begin.lerp_to(&end, 0.0).to_bits(), begin.to_bits());
        assert_eq!(begin.lerp_to(&end, 1.0).to_bits(), end.to_bits());
    }
}

fn extrapolation_and_nan_remain_visible() {
    for (t, expected) in [(-0.5, -2.0), (0.5, 2.0), (1.5, 6.0)] {
        assert_eq!(0.0_f64.lerp_to(&4.0, t), expected);
        assert_eq!(
            Offset::ZERO.lerp_to(&Offset::new(4.0, -4.0), t),
            Offset::new(expected, -expected)
        );
    }
    assert!(0.0_f64.lerp_to(&1.0, f64::NAN).is_nan());
    assert!(f64::NAN.lerp_to(&1.0, 0.5).is_nan());
    assert!(0.0_f64.lerp_to(&f64::NAN, 0.5).is_nan());
}

fn extrapolation_preserves_a_representable_sum_after_product_overflow() {
    for magnitude in [4.0, 1e308, f64::MAX] {
        for sign in [-1.0, 1.0] {
            let begin = sign * magnitude;
            for t in [3.0, 4.0] {
                let expected = begin * (1.0 - t * 0.5);
                let actual = begin.lerp_to(&(begin * 0.5), t);
                assert!(
                    actual.is_finite(),
                    "begin={begin}, t={t}: a representable extrapolation became {actual}"
                );
                assert!((actual / magnitude - expected / magnitude).abs() <= f64::EPSILON);
            }
        }
    }
}

#[test]
fn scalar_and_composite_lerp_preserve_representable_values() {
    crate::run_table(&[
        (
            "opposite extremes",
            opposite_extremes_have_finite_intermediate_values,
        ),
        (
            "composite geometry",
            composite_geometry_preserves_representable_components,
        ),
        ("authored endpoints", endpoint_samples_keep_authored_bits),
        (
            "extrapolation and NaN",
            extrapolation_and_nan_remain_visible,
        ),
        (
            "extrapolation product overflow",
            extrapolation_preserves_a_representable_sum_after_product_overflow,
        ),
    ]);
}

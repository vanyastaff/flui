//! Scalar geometry across the representable coordinate range.
use flui_foundation::geometry::{ApproxEq, DevicePoint, Line, Point};
fn opposite_extremes_are_distinct() {
    let min = DevicePoint::new(i32::MIN, 0);
    let max = DevicePoint::new(i32::MAX, 0);
    assert!(!min.approx_eq_eps(&max, 1.0));
    assert!(!max.approx_eq_eps(&min, 1.0));
    assert!(min.approx_eq_eps(&max, f64::from(u32::MAX)));
    assert!(!min.approx_eq_eps(&max, f64::from(u32::MAX) - 1.0));
}
fn signed_minimum_distance_is_representable() {
    let min = DevicePoint::new(i32::MIN, 0);
    let zero = DevicePoint::new(0, 0);
    let distance = f64::from(i32::MAX) + 1.0;
    assert!(min.approx_eq_eps(&zero, distance));
    assert!(!min.approx_eq_eps(&zero, distance - 1.0));
}
fn nearby_device_points_keep_inclusive_tolerance() {
    let a = DevicePoint::new(10, 20);
    let b = DevicePoint::new(11, 21);
    assert!(a.approx_eq_eps(&b, 1.0));
    assert!(!a.approx_eq_eps(&b, 0.0));
    assert!(a.approx_eq_eps(&a, 0.0));
}
#[test]
fn device_geometry_comparison_handles_the_full_integer_range() {
    crate::run_table(&[
        ("opposite extremes", opposite_extremes_are_distinct),
        (
            "signed minimum distance",
            signed_minimum_distance_is_representable,
        ),
        (
            "nearby inclusive tolerance",
            nearby_device_points_keep_inclusive_tolerance,
        ),
    ]);
}

fn double_precision_midpoints_keep_finite_extremes() {
    let endpoint = Point::new(f64::MAX, -f64::MAX);
    assert_eq!(endpoint.midpoint(endpoint), endpoint);
    let opposite = Point::new(-f64::MAX, f64::MAX);
    assert_eq!(endpoint.midpoint(opposite), Point::new(0.0, 0.0));
    assert_eq!(opposite.midpoint(endpoint), Point::new(0.0, 0.0));
    assert_eq!(Line::new(endpoint, endpoint).midpoint(), endpoint);
    assert_eq!(
        Line::new(endpoint, opposite).midpoint(),
        Point::new(0.0, 0.0)
    );
}

fn single_precision_midpoints_keep_finite_extremes() {
    let endpoint = Point::new(f32::MAX, -f32::MAX);
    assert_eq!(endpoint.midpoint(endpoint), endpoint);
    let opposite = Point::new(-f32::MAX, f32::MAX);
    assert_eq!(
        Line::new(endpoint, opposite).midpoint(),
        Point::new(0.0, 0.0)
    );
    assert_eq!(opposite.midpoint(endpoint), Point::new(0.0, 0.0));
}

fn midpoint_rounding_preserves_subnormal_coordinates() {
    let tiny = f64::from_bits(1);
    let a = Point::new(tiny, -tiny);
    assert_eq!(a.midpoint(a), a);
    let b = Point::new(f64::from_bits(2), -f64::from_bits(2));
    assert_eq!(Line::new(a, b).midpoint(), b);
    let tiny = f32::from_bits(1);
    let a = Point::new(tiny, -tiny);
    assert_eq!(a.midpoint(a), a);
    let b = Point::new(f32::from_bits(2), -f32::from_bits(2));
    assert_eq!(Line::new(a, b).midpoint(), b);
}

fn midpoint_nonfinite_coordinates_follow_ieee_behavior() {
    let a = Point::new(f64::INFINITY, f64::NEG_INFINITY);
    assert_eq!(Line::new(a, a).midpoint(), a);
    assert_eq!(a.midpoint(Point::new(1.0, 2.0)), a);
    let opposite = a.midpoint(Point::new(f64::NEG_INFINITY, f64::INFINITY));
    assert!(opposite.x.is_nan() && opposite.y.is_nan());
    let nan = Point::new(f64::NAN, 2.0).midpoint(Point::new(1.0, 4.0));
    assert!(nan.x.is_nan());
    assert_eq!(nan.y, 3.0);
    let a = Point::new(f32::INFINITY, f32::NEG_INFINITY);
    assert_eq!(Line::new(a, a).midpoint(), a);
    let opposite = a.midpoint(Point::new(f32::NEG_INFINITY, f32::INFINITY));
    assert!(opposite.x.is_nan() && opposite.y.is_nan());
    let nan = Point::new(f32::NAN, 2.0).midpoint(Point::new(1.0, 4.0));
    assert!(nan.x.is_nan());
    assert_eq!(nan.y, 3.0);
}

#[test]
fn floating_geometry_midpoints_preserve_the_scalar_range() {
    crate::run_table(&[
        (
            "double precision extremes",
            double_precision_midpoints_keep_finite_extremes,
        ),
        (
            "single precision extremes",
            single_precision_midpoints_keep_finite_extremes,
        ),
        (
            "subnormal rounding",
            midpoint_rounding_preserves_subnormal_coordinates,
        ),
        (
            "nonfinite coordinates",
            midpoint_nonfinite_coordinates_follow_ieee_behavior,
        ),
    ]);
}

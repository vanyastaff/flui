//! Approximate device geometry equality without integer overflow.
use flui_foundation::geometry::{ApproxEq, DevicePoint};
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

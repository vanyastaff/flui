//! Scalar geometry across the representable coordinate range.
use flui_foundation::geometry::{ApproxEq, Circle, DevicePoint, Line, Offset, Point, Vec2};
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

fn single_precision_distance_uses_the_returned_double_range() {
    let a = Point::new(1.701_411_8e38_f32, 0.0);
    let b = Point::new(-1.701_411_8e38_f32, 0.0);
    // Endpoints are +/- 2^127; their distance is exactly 2^128.
    let expected = 3.402_823_669_209_385e38;
    assert_eq!(a.distance(b), expected);
    assert_eq!(b.distance(a), expected);
    assert_eq!(Line::new(a, b).length(), expected);
    let a = Point::new(0.0, a.x);
    let b = Point::new(0.0, b.x);
    assert_eq!(Line::new(a, b).length(), expected);
}

fn single_precision_squared_distance_uses_the_returned_double_range() {
    let a = Point::new(1.701_411_8e38_f32, 0.0);
    let b = Point::new(-1.701_411_8e38_f32, 0.0);
    // The squared distance is exactly 2^256, still finite in f64.
    let expected = 1.157_920_892_373_162e77;
    assert_eq!(a.distance_squared(b), expected);
    assert_eq!(b.distance_squared(a), expected);
    assert_eq!(Line::new(a, b).length_squared(), expected);
    let a = Point::new(0.0, a.x);
    let b = Point::new(0.0, b.x);
    assert_eq!(Line::new(a, b).length_squared(), expected);
}

fn ordinary_and_coincident_distances_keep_their_geometry() {
    let a = Point::new(1.0_f32, 2.0);
    let b = Point::new(4.0_f32, 6.0);
    assert_eq!(a.distance(b), 5.0);
    assert_eq!(Line::new(a, b).length_squared(), 25.0);
    let endpoint = Point::new(f32::MAX, -f32::MAX);
    assert_eq!(endpoint.distance(endpoint), 0.0);
    assert_eq!(Line::new(endpoint, endpoint).length_squared(), 0.0);
    let a = Point::new(1.0_f64, 2.0);
    let b = Point::new(4.0_f64, 6.0);
    assert_eq!(Line::new(a, b).length(), 5.0);
    assert_eq!(a.distance_squared(b), 25.0);
}

#[test]
fn single_precision_geometry_distances_use_double_precision_range() {
    crate::run_table(&[
        (
            "distance range",
            single_precision_distance_uses_the_returned_double_range,
        ),
        (
            "squared distance range",
            single_precision_squared_distance_uses_the_returned_double_range,
        ),
        (
            "ordinary and coincident distances",
            ordinary_and_coincident_distances_keep_their_geometry,
        ),
    ]);
}

fn extreme_finite_vectors_keep_a_unit_direction() {
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    for (x, y) in [(f64::MAX, f64::MAX), (-f64::MAX, f64::MAX)] {
        let unit = Vec2::new(x, y).try_normalize().expect("finite direction");
        assert!((unit.x - diagonal.copysign(x)).abs() < 1e-15);
        assert!((unit.y - diagonal).abs() < 1e-15);
        assert!((unit.length() - 1.0).abs() < 1e-15);
    }
    let unit = Vec2::new(f64::MAX, f64::MAX / 2.0)
        .try_normalize()
        .expect("anisotropic finite direction");
    assert!((unit.length() - 1.0).abs() < 1e-15);
    assert_eq!(unit.x, unit.y * 2.0);
}

fn invalid_and_near_zero_vectors_use_the_fallback() {
    let fallback = Vec2::new(2.0, 3.0);
    for vector in [
        Vec2::ZERO,
        Vec2::new(f64::EPSILON, 0.0),
        Vec2::new(f64::from_bits(1), 0.0),
        Vec2::new(f64::INFINITY, 1.0),
        Vec2::new(1.0, f64::NEG_INFINITY),
        Vec2::new(f64::NAN, f64::INFINITY),
    ] {
        assert_eq!(vector.try_normalize(), None);
        assert_eq!(vector.normalize(), Vec2::ZERO);
        assert_eq!(vector.normalize_or(fallback), fallback);
    }
    assert_eq!(Vec2::new(3.0, 4.0).normalize(), Vec2::new(0.6, 0.8));
    assert_eq!(Vec2::new(f64::EPSILON * 2.0, 0.0).normalize(), Vec2::X);
    assert_eq!(Vec2::new(f32::MAX, 0.0).normalize(), Vec2::X);
}

fn line_consumer_keeps_the_extreme_direction() {
    let target = Point::new(f64::MAX, f64::MAX);
    let direction = Line::new(Point::new(0.0, 0.0), target).direction();
    assert!((direction.length() - 1.0).abs() < 1e-15);
}

fn circle_consumer_keeps_the_extreme_direction() {
    let target = Point::new(f64::MAX, f64::MAX);
    let circle = Circle::new(Point::new(0.0, 0.0), 4.0);
    let nearest = circle.nearest_point(target);
    assert!((nearest.distance(circle.center) - 4.0).abs() < 1e-14);
    assert!(nearest.x > 0.0 && nearest.y > 0.0);
}

fn extreme_offsets_keep_their_direction_and_step() {
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    let target = Offset::new(f64::MAX, f64::MAX);
    let normalized = target.normalize();
    assert!((normalized.dx - diagonal).abs() < 1e-15);
    assert!((normalized.dy - diagonal).abs() < 1e-15);
    let moved = Offset::ZERO.move_towards(target, 1.0);
    assert!((moved.dx - diagonal).abs() < 1e-15);
    assert!((moved.dy - diagonal).abs() < 1e-15);
    assert!((moved.distance() - 1.0).abs() < 1e-15);
}

fn offset_normalization_refuses_invalid_and_near_zero_input() {
    for offset in [
        Offset::ZERO,
        Offset::new(f64::EPSILON, 0.0),
        Offset::new(f64::from_bits(1), 0.0),
        Offset::new(f64::INFINITY, 1.0),
        Offset::new(1.0, f64::NEG_INFINITY),
        Offset::new(f64::NAN, 2.0),
    ] {
        assert_eq!(offset.normalize(), Offset::ZERO);
    }
    assert_eq!(Offset::new(3.0, 4.0).normalize(), Offset::new(0.6, 0.8));
    assert_eq!(
        Offset::new(f64::EPSILON * 2.0, 0.0).normalize(),
        Offset::new(1.0, 0.0)
    );
    assert_eq!(
        Offset::ZERO.move_towards(Offset::new(3.0, 4.0), 10.0),
        Offset::new(3.0, 4.0)
    );
    assert_eq!(
        Offset::ZERO.move_towards(Offset::new(3.0, 4.0), 0.0),
        Offset::ZERO
    );
}

#[test]
fn vector_normalization_keeps_finite_directions_and_refuses_invalid_input() {
    crate::run_table(&[
        (
            "extreme offsets and movement",
            extreme_offsets_keep_their_direction_and_step,
        ),
        (
            "offset normalization admission",
            offset_normalization_refuses_invalid_and_near_zero_input,
        ),
        (
            "finite extremes",
            extreme_finite_vectors_keep_a_unit_direction,
        ),
        (
            "fallback admission",
            invalid_and_near_zero_vectors_use_the_fallback,
        ),
        ("line consumer", line_consumer_keeps_the_extreme_direction),
        (
            "circle consumer",
            circle_consumer_keeps_the_extreme_direction,
        ),
    ]);
}

fn coincident_line_endpoints_have_no_circle_intersection() {
    let circle = Circle::new(Point::new(0.0, 0.0), 2.0);
    for point in [
        Point::new(0.0, 0.0),
        Point::new(2.0, 0.0),
        Point::new(3.0, 0.0),
    ] {
        assert_eq!(circle.intersect_line(&Line::new(point, point)), None);
    }
    // Squaring this nonzero direction underflows to the same computed zero.
    assert_eq!(
        circle.intersect_line(&Line::new(
            Point::new(0.0, 0.0),
            Point::new(f64::from_bits(1), 0.0)
        )),
        None
    );
    let hits = circle
        .intersect_line(&Line::new(Point::new(-3.0, 0.0), Point::new(3.0, 0.0)))
        .expect("ordinary crossing after refusal");
    assert_eq!(hits, (Point::new(-2.0, 0.0), Point::new(2.0, 0.0)));
}

fn circle_intersection_keeps_infinite_line_and_tangent_behavior() {
    let circle = Circle::new(Point::new(0.0, 0.0), 2.0);
    let hits = circle
        .intersect_line(&Line::new(Point::new(3.0, 0.0), Point::new(4.0, 0.0)))
        .expect("intersection outside the endpoint segment");
    assert_eq!(hits, (Point::new(-2.0, 0.0), Point::new(2.0, 0.0)));
    assert_eq!(
        circle.intersect_line(&Line::new(Point::new(-1.0, 2.0), Point::new(1.0, 2.0))),
        Some((Point::new(0.0, 2.0), Point::new(0.0, 2.0)))
    );
    assert_eq!(
        circle.intersect_line(&Line::new(Point::new(-1.0, 3.0), Point::new(1.0, 3.0))),
        None
    );
}

#[test]
fn circle_line_intersection_requires_a_computed_direction() {
    crate::run_table(&[
        (
            "coincident endpoints",
            coincident_line_endpoints_have_no_circle_intersection,
        ),
        (
            "infinite line and tangent",
            circle_intersection_keeps_infinite_line_and_tangent_behavior,
        ),
    ]);
}

fn circle_containment_keeps_finite_large_and_small_radii() {
    for radius in [
        1.0,
        2.0_f64.powi(600),
        2.0_f64.powi(-600),
        f64::MIN_POSITIVE,
        f64::MAX,
    ] {
        let circle = Circle::from_radius(radius);
        let center = Point::new(0.0, 0.0);
        let inside = Point::new(radius * 0.5, radius * 0.5);
        let outside = Point::new(radius * 0.75, radius * 0.75);
        let boundary = Point::new(radius, 0.0);
        assert!(circle.contains(center), "center at radius {radius}");
        assert!(
            circle.contains_strict(center),
            "strict center at radius {radius}"
        );
        assert!(circle.contains(inside), "inside at radius {radius}");
        assert!(
            circle.contains_strict(inside),
            "strict inside at radius {radius}"
        );
        assert!(!circle.contains(outside), "outside at radius {radius}");
        assert!(
            !circle.contains_strict(outside),
            "strict outside at radius {radius}"
        );
        assert!(circle.contains(boundary), "boundary at radius {radius}");
        assert!(
            !circle.contains_strict(boundary),
            "strict boundary at radius {radius}"
        );
    }
}

fn circle_containment_keeps_zero_and_subnormal_radii() {
    let tiny = f64::from_bits(1);
    let zero = Circle::from_radius(0.0);
    assert!(zero.contains(Point::new(0.0, 0.0)));
    assert!(!zero.contains_strict(Point::new(0.0, 0.0)));
    assert!(!zero.contains(Point::new(tiny, 0.0)));
    let circle = Circle::from_radius(tiny);
    assert!(circle.contains_strict(Point::new(0.0, 0.0)));
    assert!(circle.contains(Point::new(tiny, 0.0)));
    assert!(!circle.contains_strict(Point::new(tiny, 0.0)));
    assert!(!circle.contains(Point::new(tiny * 2.0, 0.0)));
}

fn circle_containment_rejects_opposite_finite_extremes() {
    let circle = Circle::new(Point::new(f64::MAX, 0.0), f64::MAX);
    assert!(circle.contains_strict(circle.center));
    assert!(circle.contains(Point::new(0.0, 0.0)));
    assert!(!circle.contains_strict(Point::new(0.0, 0.0)));
    assert!(!circle.contains(Point::new(-f64::MAX, 0.0)));
    assert!(!circle.contains_strict(Point::new(-f64::MAX, 0.0)));
}

#[test]
fn circle_containment_preserves_finite_distance_ranges() {
    crate::run_table(&[
        (
            "finite radii",
            circle_containment_keeps_finite_large_and_small_radii,
        ),
        (
            "zero and subnormal radii",
            circle_containment_keeps_zero_and_subnormal_radii,
        ),
        (
            "opposite finite extremes",
            circle_containment_rejects_opposite_finite_extremes,
        ),
    ]);
}

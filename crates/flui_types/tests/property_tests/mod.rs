//! Property-based tests for geometric invariants
//!
//! Uses proptest to verify mathematical properties that must hold for all inputs.
//! These tests validate contracts defined in specs/001-flui-types/contracts/README.md

use proptest::prelude::*;
use flui_types::geometry::{Pixels, Point, Rect, Size, Offset, px};

// ============================================================================
// Arbitrary generators for property testing
// ============================================================================

/// Generate arbitrary Pixels values in a reasonable range for UI coordinates
fn arb_pixels() -> impl Strategy<Value = Pixels> {
    (-10000.0f32..=10000.0f32).prop_map(Pixels)
}

/// Generate arbitrary positive Pixels for sizes (must be >= 0)
fn arb_positive_pixels() -> impl Strategy<Value = Pixels> {
    (0.0f32..=10000.0f32).prop_map(Pixels)
}

/// Generate arbitrary Points
fn arb_point() -> impl Strategy<Value = Point<Pixels>> {
    (arb_pixels(), arb_pixels())
        .prop_map(|(x, y)| Point::new(x, y))
}

/// Generate arbitrary Sizes (width and height must be non-negative)
fn arb_size() -> impl Strategy<Value = Size<Pixels>> {
    (arb_positive_pixels(), arb_positive_pixels())
        .prop_map(|(w, h)| Size::new(w, h))
}

/// Generate arbitrary Rects
fn arb_rect() -> impl Strategy<Value = Rect<Pixels>> {
    (arb_point(), arb_size())
        .prop_map(|(origin, size)| Rect::new(origin, size))
}

/// Generate arbitrary Offsets
fn arb_offset() -> impl Strategy<Value = Offset<Pixels>> {
    (arb_pixels(), arb_pixels())
        .prop_map(|(dx, dy)| Offset::new(dx, dy))
}

// ============================================================================
// Property tests for Point
// ============================================================================

proptest! {
    /// Property: Distance from A to B equals distance from B to A (symmetry)
    #[test]
    fn prop_point_distance_symmetric(a in arb_point(), b in arb_point()) {
        let dist_ab = a.distance_to(b);
        let dist_ba = b.distance_to(a);

        // Allow small floating-point error
        let epsilon = px(1e-5);
        prop_assert!((dist_ab - dist_ba).abs() < epsilon,
            "Distance must be symmetric: distance({:?}, {:?}) = {}, but distance({:?}, {:?}) = {}",
            a, b, dist_ab, b, a, dist_ba);
    }

    /// Property: Distance is always non-negative
    #[test]
    fn prop_point_distance_non_negative(a in arb_point(), b in arb_point()) {
        let dist = a.distance_to(b);
        prop_assert!(dist >= px(0.0),
            "Distance must be non-negative: distance({:?}, {:?}) = {}",
            a, b, dist);
    }

    /// Property: Distance from a point to itself is zero
    #[test]
    fn prop_point_distance_to_self_is_zero(p in arb_point()) {
        let dist = p.distance_to(p);
        let epsilon = px(1e-6);
        prop_assert!(dist < epsilon,
            "Distance to self must be zero: distance({:?}, {:?}) = {}",
            p, p, dist);
    }

    /// Property: Triangle inequality (dist(A,C) <= dist(A,B) + dist(B,C))
    #[test]
    fn prop_point_triangle_inequality(a in arb_point(), b in arb_point(), c in arb_point()) {
        let dist_ac = a.distance_to(c);
        let dist_ab = a.distance_to(b);
        let dist_bc = b.distance_to(c);

        let epsilon = px(1e-4); // Larger epsilon for accumulated error
        prop_assert!(dist_ac <= dist_ab + dist_bc + epsilon,
            "Triangle inequality violated: dist({:?},{:?})={} > dist({:?},{:?})={} + dist({:?},{:?})={}",
            a, c, dist_ac, a, b, dist_ab, b, c, dist_bc);
    }
}

// ============================================================================
// Property tests for Rect
// ============================================================================

proptest! {
    /// Property: Rectangle intersection is commutative (A ∩ B = B ∩ A)
    #[test]
    fn prop_rect_intersection_commutative(a in arb_rect(), b in arb_rect()) {
        let int_ab = a.intersect(b);
        let int_ba = b.intersect(a);

        prop_assert_eq!(int_ab, int_ba,
            "Intersection must be commutative: {:?}.intersect({:?}) != {:?}.intersect({:?})",
            a, b, b, a);
    }

    /// Property: Union contains both rectangles
    #[test]
    fn prop_rect_union_contains_both(a in arb_rect(), b in arb_rect()) {
        let union = a.union(b);

        // Union should contain all corners of both rectangles
        let a_corners = [
            a.origin,
            Point::new(a.origin.x + a.size.width, a.origin.y),
            Point::new(a.origin.x, a.origin.y + a.size.height),
            Point::new(a.origin.x + a.size.width, a.origin.y + a.size.height),
        ];

        let b_corners = [
            b.origin,
            Point::new(b.origin.x + b.size.width, b.origin.y),
            Point::new(b.origin.x, b.origin.y + b.size.height),
            Point::new(b.origin.x + b.size.width, b.origin.y + b.size.height),
        ];

        for corner in &a_corners {
            prop_assert!(union.contains(*corner),
                "Union {:?} must contain corner {:?} from rect A {:?}",
                union, corner, a);
        }

        for corner in &b_corners {
            prop_assert!(union.contains(*corner),
                "Union {:?} must contain corner {:?} from rect B {:?}",
                union, corner, b);
        }
    }

    /// Property: A rect always intersects itself
    #[test]
    fn prop_rect_intersects_self(r in arb_rect()) {
        prop_assert!(r.intersects(r),
            "Rect must intersect itself: {:?}",
            r);
    }

    /// Property: If A contains B, then A intersects B
    #[test]
    fn prop_rect_contains_implies_intersects(
        origin in arb_point(),
        outer_size in arb_size(),
        offset in arb_offset()
    ) {
        let outer = Rect::new(origin, outer_size);

        // Create inner rect that's guaranteed to be inside
        let inner_origin = Point::new(
            origin.x + offset.dx.abs().min(outer_size.width / px(2.0)),
            origin.y + offset.dy.abs().min(outer_size.height / px(2.0))
        );
        let inner_size = Size::new(
            outer_size.width / px(4.0),
            outer_size.height / px(4.0)
        );
        let inner = Rect::new(inner_origin, inner_size);

        if outer.contains(inner.origin) {
            prop_assert!(outer.intersects(inner),
                "If outer {:?} contains inner origin {:?}, it must intersect inner {:?}",
                outer, inner.origin, inner);
        }
    }
}

// ============================================================================
// Property tests for Size
// ============================================================================

proptest! {
    /// Property: Size area is width * height
    #[test]
    fn prop_size_area(size in arb_size()) {
        let area = size.area();
        let expected = size.width * size.height;

        let epsilon = px(1e-4);
        prop_assert!((area - expected).abs() < epsilon,
            "Area must equal width * height: {:?}.area() = {}, expected {}",
            size, area, expected);
    }

    /// Property: Empty size has zero area
    #[test]
    fn prop_empty_size_has_zero_area() {
        let empty = Size::new(px(0.0), px(0.0));
        prop_assert!(empty.is_empty(),
            "Zero-sized rect must be empty");
        prop_assert_eq!(empty.area(), px(0.0),
            "Empty size must have zero area");
    }

    /// Property: Scaling size by factor scales area by factor²
    #[test]
    fn prop_size_scale_area(size in arb_size(), factor in 0.1f32..=10.0f32) {
        let original_area = size.area();
        let scaled = size.scale(factor);
        let scaled_area = scaled.area();

        let expected_area = original_area * (factor * factor);
        let epsilon = px(1e-3); // Larger epsilon for multiplication errors

        prop_assert!((scaled_area - expected_area).abs() < epsilon,
            "Scaled area must be original_area * factor²: {:?}.scale({}) area = {}, expected {}",
            size, factor, scaled_area, expected_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arb_pixels_generates_valid_range() {
        // Just verify generators work
        let strategy = arb_pixels();
        let value = strategy.new_tree(&mut proptest::test_runner::TestRunner::default())
            .unwrap()
            .current();

        assert!(value.0.abs() <= 10000.0, "Generated Pixels should be in range");
    }
}

//! Property-based tests for geometric invariants
//!
//! Uses proptest to verify mathematical properties that must hold for all
//! inputs. These tests validate contracts defined in
//! specs/001-flui-types/contracts/README.md

use flui_types::geometry::{Offset, Pixels, Point, Rect, Size, px};
use proptest::prelude::*;

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
    (arb_pixels(), arb_pixels()).prop_map(|(x, y)| Point::new(x, y))
}

/// Generate arbitrary Sizes (width and height must be non-negative)
fn arb_size() -> impl Strategy<Value = Size<Pixels>> {
    (arb_positive_pixels(), arb_positive_pixels()).prop_map(|(w, h)| Size::new(w, h))
}

/// Generate arbitrary Rects
fn arb_rect() -> impl Strategy<Value = Rect<Pixels>> {
    (arb_point(), arb_size()).prop_map(|(origin, size)| Rect::from_origin_size(origin, size))
}

/// Generate arbitrary Offsets
fn arb_offset() -> impl Strategy<Value = Offset<Pixels>> {
    (arb_pixels(), arb_pixels()).prop_map(|(dx, dy)| Offset::new(dx, dy))
}

// ============================================================================
// Property tests for Point
// ============================================================================

proptest! {
    /// Property: Distance from A to B equals distance from B to A (symmetry)
    #[test]
    fn prop_point_distance_symmetric(a in arb_point(), b in arb_point()) {
        let dist_ab = a.distance(b);
        let dist_ba = b.distance(a);

        // Allow small floating-point error
        let epsilon = 1e-5;
        prop_assert!((dist_ab - dist_ba).abs() < epsilon,
            "Distance must be symmetric: distance({:?}, {:?}) = {}, but distance({:?}, {:?}) = {}",
            a, b, dist_ab, b, a, dist_ba);
    }

    /// Property: Distance is always non-negative
    #[test]
    fn prop_point_distance_non_negative(a in arb_point(), b in arb_point()) {
        let dist = a.distance(b);
        prop_assert!(dist >= 0.0,
            "Distance must be non-negative: distance({:?}, {:?}) = {}",
            a, b, dist);
    }

    /// Property: Distance from a point to itself is zero
    #[test]
    fn prop_point_distance_self_is_zero(p in arb_point()) {
        let dist = p.distance(p);
        let epsilon = 1e-6;
        prop_assert!(dist < epsilon,
            "Distance to self must be zero: distance({:?}, {:?}) = {}",
            p, p, dist);
    }

    /// Property: Triangle inequality (dist(A,C) <= dist(A,B) + dist(B,C))
    #[test]
    fn prop_point_triangle_inequality(a in arb_point(), b in arb_point(), c in arb_point()) {
        let dist_ac = a.distance(c);
        let dist_ab = a.distance(b);
        let dist_bc = b.distance(c);

        let epsilon = 1e-4; // Larger epsilon for accumulated error
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
        let int_ab = a.intersect(&b);
        let int_ba = b.intersect(&a);

        prop_assert_eq!(int_ab, int_ba,
            "Intersection must be commutative: {:?}.intersect({:?}) != {:?}.intersect({:?})",
            a, b, b, a);
    }

    /// Property: Union contains both rectangles
    #[test]
    fn prop_rect_union_contains_both(a in arb_rect(), b in arb_rect()) {
        let union = a.union(&b);

        // Union should contain all corners of both rectangles
        let a_corners = [
            a.origin(),
            Point::new(a.origin().x + a.size().width, a.origin().y),
            Point::new(a.origin().x, a.origin().y + a.size().height),
            Point::new(a.origin().x + a.size().width, a.origin().y + a.size().height),
        ];

        let b_corners = [
            b.origin(),
            Point::new(b.origin().x + b.size().width, b.origin().y),
            Point::new(b.origin().x, b.origin().y + b.size().height),
            Point::new(b.origin().x + b.size().width, b.origin().y + b.size().height),
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
        prop_assert!(r.intersects(&r),
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
        let outer = Rect::from_origin_size(origin, outer_size);

        // Create inner rect that's guaranteed to be inside
        let inner_origin = Point::new(
            origin.x + offset.dx.abs().min(outer_size.width / 2.0),
            origin.y + offset.dy.abs().min(outer_size.height / 2.0)
        );
        let inner_size = Size::new(
            outer_size.width / 4.0,
            outer_size.height / 4.0
        );
        let inner = Rect::from_origin_size(inner_origin, inner_size);

        if outer.contains(inner.origin()) {
            prop_assert!(outer.intersects(&inner),
                "If outer {:?} contains inner origin {:?}, it must intersect inner {:?}",
                outer, inner.origin(), inner);
        }
    }
}

// ============================================================================
// Property tests for Size
// ============================================================================

// Size property tests (outside proptest! macro for compatibility)

#[test]
fn test_size_area_is_width_times_height() {
    let sizes = [
        Size::new(px(3.0), px(4.0)),
        Size::new(px(100.0), px(200.0)),
        Size::new(px(0.5), px(0.5)),
        Size::new(px(1.0), px(1.0)),
    ];
    for size in &sizes {
        let area = size.area();
        let w: f32 = size.width.into();
        let h: f32 = size.height.into();
        let expected = w * h;
        assert!(
            (area - expected).abs() < 1e-4,
            "Area must equal width * height: {size:?}.area() = {area}, expected {expected}"
        );
    }
}

#[test]
fn test_empty_size_has_zero_area() {
    let empty = Size::new(px(0.0), px(0.0));
    assert!(empty.is_empty(), "Zero-sized rect must be empty");
    assert!(
        (empty.area() - 0.0f32).abs() < f32::EPSILON,
        "Empty size must have zero area"
    );
}

#[test]
fn test_nonempty_size_has_positive_area() {
    let sizes = [
        Size::new(px(1.0), px(1.0)),
        Size::new(px(0.001), px(0.001)),
        Size::new(px(9999.0), px(9999.0)),
    ];
    for size in &sizes {
        assert!(
            size.area() > 0.0,
            "Non-empty size must have positive area: {:?}.area() = {}",
            size,
            size.area()
        );
    }
}

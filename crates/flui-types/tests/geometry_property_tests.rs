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

/// A float in `[lo, hi]` drawn WITHOUT proptest's uniform float sampler.
///
/// That sampler panics from inside its own strategy on some seeds (#889:
/// `assertion failed: self.low - result < self.intervals.step`,
/// `proptest-1.11.0/src/num/float_samplers.rs:466`), which fires while
/// *generating* a value, before any assertion here runs.
///
/// The grid is 2^24 steps, not a round decimal, so subpixel and
/// near-degenerate geometry stays reachable: across a 20,000 px range that is
/// ~0.0012 px between neighbours. A coarse grid would quietly narrow these
/// tests to whole-pixel cases, which is the opposite of what a geometry
/// property suite is for.
///
/// The arithmetic runs in `f64` because `f64: From<u32>` is exact for every
/// step index, where `f32: From<u32>` does not exist at all. Only the final
/// narrowing is lossy, and that is the point — the value has to land in the
/// target type.
fn float_in(lo: f32, hi: f32) -> impl Strategy<Value = f32> {
    const STEPS: u32 = 1 << 24;
    (0u32..=STEPS).prop_map(move |n| {
        let t = f64::from(n) / f64::from(STEPS);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "narrowing to the target type is the purpose; the \
                      arithmetic above is exact in f64"
        )]
        let v = (f64::from(lo) + t * (f64::from(hi) - f64::from(lo))) as f32;
        v
    })
}

/// Generate arbitrary Pixels values in a reasonable range for UI coordinates
fn arb_pixels() -> impl Strategy<Value = Pixels> {
    float_in(-10000.0, 10000.0).prop_map(Pixels)
}

/// Generate arbitrary positive Pixels for sizes (must be >= 0)
fn arb_positive_pixels() -> impl Strategy<Value = Pixels> {
    float_in(0.0, 10000.0).prop_map(Pixels)
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

        // The tolerance must scale with magnitude: near-collinear points at
        // coordinate ~8000 make dist_ac exceed the sum by ~1 ULP of f32
        // (~0.001 there), which a fixed absolute epsilon cannot cover without
        // being uselessly loose at small magnitudes. Absolute + relative term.
        let tolerance = 1e-4 + (dist_ab + dist_bc) * 8.0 * f32::EPSILON;
        prop_assert!(dist_ac <= dist_ab + dist_bc + tolerance,
            "Triangle inequality violated beyond tolerance: dist({:?},{:?})={} > dist({:?},{:?})={} + dist({:?},{:?})={} + tolerance {} (bound {})",
            a, c, dist_ac, a, b, dist_ab, b, c, dist_bc, tolerance, dist_ab + dist_bc + tolerance);
    }
}

/// Near-collinear points at large magnitude where f32 rounding makes the
/// direct distance exceed the two-leg sum by about one ULP (~0.001 at
/// coordinate ~8000). Found by proptest in CI; pinned deterministically so
/// the tolerance can never regress to a purely absolute epsilon.
#[test]
fn triangle_inequality_tolerates_one_ulp_at_large_magnitude() {
    let a = Point::new(Pixels(7882.3926), Pixels(4223.987));
    let b = Point::new(Pixels(2886.2673), Pixels(1433.5686));
    let c = Point::new(Pixels(622.8746), Pixels(170.34543));

    let dist_ac = a.distance(c);
    let dist_ab = a.distance(b);
    let dist_bc = b.distance(c);

    // Deliberately NOT asserted: that this triple violates the absolute-only
    // 1e-4 bound. It did on the CI host that found it, but `distance` bottoms
    // out in `f32::hypot`, whose last-ULP rounding is platform- and
    // toolchain-dependent — a conforming implementation may round these
    // distances so the absolute bound already holds, and the test must not
    // fail on such a platform. The property defended here is only that the
    // magnitude-scaled tolerance covers the worst rounding this shape produces.
    let tolerance = 1e-4 + (dist_ab + dist_bc) * 8.0 * f32::EPSILON;
    assert!(
        dist_ac <= dist_ab + dist_bc + tolerance,
        "magnitude-scaled tolerance must cover the one-ULP rounding excess \
         (dist_ac={dist_ac}, bound={})",
        dist_ab + dist_bc + tolerance
    );
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

    /// Property: a rect with area intersects itself, and an EMPTY one does
    /// not — including with itself.
    ///
    /// `overlaps` uses strict inequalities, so a zero-width or zero-height
    /// rect overlaps nothing at all. That is the half-open convention and it
    /// is self-consistent: an empty region contains no points, so it can share
    /// none. (Not verified against the reference here — `Rect.overlaps` lives
    /// in `dart:ui`, which the local `.flutter` clone does not include.)
    ///
    /// The unqualified form of this property — "a rect always intersects
    /// itself" — was false for empty rects and passed only because the old
    /// generator essentially never produced one. It does now: the strategy
    /// draws from a finite grid and hits its endpoints.
    #[test]
    fn prop_rect_intersects_self_iff_it_has_area(r in arb_rect()) {
        let has_area = r.max.x > r.min.x && r.max.y > r.min.y;
        prop_assert_eq!(
            r.intersects(&r),
            has_area,
            "a rect intersects itself exactly when it has area: {:?}",
            r
        );
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

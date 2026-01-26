//! Geometric calculations tests for Phase 5 (User Story 3)
//!
//! Tests for Point, Size, Rect, and Offset operations:
//! - Distance calculations
//! - Rectangle intersections and unions
//! - Offset magnitude and normalization
//! - Geometric invariants and properties

use flui_types::geometry::{
    px, Pixels, Point, Size, Rect, Offset, Vec2,
};

// ============================================================================
// T039-T040: Point distance calculations
// ============================================================================

#[test]
fn test_point_distance_basic() {
    let p1 = Point::new(px(0.0), px(0.0));
    let p2 = Point::new(px(3.0), px(4.0));

    let dist = p1.distance(p2);
    assert_eq!(dist, 5.0); // 3-4-5 triangle
}

#[test]
fn test_point_distance_symmetric() {
    let p1 = Point::new(px(10.0), px(20.0));
    let p2 = Point::new(px(30.0), px(40.0));

    let dist_12 = p1.distance(p2);
    let dist_21 = p2.distance(p1);

    assert_eq!(dist_12, dist_21, "Distance must be symmetric");
}

#[test]
fn test_point_distance_to_self_is_zero() {
    let p = Point::new(px(100.0), px(200.0));
    let dist = p.distance(p);
    assert_eq!(dist, 0.0);
}

#[test]
fn test_point_distance_non_negative() {
    let p1 = Point::new(px(-50.0), px(-30.0));
    let p2 = Point::new(px(20.0), px(10.0));

    let dist = p1.distance(p2);
    assert!(dist >= 0.0, "Distance must be non-negative");
}

#[test]
fn test_point_distance_squared() {
    let p1 = Point::new(px(0.0), px(0.0));
    let p2 = Point::new(px(3.0), px(4.0));

    let dist_sq = p1.distance_squared(p2);
    assert_eq!(dist_sq, 25.0); // 3^2 + 4^2 = 25
}

#[test]
fn test_point_triangle_inequality() {
    // For any three points A, B, C: distance(A, B) + distance(B, C) >= distance(A, C)
    let a = Point::new(px(0.0), px(0.0));
    let b = Point::new(px(10.0), px(0.0));
    let c = Point::new(px(10.0), px(10.0));

    let ab = a.distance(b);
    let bc = b.distance(c);
    let ac = a.distance(c);

    assert!(ab + bc >= ac - 0.001, "Triangle inequality violated");
}

// ============================================================================
// T043: Offset magnitude and normalize
// ============================================================================

#[test]
fn test_offset_magnitude() {
    let offset = Offset::new(px(3.0), px(4.0));
    let magnitude = offset.distance();
    assert_eq!(magnitude, px(5.0)); // 3-4-5 triangle
}

#[test]
fn test_offset_magnitude_zero() {
    let offset = Offset::new(px(0.0), px(0.0));
    let magnitude = offset.distance();
    assert_eq!(magnitude, px(0.0));
}

#[test]
fn test_offset_normalize() {
    let offset = Offset::new(px(3.0), px(4.0));
    let normalized = offset.normalize();

    // Normalized vector should have magnitude 1.0
    let mag = normalized.distance();
    assert!((mag.get() - 1.0).abs() < 0.001,
            "Normalized vector should have magnitude 1.0, got {}", mag);

    // Direction should be preserved
    let expected_x = 3.0 / 5.0; // 0.6
    let expected_y = 4.0 / 5.0; // 0.8
    assert!((normalized.dx.get() - expected_x).abs() < 0.001);
    assert!((normalized.dy.get() - expected_y).abs() < 0.001);
}

#[test]
fn test_offset_normalize_preserves_direction() {
    let offset = Offset::new(px(100.0), px(0.0));
    let normalized = offset.normalize();

    assert_eq!(normalized.dx, px(1.0));
    assert_eq!(normalized.dy, px(0.0));
}

#[test]
fn test_offset_from_direction() {
    let offset = Offset::from_direction(0.0, 10.0); // 0 radians = right
    assert!((offset.dx.get() - 10.0).abs() < 0.001);
    assert!(offset.dy.get().abs() < 0.001);
}

// ============================================================================
// T041: Rectangle intersection
// ============================================================================

#[test]
fn test_rect_intersects_overlapping() {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));

    assert!(rect1.intersects(&rect2));
    assert!(rect2.intersects(&rect1), "Intersection must be commutative");
}

#[test]
fn test_rect_intersects_non_overlapping() {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
    let rect2 = Rect::from_xywh(px(100.0), px(100.0), px(50.0), px(50.0));

    assert!(!rect1.intersects(&rect2));
    assert!(!rect2.intersects(&rect1));
}

#[test]
fn test_rect_intersects_touching_edges() {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
    let rect2 = Rect::from_xywh(px(50.0), px(0.0), px(50.0), px(50.0));

    // Touching edges should not count as intersection
    assert!(!rect1.intersects(&rect2));
}

#[test]
fn test_rect_intersect_result() {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));

    let intersection = rect1.intersect(&rect2);
    assert!(intersection.is_some());

    let result = intersection.unwrap();
    assert_eq!(result.left(), px(50.0));
    assert_eq!(result.top(), px(50.0));
    assert_eq!(result.right(), px(100.0));
    assert_eq!(result.bottom(), px(100.0));
}

#[test]
fn test_rect_intersect_commutative() {
    let rect1 = Rect::from_xywh(px(10.0), px(10.0), px(80.0), px(80.0));
    let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));

    let int1 = rect1.intersect(&rect2);
    let int2 = rect2.intersect(&rect1);

    assert_eq!(int1, int2, "Intersection must be commutative");
}

#[test]
fn test_rect_intersect_self() {
    let rect = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));
    let intersection = rect.intersect(&rect);

    assert!(intersection.is_some());
    assert_eq!(intersection.unwrap(), rect);
}

#[test]
fn test_rect_intersect_contained() {
    let outer = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let inner = Rect::from_xywh(px(25.0), px(25.0), px(50.0), px(50.0));

    let intersection = outer.intersect(&inner);
    assert!(intersection.is_some());
    assert_eq!(intersection.unwrap(), inner, "Intersection with contained rect is the inner rect");
}

// ============================================================================
// T042: Rectangle union
// ============================================================================

#[test]
fn test_rect_union_basic() {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
    let rect2 = Rect::from_xywh(px(25.0), px(25.0), px(50.0), px(50.0));

    let union = rect1.union(&rect2);

    assert_eq!(union.left(), px(0.0));
    assert_eq!(union.top(), px(0.0));
    assert_eq!(union.right(), px(75.0));
    assert_eq!(union.bottom(), px(75.0));
}

#[test]
fn test_rect_union_contains_both() {
    let rect1 = Rect::from_xywh(px(10.0), px(10.0), px(20.0), px(20.0));
    let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(20.0), px(20.0));

    let union = rect1.union(&rect2);

    // Union must contain all corners of both rectangles
    assert!(union.contains(rect1.top_left()));
    assert!(union.contains(rect1.bottom_right()));
    assert!(union.contains(rect2.top_left()));
    assert!(union.contains(rect2.bottom_right()));
}

#[test]
fn test_rect_union_commutative() {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
    let rect2 = Rect::from_xywh(px(100.0), px(100.0), px(50.0), px(50.0));

    let union1 = rect1.union(&rect2);
    let union2 = rect2.union(&rect1);

    assert_eq!(union1, union2, "Union must be commutative");
}

#[test]
fn test_rect_union_self() {
    let rect = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));
    let union = rect.union(&rect);

    assert_eq!(union, rect);
}

#[test]
fn test_rect_union_with_point() {
    let rect = Rect::from_xywh(px(10.0), px(10.0), px(20.0), px(20.0));
    let point = Point::new(px(50.0), px(50.0));

    let union = rect.union_pt(point);

    assert!(union.contains(point));
    assert!(union.contains(rect.top_left()));
    assert!(union.contains(rect.bottom_right()));
}

// ============================================================================
// T049-T051: Rectangle operations
// ============================================================================

#[test]
fn test_rect_inflate() {
    let rect = Rect::from_xywh(px(10.0), px(10.0), px(20.0), px(20.0));
    let inflated = rect.inflate(px(5.0), px(5.0));

    assert_eq!(inflated.left(), px(5.0));
    assert_eq!(inflated.top(), px(5.0));
    assert_eq!(inflated.right(), px(35.0));
    assert_eq!(inflated.bottom(), px(35.0));
    assert_eq!(inflated.width(), px(30.0));
    assert_eq!(inflated.height(), px(30.0));
}

#[test]
fn test_rect_inflate_symmetric() {
    let rect = Rect::from_xywh(px(20.0), px(20.0), px(40.0), px(40.0));
    let inflated = rect.inflate(px(10.0), px(10.0));

    // Center should remain the same
    assert_eq!(rect.center(), inflated.center());
}

#[test]
fn test_rect_inset_deflate() {
    let rect = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));
    let inset = rect.inset(px(5.0));

    assert_eq!(inset.left(), px(15.0));
    assert_eq!(inset.top(), px(15.0));
    assert_eq!(inset.right(), px(55.0));
    assert_eq!(inset.bottom(), px(55.0));
    assert_eq!(inset.width(), px(40.0));
    assert_eq!(inset.height(), px(40.0));
}

#[test]
fn test_rect_expand() {
    let rect = Rect::from_xywh(px(20.0), px(20.0), px(20.0), px(20.0));
    let expanded = rect.expand(px(10.0));

    assert_eq!(expanded.left(), px(10.0));
    assert_eq!(expanded.top(), px(10.0));
    assert_eq!(expanded.right(), px(50.0));
    assert_eq!(expanded.bottom(), px(50.0));
}

#[test]
fn test_rect_edge_accessors() {
    let rect = Rect::from_xywh(px(10.0), px(20.0), px(30.0), px(40.0));

    assert_eq!(rect.left(), px(10.0));
    assert_eq!(rect.top(), px(20.0));
    assert_eq!(rect.right(), px(40.0));
    assert_eq!(rect.bottom(), px(60.0));
    assert_eq!(rect.width(), px(30.0));
    assert_eq!(rect.height(), px(40.0));
}

#[test]
fn test_rect_center() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let center = rect.center();

    assert_eq!(center.x, px(50.0));
    assert_eq!(center.y, px(50.0));
}

#[test]
fn test_rect_corners() {
    let rect = Rect::from_xywh(px(10.0), px(20.0), px(30.0), px(40.0));

    assert_eq!(rect.top_left(), Point::new(px(10.0), px(20.0)));
    assert_eq!(rect.top_right(), Point::new(px(40.0), px(20.0)));
    assert_eq!(rect.bottom_left(), Point::new(px(10.0), px(60.0)));
    assert_eq!(rect.bottom_right(), Point::new(px(40.0), px(60.0)));
}

#[test]
fn test_rect_contains_point() {
    let rect = Rect::from_xywh(px(10.0), px(10.0), px(80.0), px(80.0));

    assert!(rect.contains(Point::new(px(50.0), px(50.0))));
    assert!(rect.contains(Point::new(px(10.0), px(10.0)))); // Top-left corner
    assert!(!rect.contains(Point::new(px(0.0), px(0.0))));
    assert!(!rect.contains(Point::new(px(100.0), px(100.0))));
}

// ============================================================================
// T052: Size operations
// ============================================================================

#[test]
fn test_size_scale() {
    let size = Size::new(px(100.0), px(200.0));
    let scaled = size * 2.0;

    assert_eq!(scaled.width, px(200.0));
    assert_eq!(scaled.height, px(400.0));
}

#[test]
fn test_size_scale_fractional() {
    let size = Size::new(px(100.0), px(200.0));
    let scaled = size * 0.5;

    assert_eq!(scaled.width, px(50.0));
    assert_eq!(scaled.height, px(100.0));
}

#[test]
fn test_size_area() {
    let size = Size::new(px(10.0), px(20.0));
    let area = size.area();

    assert_eq!(area, 200.0);
}

#[test]
fn test_size_is_empty() {
    let size1 = Size::new(px(0.0), px(10.0));
    let size2 = Size::new(px(10.0), px(0.0));
    let size3 = Size::new(px(10.0), px(10.0));

    assert!(size1.is_empty());
    assert!(size2.is_empty());
    assert!(!size3.is_empty());
}

// ============================================================================
// Point - Point = Offset operator
// ============================================================================

#[test]
fn test_point_subtraction_gives_offset() {
    let p1 = Point::new(px(100.0), px(200.0));
    let p2 = Point::new(px(30.0), px(50.0));

    let vec = p1 - p2;

    assert_eq!(vec.x, px(70.0));
    assert_eq!(vec.y, px(150.0));
}

#[test]
fn test_point_plus_vec2() {
    let point = Point::new(px(10.0), px(20.0));
    let vec = Vec2::new(px(5.0), px(3.0));

    let result = point + vec;

    assert_eq!(result.x, px(15.0));
    assert_eq!(result.y, px(23.0));
}

#[test]
fn test_point_minus_vec2() {
    let point = Point::new(px(10.0), px(20.0));
    let vec = Vec2::new(px(5.0), px(3.0));

    let result = point - vec;

    assert_eq!(result.x, px(5.0));
    assert_eq!(result.y, px(17.0));
}

#[test]
fn test_vec2_addition_associative() {
    let p = Point::new(px(10.0), px(10.0));
    let vec1 = Vec2::new(px(5.0), px(5.0));
    let vec2 = Vec2::new(px(3.0), px(3.0));

    let result1 = (p + vec1) + vec2;
    let result2 = p + (vec1 + vec2);

    assert_eq!(result1, result2);
}

// ============================================================================
// Real-world use cases
// ============================================================================

#[test]
fn test_hit_testing_scenario() {
    // Button at position (100, 100) with size 80x40
    let button = Rect::from_xywh(px(100.0), px(100.0), px(80.0), px(40.0));

    // Click inside button
    let click1 = Point::new(px(120.0), px(110.0));
    assert!(button.contains(click1), "Click inside button should hit");

    // Click outside button
    let click2 = Point::new(px(50.0), px(50.0));
    assert!(!button.contains(click2), "Click outside button should miss");
}

#[test]
fn test_clipping_scenario() {
    // Viewport
    let viewport = Rect::from_xywh(px(0.0), px(0.0), px(800.0), px(600.0));

    // Widget partially off-screen
    let widget = Rect::from_xywh(px(700.0), px(500.0), px(200.0), px(200.0));

    // Clip widget to viewport
    let visible = viewport.intersect(&widget);
    assert!(visible.is_some());

    let clipped = visible.unwrap();
    assert_eq!(clipped.left(), px(700.0));
    assert_eq!(clipped.top(), px(500.0));
    assert_eq!(clipped.right(), px(800.0)); // Clipped at viewport edge
    assert_eq!(clipped.bottom(), px(600.0)); // Clipped at viewport edge
}

#[test]
fn test_bounding_box_scenario() {
    // Multiple widgets
    let widget1 = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));
    let widget2 = Rect::from_xywh(px(100.0), px(100.0), px(50.0), px(50.0));
    let widget3 = Rect::from_xywh(px(50.0), px(200.0), px(50.0), px(50.0));

    // Calculate bounding box
    let bbox = widget1.union(&widget2).union(&widget3);

    assert_eq!(bbox.left(), px(10.0));
    assert_eq!(bbox.top(), px(10.0));
    assert_eq!(bbox.right(), px(150.0));
    assert_eq!(bbox.bottom(), px(250.0));
}

#[test]
fn test_layout_padding_scenario() {
    // Container with padding
    let container = Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(200.0));
    let padding = px(20.0);

    // Content area after padding
    let content = container.inset(padding);

    assert_eq!(content.left(), px(20.0));
    assert_eq!(content.top(), px(20.0));
    assert_eq!(content.right(), px(180.0));
    assert_eq!(content.bottom(), px(180.0));
    assert_eq!(content.width(), px(160.0));
    assert_eq!(content.height(), px(160.0));
}

#[test]
fn test_drag_distance_scenario() {
    // Mouse down at one point
    let start = Point::new(px(100.0), px(100.0));

    // Mouse moved to another point
    let current = Point::new(px(120.0), px(105.0));

    // Calculate drag distance
    let drag_vec = current - start;
    let drag_distance = drag_vec.length();

    // Should detect significant drag (> 5px threshold)
    assert!(drag_distance > 5.0, "Drag distance should exceed threshold");
}

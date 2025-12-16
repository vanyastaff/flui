//! Integration tests for hit testing.
//!
//! These tests verify that hit testing works correctly through the render tree,
//! including proper bounds checking, child hit testing, and result accumulation.
//!
//! # Hit Testing Architecture
//!
//! FLUI uses `HitTestBehavior` to control how render objects respond to hits:
//!
//! - `Opaque` (default): Returns true if position is within bounds
//! - `DeferToChild`: Returns true only if a child was hit
//! - `Translucent`: Adds to result but returns based on children
//!
//! By default, RenderBox uses `Opaque` behavior, meaning any render object
//! will report a hit when the position is within its bounds.

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::objects::r#box::basic::{RenderPadding, RenderSizedBox};
use flui_rendering::traits::{BoxHitTestResult, RenderBox};
use flui_types::{EdgeInsets, Offset, Size};

// ============================================================================
// Helper Functions
// ============================================================================

fn create_sized_box(width: f32, height: f32) -> Box<dyn RenderBox> {
    Box::new(RenderSizedBox::fixed(width, height))
}

fn layout_render_box(render_box: &mut dyn RenderBox, max_size: Size) -> Size {
    let constraints = BoxConstraints::loose(max_size);
    render_box.perform_layout(constraints)
}

// ============================================================================
// Basic Hit Test Tests
// ============================================================================

#[test]
fn test_hit_test_result_creation() {
    let result = BoxHitTestResult::new();
    assert!(result.is_empty());
}

#[test]
fn test_sized_box_hit_test_inside_bounds() {
    // With Opaque behavior (default), hit_test returns true when inside bounds
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(50.0, 25.0);
    let hit = sized_box.hit_test(&mut result, position);

    // Default Opaque behavior: returns true when inside bounds
    assert!(
        hit,
        "SizedBox should report hit when position is inside bounds"
    );
    assert!(!result.is_empty(), "Result should contain an entry");
}

#[test]
fn test_sized_box_hit_test_at_origin() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(0.0, 0.0);
    let hit = sized_box.hit_test(&mut result, position);

    // Origin (0,0) is inside bounds [0, 100) x [0, 50)
    assert!(hit, "Origin should be inside bounds");
}

#[test]
fn test_sized_box_hit_test_outside_bounds() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(150.0, 25.0); // x is outside 0-100
    let hit = sized_box.hit_test(&mut result, position);

    assert!(!hit, "Position outside bounds should not hit");
    assert!(result.is_empty(), "Result should be empty for miss");
}

#[test]
fn test_sized_box_hit_test_at_edge_boundary() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    // Test exactly at the edge (exclusive boundary)
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(100.0, 50.0); // exactly at max boundary
    let hit = sized_box.hit_test(&mut result, position);

    // Edge is exclusive: bounds are [0, 100) x [0, 50)
    assert!(!hit, "Position at exclusive boundary should not hit");
}

#[test]
fn test_sized_box_hit_test_just_inside_edge() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(99.9, 49.9); // just inside edge
    let hit = sized_box.hit_test(&mut result, position);

    assert!(hit, "Position just inside edge should hit");
}

// ============================================================================
// Negative Position Tests
// ============================================================================

#[test]
fn test_hit_test_negative_x() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(-10.0, 25.0);
    let hit = sized_box.hit_test(&mut result, position);

    // Negative coordinates are outside bounds
    assert!(!hit, "Negative x should be outside bounds");
}

#[test]
fn test_hit_test_negative_y() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(50.0, -10.0);
    let hit = sized_box.hit_test(&mut result, position);

    assert!(!hit, "Negative y should be outside bounds");
}

#[test]
fn test_hit_test_both_negative() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(-10.0, -10.0);
    let hit = sized_box.hit_test(&mut result, position);

    assert!(!hit, "Both negative should be outside bounds");
}

// ============================================================================
// Padding Hit Test Tests
// ============================================================================

#[test]
fn test_padding_hit_test_in_padding_area() {
    // Padding with Opaque behavior should hit in padding area
    let child = create_sized_box(100.0, 50.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);
    layout_render_box(&mut padding, Size::new(200.0, 200.0));

    // Hit test in padding area (not in child)
    // Total size: 120x70, padding area is 0-10 on each side
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(5.0, 5.0); // inside padding, outside child
    let hit = padding.hit_test(&mut result, position);

    // With Opaque behavior, padding area reports a hit
    assert!(hit, "Padding area should report hit with Opaque behavior");
}

#[test]
fn test_padding_hit_test_in_child_area() {
    let child = create_sized_box(100.0, 50.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);
    layout_render_box(&mut padding, Size::new(200.0, 200.0));

    // Child is at offset (10, 10), size 100x50
    // So child bounds are (10, 10) to (110, 60)
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(60.0, 35.0); // inside child area
    let hit = padding.hit_test(&mut result, position);

    assert!(hit, "Child area should report hit");
}

#[test]
fn test_padding_hit_test_outside() {
    let child = create_sized_box(100.0, 50.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);
    layout_render_box(&mut padding, Size::new(200.0, 200.0));

    // Hit test outside entire padding box (size is 120x70)
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(150.0, 80.0);
    let hit = padding.hit_test(&mut result, position);

    assert!(!hit, "Outside bounds should not hit");
}

// ============================================================================
// Hit Test Result Tests
// ============================================================================

#[test]
fn test_hit_test_result_add_with_paint_offset() {
    let mut result = BoxHitTestResult::new();

    // Test add_with_paint_offset
    let hit = result.add_with_paint_offset(
        Some(Offset::new(10.0, 10.0)),
        Offset::new(50.0, 50.0),
        |_result, transformed| {
            // The transformed position should be (40, 40)
            assert_eq!(transformed, Offset::new(40.0, 40.0));
            true
        },
    );

    assert!(hit);
}

#[test]
fn test_hit_test_result_add_with_no_offset() {
    let mut result = BoxHitTestResult::new();

    let hit =
        result.add_with_paint_offset(None, Offset::new(50.0, 50.0), |_result, transformed| {
            // Without offset, position should be unchanged
            assert_eq!(transformed, Offset::new(50.0, 50.0));
            true
        });

    assert!(hit);
}

#[test]
fn test_hit_test_result_callback_returns_false() {
    let mut result = BoxHitTestResult::new();

    let hit = result.add_with_paint_offset(
        Some(Offset::new(10.0, 10.0)),
        Offset::new(50.0, 50.0),
        |_result, _transformed| {
            false // Simulate no hit
        },
    );

    assert!(!hit);
}

#[test]
fn test_hit_test_result_entries_accumulated() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();

    // Hit inside bounds
    let hit = sized_box.hit_test(&mut result, Offset::new(50.0, 50.0));
    assert!(hit);
    assert_eq!(result.entries().len(), 1, "Should have one entry after hit");

    // Hit again at different position (still inside)
    let hit2 = sized_box.hit_test(&mut result, Offset::new(75.0, 75.0));
    assert!(hit2);
    assert_eq!(
        result.entries().len(),
        2,
        "Should have two entries after second hit"
    );
}

// ============================================================================
// Zero Size Hit Test
// ============================================================================

#[test]
fn test_zero_size_hit_test() {
    let mut sized_box = RenderSizedBox::fixed(0.0, 0.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(0.0, 0.0);
    let hit = sized_box.hit_test(&mut result, position);

    // Zero size box: bounds are [0, 0) x [0, 0), nothing is inside
    assert!(!hit, "Zero size box should not be hittable");
}

#[test]
fn test_zero_width_hit_test() {
    let mut sized_box = RenderSizedBox::fixed(0.0, 50.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(0.0, 25.0);
    let hit = sized_box.hit_test(&mut result, position);

    // Zero width means x >= 0.0 && x < 0.0 is always false
    assert!(!hit, "Zero width box should not be hittable");
}

#[test]
fn test_zero_height_hit_test() {
    let mut sized_box = RenderSizedBox::fixed(50.0, 0.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(25.0, 0.0);
    let hit = sized_box.hit_test(&mut result, position);

    // Zero height means y >= 0.0 && y < 0.0 is always false
    assert!(!hit, "Zero height box should not be hittable");
}

// ============================================================================
// Large Coordinates Hit Test
// ============================================================================

#[test]
fn test_hit_test_large_coordinates_inside() {
    let mut sized_box = RenderSizedBox::fixed(10000.0, 10000.0);
    layout_render_box(&mut sized_box, Size::new(20000.0, 20000.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(5000.0, 5000.0);
    let hit = sized_box.hit_test(&mut result, position);

    assert!(hit, "Large coordinates inside bounds should hit");
}

#[test]
fn test_hit_test_large_coordinates_outside() {
    let mut sized_box = RenderSizedBox::fixed(10000.0, 10000.0);
    layout_render_box(&mut sized_box, Size::new(20000.0, 20000.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(15000.0, 15000.0);
    let hit = sized_box.hit_test(&mut result, position);

    assert!(!hit, "Large coordinates outside bounds should not hit");
}

// ============================================================================
// Nested Hit Test Tests
// ============================================================================

#[test]
fn test_nested_padding_hit_test_outer_area() {
    // Create nested padding: Padding(10) -> Padding(5) -> SizedBox(50x50)
    let inner_child = create_sized_box(50.0, 50.0);
    let inner_padding = Box::new(RenderPadding::with_child(EdgeInsets::all(5.0), inner_child));
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(10.0), inner_padding);

    layout_render_box(&mut outer_padding, Size::new(200.0, 200.0));
    // Total size: 50 + 5*2 + 10*2 = 80x80

    // Hit in outer padding area
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(5.0, 5.0); // outer padding area
    let hit = outer_padding.hit_test(&mut result, position);
    assert!(hit, "Outer padding area should hit");
}

#[test]
fn test_nested_padding_hit_test_inner_area() {
    let inner_child = create_sized_box(50.0, 50.0);
    let inner_padding = Box::new(RenderPadding::with_child(EdgeInsets::all(5.0), inner_child));
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(10.0), inner_padding);

    layout_render_box(&mut outer_padding, Size::new(200.0, 200.0));

    // Hit in inner padding area (between outer and child)
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(12.0, 12.0); // inner padding area
    let hit = outer_padding.hit_test(&mut result, position);
    assert!(hit, "Inner padding area should hit");
}

#[test]
fn test_nested_padding_hit_test_child_center() {
    let inner_child = create_sized_box(50.0, 50.0);
    let inner_padding = Box::new(RenderPadding::with_child(EdgeInsets::all(5.0), inner_child));
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(10.0), inner_padding);

    layout_render_box(&mut outer_padding, Size::new(200.0, 200.0));

    // Hit in center of innermost child
    // Child offset: 10 (outer) + 5 (inner) = 15
    // Child center: 15 + 25 = 40
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(40.0, 40.0);
    let hit = outer_padding.hit_test(&mut result, position);
    assert!(hit, "Child center should hit");
}

#[test]
fn test_nested_padding_hit_test_outside() {
    let inner_child = create_sized_box(50.0, 50.0);
    let inner_padding = Box::new(RenderPadding::with_child(EdgeInsets::all(5.0), inner_child));
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(10.0), inner_padding);

    layout_render_box(&mut outer_padding, Size::new(200.0, 200.0));

    // Hit outside all bounds
    let mut result = BoxHitTestResult::new();
    let position = Offset::new(100.0, 100.0);
    let hit = outer_padding.hit_test(&mut result, position);
    assert!(!hit, "Outside bounds should not hit");
}

// ============================================================================
// Multiple Hit Tests on Same Object
// ============================================================================

#[test]
fn test_multiple_hit_tests_same_object() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    // Test multiple positions
    let test_cases = [
        (Offset::new(0.0, 0.0), true, "origin"),
        (Offset::new(50.0, 50.0), true, "center"),
        (Offset::new(99.9, 99.9), true, "near edge"),
        (Offset::new(100.0, 100.0), false, "at edge (exclusive)"),
        (Offset::new(-1.0, 50.0), false, "negative x"),
        (Offset::new(50.0, -1.0), false, "negative y"),
        (Offset::new(150.0, 50.0), false, "beyond right"),
        (Offset::new(50.0, 150.0), false, "beyond bottom"),
    ];

    for (pos, expected, desc) in test_cases {
        let mut result = BoxHitTestResult::new();
        let hit = sized_box.hit_test(&mut result, pos);
        assert_eq!(hit, expected, "Failed for {}: {:?}", desc, pos);
    }
}

// ============================================================================
// Hit Test Entry Position Tests
// ============================================================================

#[test]
fn test_hit_test_entry_records_position() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);
    layout_render_box(&mut sized_box, Size::new(200.0, 200.0));

    let mut result = BoxHitTestResult::new();
    let position = Offset::new(42.0, 73.0);
    sized_box.hit_test(&mut result, position);

    assert_eq!(result.entries().len(), 1);
    assert_eq!(
        result.entries()[0].local_position,
        position,
        "Entry should record the hit position"
    );
}

//! Integration tests for the layout pipeline.
//!
//! These tests verify that layout propagates correctly through the render tree,
//! constraints are passed down properly, and sizes bubble up correctly.

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::objects::r#box::basic::{
    RenderAlign, RenderConstrainedBox, RenderPadding, RenderSizedBox,
};
use flui_rendering::traits::RenderBox;
use flui_types::{Alignment, EdgeInsets, Size};

// ============================================================================
// Helper Functions
// ============================================================================

fn create_sized_box(width: f32, height: f32) -> Box<dyn RenderBox> {
    Box::new(RenderSizedBox::fixed(width, height))
}

fn create_padding_with_child(padding: f32, child: Box<dyn RenderBox>) -> Box<dyn RenderBox> {
    Box::new(RenderPadding::with_child(EdgeInsets::all(padding), child))
}

fn create_align_with_child(alignment: Alignment, child: Box<dyn RenderBox>) -> Box<dyn RenderBox> {
    Box::new(RenderAlign::with_child(alignment, child))
}

// ============================================================================
// Basic Layout Tests
// ============================================================================

#[test]
fn test_sized_box_layout() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // Layout with loose constraints
    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    let size = sized_box.perform_layout(constraints);

    assert_eq!(size, Size::new(100.0, 50.0));
}

#[test]
fn test_sized_box_respects_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);

    // Layout with tight constraints smaller than requested size
    let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));
    let size = sized_box.perform_layout(constraints);

    // Size should be constrained
    assert_eq!(size, Size::new(50.0, 50.0));
}

#[test]
fn test_padding_without_child() {
    let mut padding = RenderPadding::new(EdgeInsets::all(10.0));

    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    let size = padding.perform_layout(constraints);

    // Without child, size is just the padding
    assert_eq!(size, Size::new(20.0, 20.0));
}

#[test]
fn test_padding_with_child() {
    let child = create_sized_box(100.0, 50.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    let size = padding.perform_layout(constraints);

    // Size is child size + padding on all sides
    assert_eq!(size, Size::new(120.0, 70.0));
}

#[test]
fn test_padding_asymmetric() {
    let child = create_sized_box(100.0, 50.0);
    // EdgeInsets::new(left, top, right, bottom)
    let padding_insets = EdgeInsets::new(20.0, 5.0, 10.0, 15.0);
    let mut padding = RenderPadding::with_child(padding_insets, child);

    let constraints = BoxConstraints::loose(Size::new(300.0, 300.0));
    let size = padding.perform_layout(constraints);

    // width = left(20) + child(100) + right(10) = 130
    // height = top(5) + child(50) + bottom(15) = 70
    assert_eq!(size, Size::new(130.0, 70.0));
}

// ============================================================================
// Nested Layout Tests
// ============================================================================

#[test]
fn test_nested_padding() {
    // Create: Padding(10) -> Padding(5) -> SizedBox(100x50)
    let inner_child = create_sized_box(100.0, 50.0);
    let inner_padding = create_padding_with_child(5.0, inner_child);
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(10.0), inner_padding);

    let constraints = BoxConstraints::loose(Size::new(300.0, 300.0));
    let size = outer_padding.perform_layout(constraints);

    // Size = 100 + 5*2 + 10*2 = 130 width, 50 + 5*2 + 10*2 = 80 height
    assert_eq!(size, Size::new(130.0, 80.0));
}

#[test]
fn test_deeply_nested_layout() {
    // Create: Padding(10) -> Padding(10) -> Padding(10) -> SizedBox(50x50)
    let child = create_sized_box(50.0, 50.0);
    let padding1 = create_padding_with_child(10.0, child);
    let padding2 = create_padding_with_child(10.0, padding1);
    let mut padding3 = RenderPadding::with_child(EdgeInsets::all(10.0), padding2);

    let constraints = BoxConstraints::loose(Size::new(500.0, 500.0));
    let size = padding3.perform_layout(constraints);

    // Size = 50 + 10*2*3 = 110 for both width and height
    assert_eq!(size, Size::new(110.0, 110.0));
}

// ============================================================================
// Constraints Propagation Tests
// ============================================================================

#[test]
fn test_constraints_deflation_through_padding() {
    // Padding should deflate constraints when passing to child
    let child = create_sized_box(1000.0, 1000.0); // Very large, will be constrained
    let mut padding = RenderPadding::with_child(EdgeInsets::all(50.0), child);

    // Parent constraints: max 200x200
    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    let size = padding.perform_layout(constraints);

    // Child gets max 100x100 (200 - 50*2), so padding size is 200x200
    assert_eq!(size, Size::new(200.0, 200.0));
}

#[test]
fn test_tight_constraints_propagation() {
    let child = create_sized_box(50.0, 50.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    // Tight constraints - force exact size
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    let size = padding.perform_layout(constraints);

    // Size must be exactly 100x100 due to tight constraints
    assert_eq!(size, Size::new(100.0, 100.0));
}

#[test]
fn test_constrained_box_enforces_constraints() {
    // ConstrainedBox should enforce additional constraints on child
    let child = create_sized_box(200.0, 200.0);
    let additional_constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
    let mut constrained = RenderConstrainedBox::with_child(additional_constraints, child);

    let constraints = BoxConstraints::loose(Size::new(300.0, 300.0));
    let size = constrained.perform_layout(constraints);

    // Child wants 200x200, but additional constraints limit to 100x100
    assert_eq!(size, Size::new(100.0, 100.0));
}

#[test]
fn test_constrained_box_minimum_size() {
    // ConstrainedBox with minimum constraints
    let child = create_sized_box(20.0, 20.0);
    let additional_constraints = BoxConstraints::new(50.0, f32::INFINITY, 50.0, f32::INFINITY);
    let mut constrained = RenderConstrainedBox::with_child(additional_constraints, child);

    let constraints = BoxConstraints::loose(Size::new(300.0, 300.0));
    let size = constrained.perform_layout(constraints);

    // Child wants 20x20, but minimum is 50x50
    assert_eq!(size, Size::new(50.0, 50.0));
}

// ============================================================================
// Align Layout Tests
// ============================================================================

#[test]
fn test_align_expands_to_constraints() {
    let child = create_sized_box(50.0, 50.0);
    let mut align = RenderAlign::with_child(Alignment::CENTER, child);

    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    let size = align.perform_layout(constraints);

    // Align expands to fill available space
    assert_eq!(size, Size::new(200.0, 200.0));
}

#[test]
fn test_align_with_tight_constraints() {
    let child = create_sized_box(50.0, 50.0);
    let mut align = RenderAlign::with_child(Alignment::CENTER, child);

    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    let size = align.perform_layout(constraints);

    // With tight constraints, align takes the exact size
    assert_eq!(size, Size::new(100.0, 100.0));
}

// ============================================================================
// Complex Layout Trees
// ============================================================================

#[test]
fn test_complex_layout_tree() {
    // Build tree:
    //   Padding(20)
    //     └── Align(center)
    //           └── Padding(10)
    //                 └── SizedBox(50x50)

    let sized_box = create_sized_box(50.0, 50.0);
    let inner_padding = create_padding_with_child(10.0, sized_box);
    let align = create_align_with_child(Alignment::CENTER, inner_padding);
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(20.0), align);

    let constraints = BoxConstraints::loose(Size::new(300.0, 300.0));
    let size = outer_padding.perform_layout(constraints);

    // Outer padding takes: 20*2 + align size
    // Align expands to: 300 - 20*2 = 260x260
    // So total = 300x300 (fills available)
    assert_eq!(size, Size::new(300.0, 300.0));
}

#[test]
fn test_layout_with_mixed_constraints() {
    // ConstrainedBox -> Padding -> SizedBox
    let sized_box = create_sized_box(80.0, 80.0);
    let padding = create_padding_with_child(10.0, sized_box);
    let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
    let mut constrained = RenderConstrainedBox::with_child(additional, padding);

    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    let size = constrained.perform_layout(constraints);

    // Padding wants 80 + 20 = 100, which is within [50, 150]
    assert_eq!(size, Size::new(100.0, 100.0));
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_zero_size_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);

    let constraints = BoxConstraints::tight(Size::ZERO);
    let size = sized_box.perform_layout(constraints);

    assert_eq!(size, Size::ZERO);
}

#[test]
fn test_unbounded_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);

    // Unbounded constraints (like in scrollable)
    let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
    let size = sized_box.perform_layout(constraints);

    // SizedBox should use its preferred size
    assert_eq!(size, Size::new(100.0, 100.0));
}

#[test]
fn test_partial_unbounded_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);

    // Unbounded height (like in vertical ListView)
    let constraints = BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY);
    let size = sized_box.perform_layout(constraints);

    assert_eq!(size, Size::new(100.0, 100.0));
}

// ============================================================================
// Layout Stability Tests
// ============================================================================

#[test]
fn test_layout_is_idempotent() {
    let child = create_sized_box(100.0, 50.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));

    // Layout multiple times
    let size1 = padding.perform_layout(constraints);
    let size2 = padding.perform_layout(constraints);
    let size3 = padding.perform_layout(constraints);

    // All should produce same result
    assert_eq!(size1, size2);
    assert_eq!(size2, size3);
}

#[test]
fn test_layout_with_different_constraints() {
    let child = create_sized_box(100.0, 100.0);
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    // First layout with large constraints
    let constraints1 = BoxConstraints::loose(Size::new(500.0, 500.0));
    let size1 = padding.perform_layout(constraints1);
    assert_eq!(size1, Size::new(120.0, 120.0));

    // Second layout with tighter constraints
    let constraints2 = BoxConstraints::tight(Size::new(80.0, 80.0));
    let size2 = padding.perform_layout(constraints2);
    assert_eq!(size2, Size::new(80.0, 80.0));

    // Third layout back to original
    let size3 = padding.perform_layout(constraints1);
    assert_eq!(size3, Size::new(120.0, 120.0));
}

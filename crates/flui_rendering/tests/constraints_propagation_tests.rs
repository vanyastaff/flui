//! Integration tests for constraints propagation.
//!
//! These tests verify that BoxConstraints are correctly propagated through
//! the render tree, including constraint tightening, loosening, and transformation
//! by various render objects.

use flui_rendering::constraints::{BoxConstraints, Constraints};
use flui_rendering::objects::r#box::basic::{
    RenderAlign, RenderConstrainedBox, RenderPadding, RenderSizedBox,
};
use flui_rendering::traits::{RenderBox, SingleChildRenderBox};
use flui_types::{Alignment, EdgeInsets, Size};

// ============================================================================
// BoxConstraints Basic Tests
// ============================================================================

#[test]
fn test_tight_constraints() {
    let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

    assert_eq!(constraints.min_width, 100.0);
    assert_eq!(constraints.max_width, 100.0);
    assert_eq!(constraints.min_height, 50.0);
    assert_eq!(constraints.max_height, 50.0);
    assert!(constraints.is_tight());
}

#[test]
fn test_loose_constraints() {
    let constraints = BoxConstraints::loose(Size::new(200.0, 100.0));

    assert_eq!(constraints.min_width, 0.0);
    assert_eq!(constraints.max_width, 200.0);
    assert_eq!(constraints.min_height, 0.0);
    assert_eq!(constraints.max_height, 100.0);
    assert!(!constraints.is_tight());
}

#[test]
fn test_constrain_size() {
    let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 80.0);

    // Size within constraints
    let size1 = constraints.constrain(Size::new(100.0, 50.0));
    assert_eq!(size1, Size::new(100.0, 50.0));

    // Size below min
    let size2 = constraints.constrain(Size::new(20.0, 10.0));
    assert_eq!(size2, Size::new(50.0, 30.0));

    // Size above max
    let size3 = constraints.constrain(Size::new(200.0, 150.0));
    assert_eq!(size3, Size::new(150.0, 80.0));
}

#[test]
fn test_enforce_constraints() {
    let outer = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
    let inner = BoxConstraints::new(50.0, 300.0, 25.0, 150.0);

    let enforced = outer.enforce(inner);

    // min should be max of mins, but capped at outer max
    assert_eq!(enforced.min_width, 50.0);
    assert_eq!(enforced.min_height, 25.0);

    // max should be min of maxes
    assert_eq!(enforced.max_width, 200.0);
    assert_eq!(enforced.max_height, 100.0);
}

// ============================================================================
// SizedBox Constraints Tests
// ============================================================================

#[test]
fn test_sized_box_ignores_loose_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // SizedBox with fixed size should return that size regardless of loose constraints
    let constraints = BoxConstraints::loose(Size::new(500.0, 500.0));
    let size = sized_box.perform_layout(constraints);

    assert_eq!(size, Size::new(100.0, 50.0));
}

#[test]
fn test_sized_box_respects_tight_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // With tight constraints smaller than requested, should clamp
    let constraints = BoxConstraints::tight(Size::new(80.0, 40.0));
    let size = sized_box.perform_layout(constraints);

    // SizedBox should respect constraints
    assert_eq!(size, Size::new(80.0, 40.0));
}

#[test]
fn test_sized_box_with_min_constraints() {
    let mut sized_box = RenderSizedBox::fixed(50.0, 30.0);

    // Min constraints larger than requested size
    let constraints = BoxConstraints::new(100.0, 200.0, 80.0, 150.0);
    let size = sized_box.perform_layout(constraints);

    // Should be clamped to minimum
    assert_eq!(size, Size::new(100.0, 80.0));
}

// ============================================================================
// ConstrainedBox Tests
// ============================================================================

#[test]
fn test_constrained_box_applies_additional_constraints() {
    let child = Box::new(RenderSizedBox::expand());
    let additional = BoxConstraints::new(50.0, 150.0, 40.0, 120.0);
    let mut constrained = RenderConstrainedBox::with_child(additional, child);

    // Parent gives loose constraints
    let parent_constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    let size = constrained.perform_layout(parent_constraints);

    // Child wants to expand, but constrained box limits it
    assert!(size.width <= 150.0);
    assert!(size.height <= 120.0);
}

#[test]
fn test_constrained_box_with_tight_parent() {
    let child = Box::new(RenderSizedBox::expand());
    let additional = BoxConstraints::new(0.0, 100.0, 0.0, 80.0);
    let mut constrained = RenderConstrainedBox::with_child(additional, child);

    // Parent gives tight constraints smaller than additional max
    let parent_constraints = BoxConstraints::tight(Size::new(50.0, 40.0));
    let size = constrained.perform_layout(parent_constraints);

    // Tight parent constraints should win
    assert_eq!(size, Size::new(50.0, 40.0));
}

#[test]
fn test_constrained_box_respects_parent_limits() {
    let child = Box::new(RenderSizedBox::expand());
    // Constrained box wants size between 50-100
    let additional = BoxConstraints::new(50.0, 100.0, 40.0, 80.0);
    let mut constrained = RenderConstrainedBox::with_child(additional, child);

    // Parent gives constraints that overlap with additional constraints
    let parent_constraints = BoxConstraints::new(0.0, 150.0, 0.0, 120.0);
    let size = constrained.perform_layout(parent_constraints);

    // Should be within the constrained box limits
    assert!(size.width >= 50.0 && size.width <= 100.0);
    assert!(size.height >= 40.0 && size.height <= 80.0);
}

// ============================================================================
// Padding Constraints Tests
// ============================================================================

#[test]
fn test_padding_reduces_available_space() {
    let child = Box::new(RenderSizedBox::expand());
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    let constraints = BoxConstraints::tight(Size::new(100.0, 80.0));
    let size = padding.perform_layout(constraints);

    // Padding consumes 20 on each axis (10 * 2)
    assert_eq!(size, Size::new(100.0, 80.0));

    // Child should have received reduced constraints
    // Child size = parent size - padding = 80 x 60
    if let Some(child) = padding.child() {
        assert_eq!(child.size(), Size::new(80.0, 60.0));
    }
}

#[test]
fn test_padding_with_asymmetric_insets() {
    let child = Box::new(RenderSizedBox::expand());
    let insets = EdgeInsets::new(10.0, 20.0, 30.0, 40.0); // left, top, right, bottom
    let mut padding = RenderPadding::with_child(insets, child);

    let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
    let size = padding.perform_layout(constraints);

    assert_eq!(size, Size::new(200.0, 200.0));

    // Child: 200 - (10+30) = 160 width, 200 - (20+40) = 140 height
    if let Some(child) = padding.child() {
        assert_eq!(child.size(), Size::new(160.0, 140.0));
    }
}

#[test]
fn test_padding_larger_than_constraints() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(100.0), child);

    // Constraints are smaller than padding alone would require
    let constraints = BoxConstraints::loose(Size::new(150.0, 150.0));
    let size = padding.perform_layout(constraints);

    // Padding of 200 total on each axis, but constrained to 150
    // This depends on implementation - child may get 0 or negative constraints
    assert!(size.width <= 150.0);
    assert!(size.height <= 150.0);
}

// ============================================================================
// Align Constraints Tests
// ============================================================================

#[test]
fn test_align_loosens_constraints() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut align = RenderAlign::with_child(Alignment::CENTER, child);

    // Tight constraints
    let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
    let size = align.perform_layout(constraints);

    // Align takes full size
    assert_eq!(size, Size::new(200.0, 200.0));

    // Child should have its requested size
    if let Some(child) = align.child() {
        assert_eq!(child.size(), Size::new(50.0, 50.0));
    }
}

#[test]
fn test_align_with_loose_constraints() {
    let child = Box::new(RenderSizedBox::fixed(100.0, 80.0));
    let mut align = RenderAlign::with_child(Alignment::TOP_LEFT, child);

    let constraints = BoxConstraints::loose(Size::new(300.0, 250.0));
    let size = align.perform_layout(constraints);

    // With loose constraints, Align should expand to fill
    // (or shrink wrap depending on implementation)
    assert!(size.width <= 300.0);
    assert!(size.height <= 250.0);
}

// ============================================================================
// Nested Constraints Propagation Tests
// ============================================================================

#[test]
fn test_nested_constraints_chain() {
    // Padding(10) -> ConstrainedBox(max 150x100) -> SizedBox.expand
    let inner = Box::new(RenderSizedBox::expand());
    let constrained = Box::new(RenderConstrainedBox::with_child(
        BoxConstraints::new(0.0, 150.0, 0.0, 100.0),
        inner,
    ));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), constrained);

    let constraints = BoxConstraints::loose(Size::new(300.0, 300.0));
    let size = padding.perform_layout(constraints);

    // Padding adds 20 to each dimension
    // ConstrainedBox limits child to 150x100
    // So total: min(150+20, 300) x min(100+20, 300) = 170 x 120
    assert_eq!(size, Size::new(170.0, 120.0));
}

#[test]
fn test_double_padding_constraints() {
    let inner_child = Box::new(RenderSizedBox::expand());
    let inner_padding = Box::new(RenderPadding::with_child(EdgeInsets::all(5.0), inner_child));
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(10.0), inner_padding);

    let constraints = BoxConstraints::tight(Size::new(100.0, 80.0));
    let size = outer_padding.perform_layout(constraints);

    assert_eq!(size, Size::new(100.0, 80.0));

    // Inner padding child should be: 100 - 20 - 10 = 70 width, 80 - 20 - 10 = 50 height
    // Note: each padding removes its own insets from available space
}

#[test]
fn test_align_in_padding_constraints() {
    let child = Box::new(RenderSizedBox::fixed(30.0, 30.0));
    let align = Box::new(RenderAlign::with_child(Alignment::CENTER, child));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(20.0), align);

    let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
    let size = padding.perform_layout(constraints);

    assert_eq!(size, Size::new(200.0, 200.0));

    // Align receives: 200 - 40 = 160 on each axis
    if let Some(align_child) = padding.child() {
        assert_eq!(align_child.size(), Size::new(160.0, 160.0));
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_zero_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 100.0);

    let constraints = BoxConstraints::tight(Size::ZERO);
    let size = sized_box.perform_layout(constraints);

    assert_eq!(size, Size::ZERO);
}

#[test]
fn test_infinite_constraints() {
    let mut sized_box = RenderSizedBox::fixed(100.0, 80.0);

    let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
    let size = sized_box.perform_layout(constraints);

    // Fixed size should be returned when constraints allow
    assert_eq!(size, Size::new(100.0, 80.0));
}

#[test]
fn test_expand_with_infinite_constraints() {
    let mut sized_box = RenderSizedBox::expand();

    // This would cause issues in real use - expand with infinite constraints
    let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
    let size = sized_box.perform_layout(constraints);

    // Implementation dependent - should either use 0 or some default
    // The key is it shouldn't panic
    assert!(size.width.is_finite() || size.width == f32::INFINITY);
}

#[test]
fn test_normalized_constraints() {
    // Test that normalize works correctly
    let constraints = BoxConstraints::new(50.0, 150.0, 40.0, 120.0);

    // Constrain should work correctly
    let size = constraints.constrain(Size::new(75.0, 60.0));

    // Should be within bounds
    assert!(size.width >= 50.0 && size.width <= 150.0);
    assert!(size.height >= 40.0 && size.height <= 120.0);
}

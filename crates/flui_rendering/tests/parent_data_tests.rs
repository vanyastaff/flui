//! Integration tests for parent data system.
//!
//! These tests verify that parent data is correctly propagated through the render tree,
//! including offset handling, layout positioning, and flex factors.

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::objects::r#box::basic::{RenderAlign, RenderPadding, RenderSizedBox};
use flui_rendering::parent_data::BoxParentData;
use flui_rendering::traits::{get_child_offset, set_child_offset, RenderBox, SingleChildRenderBox};
use flui_types::{Alignment, EdgeInsets, Offset, Size};

// ============================================================================
// Helper Functions
// ============================================================================

fn layout_render_box(render_box: &mut dyn RenderBox, max_size: Size) -> Size {
    let constraints = BoxConstraints::loose(max_size);
    render_box.perform_layout(constraints)
}

// ============================================================================
// BoxParentData Tests
// ============================================================================

#[test]
fn test_box_parent_data_default() {
    let parent_data = BoxParentData::default();
    assert_eq!(parent_data.offset, Offset::ZERO);
}

#[test]
fn test_box_parent_data_offset() {
    let mut parent_data = BoxParentData::default();
    parent_data.offset = Offset::new(10.0, 20.0);
    assert_eq!(parent_data.offset.dx, 10.0);
    assert_eq!(parent_data.offset.dy, 20.0);
}

// ============================================================================
// Child Offset Helper Tests
// ============================================================================

#[test]
fn test_set_and_get_child_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    // Layout first to ensure child has parent data
    layout_render_box(&mut padding, Size::new(200.0, 200.0));

    // Get the child and check offset
    if let Some(child) = padding.child() {
        let offset = get_child_offset(child);
        // Padding of 10 on all sides means child is at (10, 10)
        assert_eq!(offset.dx, 10.0, "Child should be offset by left padding");
        assert_eq!(offset.dy, 10.0, "Child should be offset by top padding");
    } else {
        panic!("Padding should have a child");
    }
}

#[test]
fn test_asymmetric_padding_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let padding = EdgeInsets::new(10.0, 20.0, 30.0, 40.0); // left, top, right, bottom
    let mut padded = RenderPadding::with_child(padding, child);

    layout_render_box(&mut padded, Size::new(200.0, 200.0));

    if let Some(child) = padded.child() {
        let offset = get_child_offset(child);
        assert_eq!(offset.dx, 10.0, "Child x should be left padding");
        assert_eq!(offset.dy, 20.0, "Child y should be top padding");
    }
}

// ============================================================================
// Align Parent Data Tests
// ============================================================================

#[test]
fn test_align_center_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut align = RenderAlign::with_child(Alignment::CENTER, child);

    // Layout with larger constraints
    let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
    align.perform_layout(constraints);

    // Child should be centered: (200-50)/2 = 75
    if let Some(child) = align.child() {
        let offset = get_child_offset(child);
        assert_eq!(offset.dx, 75.0, "Child should be horizontally centered");
        assert_eq!(offset.dy, 75.0, "Child should be vertically centered");
    }
}

#[test]
fn test_align_top_left_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut align = RenderAlign::with_child(Alignment::TOP_LEFT, child);

    let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
    align.perform_layout(constraints);

    if let Some(child) = align.child() {
        let offset = get_child_offset(child);
        assert_eq!(offset.dx, 0.0, "Child should be at left edge");
        assert_eq!(offset.dy, 0.0, "Child should be at top edge");
    }
}

#[test]
fn test_align_bottom_right_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut align = RenderAlign::with_child(Alignment::BOTTOM_RIGHT, child);

    let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
    align.perform_layout(constraints);

    if let Some(child) = align.child() {
        let offset = get_child_offset(child);
        // Child at bottom right: 200 - 50 = 150
        assert_eq!(offset.dx, 150.0, "Child should be at right edge");
        assert_eq!(offset.dy, 150.0, "Child should be at bottom edge");
    }
}

#[test]
fn test_align_custom_alignment() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    // Alignment(0.5, -0.5) = center-ish horizontally, top-ish vertically
    let alignment = Alignment::new(0.5, -0.5);
    let mut align = RenderAlign::with_child(alignment, child);

    let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
    align.perform_layout(constraints);

    if let Some(child) = align.child() {
        let offset = get_child_offset(child);
        // x: (200-50) * (0.5 + 1) / 2 = 150 * 0.75 = 112.5
        // y: (200-50) * (-0.5 + 1) / 2 = 150 * 0.25 = 37.5
        assert!(
            (offset.dx - 112.5).abs() < 0.01,
            "Child x offset should be ~112.5, got {}",
            offset.dx
        );
        assert!(
            (offset.dy - 37.5).abs() < 0.01,
            "Child y offset should be ~37.5, got {}",
            offset.dy
        );
    }
}

// ============================================================================
// Nested Layout Offset Tests
// ============================================================================

#[test]
fn test_nested_padding_cumulative_offset() {
    // Inner child
    let inner_child = Box::new(RenderSizedBox::fixed(50.0, 50.0));

    // Inner padding: 5 on all sides
    let inner_padding = Box::new(RenderPadding::with_child(EdgeInsets::all(5.0), inner_child));

    // Outer padding: 10 on all sides
    let mut outer_padding = RenderPadding::with_child(EdgeInsets::all(10.0), inner_padding);

    layout_render_box(&mut outer_padding, Size::new(200.0, 200.0));

    // Check outer padding's child offset (should be at 10, 10)
    if let Some(inner) = outer_padding.child() {
        let outer_offset = get_child_offset(inner);
        assert_eq!(outer_offset, Offset::new(10.0, 10.0));

        // Check inner padding's child offset (should be at 5, 5 relative to inner)
        // But we can't easily access the inner child's offset here without downcasting
        // This test verifies the outer offset is correct
    }
}

#[test]
fn test_align_inside_padding_offset() {
    // Child at 50x50
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));

    // Align child to center
    let align = Box::new(RenderAlign::with_child(Alignment::CENTER, child));

    // Wrap with padding
    let mut padding = RenderPadding::with_child(EdgeInsets::all(20.0), align);

    layout_render_box(&mut padding, Size::new(200.0, 200.0));

    // Padding child (the Align) should be at offset (20, 20)
    if let Some(align_box) = padding.child() {
        let align_offset = get_child_offset(align_box);
        assert_eq!(align_offset, Offset::new(20.0, 20.0));
    }
}

// ============================================================================
// Layout Size vs Offset Tests
// ============================================================================

#[test]
fn test_size_independent_of_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    let size = layout_render_box(&mut padding, Size::new(200.0, 200.0));

    // Padding size should be child size + padding = 50 + 20 = 70
    assert_eq!(size.width, 70.0);
    assert_eq!(size.height, 70.0);

    // Child offset doesn't affect parent size
    if let Some(child) = padding.child() {
        let offset = get_child_offset(child);
        assert_eq!(offset, Offset::new(10.0, 10.0));
        // Child size is still 50x50
        assert_eq!(child.size().width, 50.0);
        assert_eq!(child.size().height, 50.0);
    }
}

// ============================================================================
// Mutable Offset Tests
// ============================================================================

#[test]
fn test_set_child_offset_manual() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(10.0), child);

    // Layout to set up parent data
    layout_render_box(&mut padding, Size::new(200.0, 200.0));

    // Get child mutably and change offset
    if let Some(child) = padding.child_mut() {
        set_child_offset(child, Offset::new(100.0, 100.0));

        let new_offset = get_child_offset(child);
        assert_eq!(new_offset, Offset::new(100.0, 100.0));
    }
}

// ============================================================================
// Zero and Edge Cases
// ============================================================================

#[test]
fn test_zero_padding_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(0.0), child);

    layout_render_box(&mut padding, Size::new(200.0, 200.0));

    if let Some(child) = padding.child() {
        let offset = get_child_offset(child);
        assert_eq!(offset, Offset::ZERO);
    }
}

#[test]
fn test_large_padding_offset() {
    let child = Box::new(RenderSizedBox::fixed(50.0, 50.0));
    let mut padding = RenderPadding::with_child(EdgeInsets::all(1000.0), child);

    layout_render_box(&mut padding, Size::new(3000.0, 3000.0));

    if let Some(child) = padding.child() {
        let offset = get_child_offset(child);
        assert_eq!(offset, Offset::new(1000.0, 1000.0));
    }
}

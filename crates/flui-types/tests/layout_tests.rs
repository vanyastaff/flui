//! Comprehensive layout system tests
//!
//! This test suite validates the layout primitives including:
//! - BoxConstraints (tight, loose, tightFor, enforce)
//! - Edges (inflate, deflate, validation)
//! - Alignment (constants, lerp, arithmetic)
//! - Axis utilities (perpendicular, direction)

use flui_types::geometry::{px, Edges, Offset, Point, Rect, Size};
use flui_types::layout::{
    Alignment, Axis, AxisDirection, BoxConstraints, CrossAxisAlignment, MainAxisAlignment,
    MainAxisSize, Orientation,
};

// ============================================================================
// BoxConstraints Tests (10 tests)
// ============================================================================

#[test]
fn test_box_constraints_tight() {
    let size = Size::new(px(100.0), px(200.0));
    let constraints = BoxConstraints::tight(size);

    assert_eq!(constraints.min_width, px(100.0));
    assert_eq!(constraints.max_width, px(100.0));
    assert_eq!(constraints.min_height, px(200.0));
    assert_eq!(constraints.max_height, px(200.0));
    assert!(constraints.is_tight());
}

#[test]
fn test_box_constraints_loose() {
    let size = Size::new(px(100.0), px(200.0));
    let constraints = BoxConstraints::loose(size);

    assert_eq!(constraints.min_width, px(0.0));
    assert_eq!(constraints.max_width, px(100.0));
    assert_eq!(constraints.min_height, px(0.0));
    assert_eq!(constraints.max_height, px(200.0));
    assert!(!constraints.is_tight());
}

#[test]
fn test_box_constraints_tight_width() {
    let constraints = BoxConstraints::tight_width(px(100.0));

    assert_eq!(constraints.min_width, px(100.0));
    assert_eq!(constraints.max_width, px(100.0));
    assert_eq!(constraints.min_height, px(0.0));
    assert_eq!(constraints.max_height, px(f32::MAX));
}

#[test]
fn test_box_constraints_tight_height() {
    let constraints = BoxConstraints::tight_height(px(200.0));

    assert_eq!(constraints.min_width, px(0.0));
    assert_eq!(constraints.max_width, px(f32::MAX));
    assert_eq!(constraints.min_height, px(200.0));
    assert_eq!(constraints.max_height, px(200.0));
}

#[test]
fn test_box_constraints_constrain() {
    let constraints = BoxConstraints::new(px(50.0), px(150.0), px(50.0), px(150.0));

    // Within bounds
    let size1 = constraints.constrain(Size::new(px(100.0), px(100.0)));
    assert_eq!(size1, Size::new(px(100.0), px(100.0)));

    // Too small - should clamp to min
    let size2 = constraints.constrain(Size::new(px(10.0), px(10.0)));
    assert_eq!(size2, Size::new(px(50.0), px(50.0)));

    // Too large - should clamp to max
    let size3 = constraints.constrain(Size::new(px(200.0), px(200.0)));
    assert_eq!(size3, Size::new(px(150.0), px(150.0)));
}

#[test]
fn test_box_constraints_constrain_width() {
    let constraints = BoxConstraints::new(px(50.0), px(150.0), px(50.0), px(150.0));

    assert_eq!(constraints.constrain_width(px(100.0)), px(100.0));
    assert_eq!(constraints.constrain_width(px(10.0)), px(50.0));
    assert_eq!(constraints.constrain_width(px(200.0)), px(150.0));
}

#[test]
fn test_box_constraints_constrain_height() {
    let constraints = BoxConstraints::new(px(50.0), px(150.0), px(50.0), px(150.0));

    assert_eq!(constraints.constrain_height(px(100.0)), px(100.0));
    assert_eq!(constraints.constrain_height(px(10.0)), px(50.0));
    assert_eq!(constraints.constrain_height(px(200.0)), px(150.0));
}

#[test]
fn test_box_constraints_enforce() {
    let _constraints = BoxConstraints::new(px(50.0), px(150.0), px(50.0), px(150.0));
    let other = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));

    let enforced = other.enforce();

    // enforce() ensures min <= max
    assert!(enforced.max_width >= enforced.min_width);
    assert!(enforced.max_height >= enforced.min_height);
}

#[test]
fn test_box_constraints_deflate() {
    let constraints = BoxConstraints::new(px(100.0), px(200.0), px(100.0), px(200.0));
    let insets = Edges::all(px(10.0));

    let deflated = constraints.deflate(insets);

    // Should shrink by 20 (10 on each side)
    assert_eq!(deflated.min_width, px(80.0));
    assert_eq!(deflated.max_width, px(180.0));
    assert_eq!(deflated.min_height, px(80.0));
    assert_eq!(deflated.max_height, px(180.0));
}

#[test]
fn test_box_constraints_has_bounded_dimensions() {
    let bounded = BoxConstraints::new(px(50.0), px(150.0), px(50.0), px(150.0));
    assert!(bounded.has_bounded_width());
    assert!(bounded.has_bounded_height());

    let unbounded = BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(f32::INFINITY));
    assert!(!unbounded.has_bounded_width());
    assert!(!unbounded.has_bounded_height());
}

// ============================================================================
// Edges<Pixels> Tests (10 tests)
// ============================================================================

#[test]
fn test_edges_constructors() {
    let all = Edges::all(px(10.0));
    assert_eq!(all.top, px(10.0));
    assert_eq!(all.right, px(10.0));
    assert_eq!(all.bottom, px(10.0));
    assert_eq!(all.left, px(10.0));

    let symmetric = Edges::symmetric(px(5.0), px(10.0));
    assert_eq!(symmetric.top, px(5.0));
    assert_eq!(symmetric.right, px(10.0));
    assert_eq!(symmetric.bottom, px(5.0));
    assert_eq!(symmetric.left, px(10.0));
}

#[test]
fn test_edges_only_constructors() {
    let left = Edges::only_left(px(10.0));
    assert_eq!(left.left, px(10.0));
    assert_eq!(left.top, px(0.0));
    assert_eq!(left.right, px(0.0));
    assert_eq!(left.bottom, px(0.0));

    let top = Edges::only_top(px(20.0));
    assert_eq!(top.top, px(20.0));
    assert_eq!(top.left, px(0.0));
}

#[test]
fn test_edges_inflate_rect() {
    let insets = Edges::all(px(10.0));
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));

    let inflated = insets.inflate_rect(rect);

    assert_eq!(inflated.left(), px(-10.0));
    assert_eq!(inflated.top(), px(-10.0));
    assert_eq!(inflated.right(), px(110.0));
    assert_eq!(inflated.bottom(), px(110.0));
}

#[test]
fn test_edges_deflate_rect() {
    let insets = Edges::all(px(10.0));
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));

    let deflated = insets.deflate_rect(rect);

    assert_eq!(deflated.left(), px(10.0));
    assert_eq!(deflated.top(), px(10.0));
    assert_eq!(deflated.right(), px(90.0));
    assert_eq!(deflated.bottom(), px(90.0));
}

#[test]
fn test_edges_inflate_size() {
    let insets = Edges::symmetric(px(5.0), px(10.0));
    let size = Size::new(px(100.0), px(100.0));

    let inflated = insets.inflate_size(size);

    // horizontal: left(10) + right(10) = 20
    // vertical: top(5) + bottom(5) = 10
    assert_eq!(inflated.width, px(120.0));
    assert_eq!(inflated.height, px(110.0));
}

#[test]
fn test_edges_deflate_size() {
    let insets = Edges::symmetric(px(5.0), px(10.0));
    let size = Size::new(px(100.0), px(100.0));

    let deflated = insets.deflate_size(size);

    assert_eq!(deflated.width, px(80.0));
    assert_eq!(deflated.height, px(90.0));
}

#[test]
fn test_edges_flip_horizontal() {
    let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));
    let flipped = insets.flip_horizontal();

    assert_eq!(flipped.left, px(20.0));
    assert_eq!(flipped.right, px(40.0));
    assert_eq!(flipped.top, px(10.0));
    assert_eq!(flipped.bottom, px(30.0));
}

#[test]
fn test_edges_flip_vertical() {
    let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));
    let flipped = insets.flip_vertical();

    assert_eq!(flipped.top, px(30.0));
    assert_eq!(flipped.bottom, px(10.0));
    assert_eq!(flipped.left, px(40.0));
    assert_eq!(flipped.right, px(20.0));
}

#[test]
fn test_edges_totals() {
    let insets = Edges::new(px(10.0), px(20.0), px(30.0), px(40.0));

    assert_eq!(insets.horizontal_total(), px(60.0)); // left(40) + right(20)
    assert_eq!(insets.vertical_total(), px(40.0)); // top(10) + bottom(30)
}

#[test]
fn test_edges_validation() {
    let zero = Edges::<flui_types::geometry::Pixels>::ZERO;
    assert!(zero.is_zero());

    let positive = Edges::all(px(10.0));
    assert!(!positive.is_zero());
    assert!(positive.is_non_negative());

    let negative = Edges::new(px(-5.0), px(10.0), px(-3.0), px(20.0));
    assert!(!negative.is_non_negative());

    let clamped = negative.clamp_non_negative();
    assert_eq!(clamped.top, px(0.0));
    assert_eq!(clamped.bottom, px(0.0));
    assert!(clamped.is_non_negative());
}

// ============================================================================
// Alignment Tests (6 tests)
// ============================================================================

#[test]
fn test_alignment_constants() {
    assert_eq!(Alignment::TOP_LEFT.x, -1.0);
    assert_eq!(Alignment::TOP_LEFT.y, -1.0);

    assert_eq!(Alignment::CENTER.x, 0.0);
    assert_eq!(Alignment::CENTER.y, 0.0);

    assert_eq!(Alignment::BOTTOM_RIGHT.x, 1.0);
    assert_eq!(Alignment::BOTTOM_RIGHT.y, 1.0);
}

#[test]
fn test_alignment_coordinates() {
    // Alignment uses [-1, 1] coordinates
    // -1 = left/top, 0 = center, 1 = right/bottom

    // Test that coordinates map correctly
    let top_left = Alignment::TOP_LEFT;
    assert!(top_left.x < 0.0);
    assert!(top_left.y < 0.0);

    let center = Alignment::CENTER;
    assert_eq!(center.x, 0.0);
    assert_eq!(center.y, 0.0);

    let bottom_right = Alignment::BOTTOM_RIGHT;
    assert!(bottom_right.x > 0.0);
    assert!(bottom_right.y > 0.0);
}

#[test]
fn test_alignment_lerp() {
    let a1 = Alignment::TOP_LEFT;
    let a2 = Alignment::BOTTOM_RIGHT;

    let mid = Alignment::lerp(a1, a2, 0.5);
    assert_eq!(mid, Alignment::CENTER);

    let quarter = Alignment::lerp(a1, a2, 0.25);
    assert_eq!(quarter.x, -0.5);
    assert_eq!(quarter.y, -0.5);
}

#[test]
fn test_alignment_addition() {
    let a1 = Alignment::new(0.5, 0.5);
    let a2 = Alignment::new(0.25, -0.25);

    let sum = a1 + a2;
    assert_eq!(sum.x, 0.75);
    assert_eq!(sum.y, 0.25);
}

#[test]
fn test_alignment_negation() {
    let alignment = Alignment::new(0.5, -0.5);
    let negated = -alignment;

    assert_eq!(negated.x, -0.5);
    assert_eq!(negated.y, 0.5);
}

#[test]
fn test_alignment_construction() {
    // Test custom alignment construction
    let custom = Alignment::new(0.5, -0.75);
    assert_eq!(custom.x, 0.5);
    assert_eq!(custom.y, -0.75);

    // Test from tuple
    let from_tuple: Alignment = (0.5, -0.75).into();
    assert_eq!(from_tuple.x, 0.5);
    assert_eq!(from_tuple.y, -0.75);
}

// ============================================================================
// Axis and Orientation Tests (5 tests)
// ============================================================================

#[test]
fn test_axis_opposite() {
    assert_eq!(Axis::Horizontal.opposite(), Axis::Vertical);
    assert_eq!(Axis::Vertical.opposite(), Axis::Horizontal);
}

#[test]
fn test_axis_direction_axis() {
    assert_eq!(AxisDirection::LeftToRight.axis(), Axis::Horizontal);
    assert_eq!(AxisDirection::RightToLeft.axis(), Axis::Horizontal);
    assert_eq!(AxisDirection::TopToBottom.axis(), Axis::Vertical);
    assert_eq!(AxisDirection::BottomToTop.axis(), Axis::Vertical);
}

#[test]
fn test_axis_direction_opposite() {
    assert_eq!(
        AxisDirection::LeftToRight.opposite(),
        AxisDirection::RightToLeft
    );
    assert_eq!(
        AxisDirection::RightToLeft.opposite(),
        AxisDirection::LeftToRight
    );
    assert_eq!(
        AxisDirection::TopToBottom.opposite(),
        AxisDirection::BottomToTop
    );
    assert_eq!(
        AxisDirection::BottomToTop.opposite(),
        AxisDirection::TopToBottom
    );
}

#[test]
fn test_orientation_values() {
    // Verify Orientation enum variants exist
    let portrait = Orientation::Portrait;
    let landscape = Orientation::Landscape;

    assert_ne!(portrait, landscape);
}

#[test]
fn test_axis_orientation_relationship() {
    // Test the relationship between Axis and Orientation conceptually
    // Horizontal axis typically corresponds to landscape
    // Vertical axis typically corresponds to portrait
    let h_axis = Axis::Horizontal;
    let v_axis = Axis::Vertical;

    assert_ne!(h_axis, v_axis);
    assert_eq!(h_axis.opposite(), v_axis);
}

// ============================================================================
// Cross-Axis and Main-Axis Alignment Tests (2 tests)
// ============================================================================

#[test]
fn test_cross_axis_alignment_values() {
    // Just verify the enum variants exist and can be constructed
    let alignments = [
        CrossAxisAlignment::Start,
        CrossAxisAlignment::End,
        CrossAxisAlignment::Center,
        CrossAxisAlignment::Stretch,
        CrossAxisAlignment::Baseline,
    ];

    assert_eq!(alignments.len(), 5);
}

#[test]
fn test_main_axis_alignment_and_size() {
    // Verify enum variants exist
    let alignments = [
        MainAxisAlignment::Start,
        MainAxisAlignment::End,
        MainAxisAlignment::Center,
        MainAxisAlignment::SpaceBetween,
        MainAxisAlignment::SpaceAround,
        MainAxisAlignment::SpaceEvenly,
    ];

    let sizes = [MainAxisSize::Min, MainAxisSize::Max];

    assert_eq!(alignments.len(), 6);
    assert_eq!(sizes.len(), 2);
}

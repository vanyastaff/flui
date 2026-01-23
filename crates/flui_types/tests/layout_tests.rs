//! Comprehensive integration tests for layout types

use flui_types::layout::{BoxConstraints, EdgeInsets, Alignment};
use flui_types::geometry::{px, Size, Offset, Rect, Pixels};

// ============================================================================
// BoxConstraints Tests (15 tests)
// ============================================================================

#[test]
fn test_box_constraints_new() {
    let constraints = BoxConstraints::new(px(10.0), px(100.0), px(20.0), px(200.0));
    assert_eq!(constraints.min_width, px(10.0));
    assert_eq!(constraints.max_width, px(100.0));
    assert_eq!(constraints.min_height, px(20.0));
    assert_eq!(constraints.max_height, px(200.0));
}

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
    let constraints = BoxConstraints::tight_width(px(50.0));
    assert_eq!(constraints.min_width, px(50.0));
    assert_eq!(constraints.max_width, px(50.0));
    assert_eq!(constraints.min_height, px(0.0));
    assert_eq!(constraints.max_height, Pixels::MAX);
}

#[test]
fn test_box_constraints_tight_height() {
    let constraints = BoxConstraints::tight_height(px(100.0));
    assert_eq!(constraints.min_width, px(0.0));
    assert_eq!(constraints.max_width, Pixels::MAX);
    assert_eq!(constraints.min_height, px(100.0));
    assert_eq!(constraints.max_height, px(100.0));
}

#[test]
fn test_box_constraints_unbounded() {
    let constraints = BoxConstraints::unbounded();
    assert_eq!(constraints.min_width, px(0.0));
    assert_eq!(constraints.max_width, Pixels::MAX);
    assert_eq!(constraints.min_height, px(0.0));
    assert_eq!(constraints.max_height, Pixels::MAX);
    assert!(!constraints.is_tight());
}

#[test]
fn test_box_constraints_expand() {
    let constraints = BoxConstraints::expand();
    assert_eq!(constraints.min_width, Pixels::MAX);
    assert_eq!(constraints.max_width, Pixels::MAX);
    assert_eq!(constraints.min_height, Pixels::MAX);
    assert_eq!(constraints.max_height, Pixels::MAX);
}

#[test]
fn test_box_constraints_constrain_within_bounds() {
    let constraints = BoxConstraints::new(px(50.0), px(150.0), px(100.0), px(300.0));
    let size = Size::new(px(100.0), px(200.0));
    let constrained = constraints.constrain(size);

    assert_eq!(constrained, size); // Size is within bounds
}

#[test]
fn test_box_constraints_constrain_too_small() {
    let constraints = BoxConstraints::new(px(50.0), px(150.0), px(100.0), px(300.0));
    let size = Size::new(px(10.0), px(50.0));
    let constrained = constraints.constrain(size);

    assert_eq!(constrained.width, px(50.0)); // Clamped to min
    assert_eq!(constrained.height, px(100.0)); // Clamped to min
}

#[test]
fn test_box_constraints_constrain_too_large() {
    let constraints = BoxConstraints::new(px(50.0), px(150.0), px(100.0), px(300.0));
    let size = Size::new(px(200.0), px(400.0));
    let constrained = constraints.constrain(size);

    assert_eq!(constrained.width, px(150.0)); // Clamped to max
    assert_eq!(constrained.height, px(300.0)); // Clamped to max
}

#[test]
fn test_box_constraints_has_bounded_width() {
    let bounded = BoxConstraints::new(px(0.0), px(100.0), px(0.0), Pixels::MAX);
    assert!(bounded.has_bounded_width());

    let unbounded = BoxConstraints::unbounded();
    assert!(!unbounded.has_bounded_width());
}

#[test]
fn test_box_constraints_has_bounded_height() {
    let bounded = BoxConstraints::new(px(0.0), Pixels::MAX, px(0.0), px(100.0));
    assert!(bounded.has_bounded_height());

    let unbounded = BoxConstraints::unbounded();
    assert!(!unbounded.has_bounded_height());
}

#[test]
fn test_box_constraints_has_infinite_width() {
    let infinite = BoxConstraints::unbounded();
    assert!(infinite.has_infinite_width());

    let finite = BoxConstraints::loose(Size::new(px(100.0), px(100.0)));
    assert!(!finite.has_infinite_width());
}

#[test]
fn test_box_constraints_has_infinite_height() {
    let infinite = BoxConstraints::unbounded();
    assert!(infinite.has_infinite_height());

    let finite = BoxConstraints::loose(Size::new(px(100.0), px(100.0)));
    assert!(!finite.has_infinite_height());
}

#[test]
fn test_box_constraints_biggest() {
    let constraints = BoxConstraints::new(px(50.0), px(150.0), px(100.0), px(300.0));
    let biggest = constraints.biggest();

    assert_eq!(biggest.width, px(150.0));
    assert_eq!(biggest.height, px(300.0));
}

// ============================================================================
// EdgeInsets Tests (10 tests)
// ============================================================================

#[test]
fn test_edge_insets_all() {
    let insets = EdgeInsets::all(px(10.0));
    assert_eq!(insets.left, px(10.0));
    assert_eq!(insets.right, px(10.0));
    assert_eq!(insets.top, px(10.0));
    assert_eq!(insets.bottom, px(10.0));
}

#[test]
fn test_edge_insets_symmetric() {
    let insets = EdgeInsets::symmetric(px(20.0), px(30.0));
    assert_eq!(insets.left, px(20.0));
    assert_eq!(insets.right, px(20.0));
    assert_eq!(insets.top, px(30.0));
    assert_eq!(insets.bottom, px(30.0));
}

#[test]
fn test_edge_insets_only() {
    let insets = EdgeInsets::only(px(5.0), px(10.0), px(15.0), px(20.0));
    assert_eq!(insets.left, px(5.0));
    assert_eq!(insets.top, px(10.0));
    assert_eq!(insets.right, px(15.0));
    assert_eq!(insets.bottom, px(20.0));
}

#[test]
fn test_edge_insets_zero() {
    let insets = EdgeInsets::zero();
    assert_eq!(insets.left, px(0.0));
    assert_eq!(insets.right, px(0.0));
    assert_eq!(insets.top, px(0.0));
    assert_eq!(insets.bottom, px(0.0));
}

#[test]
fn test_edge_insets_horizontal() {
    let insets = EdgeInsets::symmetric(px(15.0), px(25.0));
    assert_eq!(insets.horizontal(), px(30.0)); // left + right
}

#[test]
fn test_edge_insets_vertical() {
    let insets = EdgeInsets::symmetric(px(15.0), px(25.0));
    assert_eq!(insets.vertical(), px(50.0)); // top + bottom
}

#[test]
fn test_edge_insets_deflate_size() {
    let insets = EdgeInsets::all(px(10.0));
    let size = Size::new(px(100.0), px(100.0));
    let deflated = insets.deflate_size(size);

    assert_eq!(deflated.width, px(80.0)); // 100 - 10 - 10
    assert_eq!(deflated.height, px(80.0)); // 100 - 10 - 10
}

#[test]
fn test_edge_insets_inflate_size() {
    let insets = EdgeInsets::all(px(10.0));
    let size = Size::new(px(100.0), px(100.0));
    let inflated = insets.inflate_size(size);

    assert_eq!(inflated.width, px(120.0)); // 100 + 10 + 10
    assert_eq!(inflated.height, px(120.0)); // 100 + 10 + 10
}

#[test]
fn test_edge_insets_deflate_rect() {
    let insets = EdgeInsets::all(px(10.0));
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let deflated = insets.deflate_rect(rect);

    assert_eq!(deflated.left(), px(10.0));
    assert_eq!(deflated.top(), px(10.0));
    assert_eq!(deflated.right(), px(90.0));
    assert_eq!(deflated.bottom(), px(90.0));
}

#[test]
fn test_edge_insets_inflate_rect() {
    let insets = EdgeInsets::all(px(10.0));
    let rect = Rect::from_ltrb(px(10.0), px(10.0), px(90.0), px(90.0));
    let inflated = insets.inflate_rect(rect);

    assert_eq!(inflated.left(), px(0.0));
    assert_eq!(inflated.top(), px(0.0));
    assert_eq!(inflated.right(), px(100.0));
    assert_eq!(inflated.bottom(), px(100.0));
}

//! Edge insets (padding/margins) tests for Phase 8 (User Story 6)
//!
//! Tests for Edges type (EdgeInsets equivalent):
//! - Construction methods
//! - Rect inset operations
//! - Padding and margin applications

use flui_types::geometry::{px, Edges, Rect, Size};

// ============================================================================
// T075: Edges::all() and Edges::symmetric()
// ============================================================================

#[test]
fn test_edges_all() {
    let padding = Edges::all(px(10.0));

    assert_eq!(padding.top, px(10.0));
    assert_eq!(padding.right, px(10.0));
    assert_eq!(padding.bottom, px(10.0));
    assert_eq!(padding.left, px(10.0));
}

#[test]
fn test_edges_all_zero() {
    let no_padding = Edges::all(px(0.0));

    assert_eq!(no_padding.top, px(0.0));
    assert_eq!(no_padding.right, px(0.0));
    assert_eq!(no_padding.bottom, px(0.0));
    assert_eq!(no_padding.left, px(0.0));
}

#[test]
fn test_edges_symmetric() {
    let padding = Edges::symmetric(px(20.0), px(10.0));

    // Vertical: top and bottom
    assert_eq!(padding.top, px(20.0));
    assert_eq!(padding.bottom, px(20.0));

    // Horizontal: left and right
    assert_eq!(padding.left, px(10.0));
    assert_eq!(padding.right, px(10.0));
}

#[test]
fn test_edges_symmetric_vertical_only() {
    let padding = Edges::symmetric(px(15.0), px(0.0));

    assert_eq!(padding.top, px(15.0));
    assert_eq!(padding.bottom, px(15.0));
    assert_eq!(padding.left, px(0.0));
    assert_eq!(padding.right, px(0.0));
}

#[test]
fn test_edges_symmetric_horizontal_only() {
    let padding = Edges::symmetric(px(0.0), px(25.0));

    assert_eq!(padding.top, px(0.0));
    assert_eq!(padding.bottom, px(0.0));
    assert_eq!(padding.left, px(25.0));
    assert_eq!(padding.right, px(25.0));
}

// ============================================================================
// T076: Edges::horizontal() and Edges::vertical()
// ============================================================================

#[test]
fn test_edges_horizontal() {
    let padding = Edges::horizontal(px(10.0));

    assert_eq!(padding.left, px(10.0));
    assert_eq!(padding.right, px(10.0));
    assert_eq!(padding.top, px(0.0));
    assert_eq!(padding.bottom, px(0.0));
}

#[test]
fn test_edges_vertical() {
    let padding = Edges::vertical(px(15.0));

    assert_eq!(padding.top, px(15.0));
    assert_eq!(padding.bottom, px(15.0));
    assert_eq!(padding.left, px(0.0));
    assert_eq!(padding.right, px(0.0));
}

#[test]
fn test_edges_horizontal_total() {
    let padding = Edges::horizontal(px(10.0));
    let total = padding.horizontal_total();

    assert_eq!(total, px(20.0)); // left + right
}

#[test]
fn test_edges_vertical_total() {
    let padding = Edges::vertical(px(15.0));
    let total = padding.vertical_total();

    assert_eq!(total, px(30.0)); // top + bottom
}

// ============================================================================
// T077: Rect inset operations with Edges
// ============================================================================

#[test]
fn test_edges_deflate_rect_uniform() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let padding = Edges::all(px(10.0));

    let content = padding.deflate_rect(rect);

    // Should shrink by padding on all sides
    assert_eq!(content.left(), px(10.0));
    assert_eq!(content.top(), px(10.0));
    assert_eq!(content.right(), px(90.0));
    assert_eq!(content.bottom(), px(90.0));
    assert_eq!(content.width(), px(80.0));
    assert_eq!(content.height(), px(80.0));
}

#[test]
fn test_edges_deflate_rect_asymmetric() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(150.0));
    let padding = Edges::new(px(10.0), px(20.0), px(15.0), px(25.0));

    let content = padding.deflate_rect(rect);

    assert_eq!(content.left(), px(25.0)); // left padding
    assert_eq!(content.top(), px(10.0)); // top padding
    assert_eq!(content.right(), px(180.0)); // 200 - 20 (right padding)
    assert_eq!(content.bottom(), px(135.0)); // 150 - 15 (bottom padding)
}

#[test]
fn test_edges_inflate_rect_uniform() {
    let rect = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));
    let margin = Edges::all(px(10.0));

    let expanded = margin.inflate_rect(rect);

    // Should expand by margin on all sides
    assert_eq!(expanded.left(), px(40.0));
    assert_eq!(expanded.top(), px(40.0));
    assert_eq!(expanded.right(), px(160.0));
    assert_eq!(expanded.bottom(), px(160.0));
    assert_eq!(expanded.width(), px(120.0));
    assert_eq!(expanded.height(), px(120.0));
}

#[test]
fn test_edges_inflate_rect_asymmetric() {
    let rect = Rect::from_xywh(px(100.0), px(100.0), px(50.0), px(50.0));
    let margin = Edges::new(px(5.0), px(10.0), px(8.0), px(12.0));

    let expanded = margin.inflate_rect(rect);

    assert_eq!(expanded.left(), px(88.0)); // 100 - 12 (left margin)
    assert_eq!(expanded.top(), px(95.0)); // 100 - 5 (top margin)
    assert_eq!(expanded.right(), px(160.0)); // 150 + 10 (right margin)
    assert_eq!(expanded.bottom(), px(158.0)); // 150 + 8 (bottom margin)
}

#[test]
fn test_edges_deflate_then_inflate_roundtrip() {
    let original = Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(100.0));
    let edges = Edges::all(px(15.0));

    let deflated = edges.deflate_rect(original);
    let restored = edges.inflate_rect(deflated);

    // Should get back to original
    assert_eq!(restored, original);
}

#[test]
fn test_edges_zero_has_no_effect() {
    let rect = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));
    let no_padding = Edges::all(px(0.0));

    let deflated = no_padding.deflate_rect(rect);
    let inflated = no_padding.inflate_rect(rect);

    assert_eq!(deflated, rect);
    assert_eq!(inflated, rect);
}

// ============================================================================
// Construction methods
// ============================================================================

#[test]
fn test_edges_new() {
    let edges = Edges::new(px(1.0), px(2.0), px(3.0), px(4.0));

    assert_eq!(edges.top, px(1.0));
    assert_eq!(edges.right, px(2.0));
    assert_eq!(edges.bottom, px(3.0));
    assert_eq!(edges.left, px(4.0));
}

#[test]
fn test_edges_only_left() {
    let edges = Edges::only_left(px(10.0));

    assert_eq!(edges.left, px(10.0));
    assert_eq!(edges.top, px(0.0));
    assert_eq!(edges.right, px(0.0));
    assert_eq!(edges.bottom, px(0.0));
}

#[test]
fn test_edges_only_right() {
    let edges = Edges::only_right(px(10.0));

    assert_eq!(edges.right, px(10.0));
    assert_eq!(edges.top, px(0.0));
    assert_eq!(edges.left, px(0.0));
    assert_eq!(edges.bottom, px(0.0));
}

#[test]
fn test_edges_only_top() {
    let edges = Edges::only_top(px(10.0));

    assert_eq!(edges.top, px(10.0));
    assert_eq!(edges.left, px(0.0));
    assert_eq!(edges.right, px(0.0));
    assert_eq!(edges.bottom, px(0.0));
}

#[test]
fn test_edges_only_bottom() {
    let edges = Edges::only_bottom(px(10.0));

    assert_eq!(edges.bottom, px(10.0));
    assert_eq!(edges.top, px(0.0));
    assert_eq!(edges.left, px(0.0));
    assert_eq!(edges.right, px(0.0));
}

// ============================================================================
// Size operations
// ============================================================================

#[test]
fn test_edges_deflate_size() {
    let size = Size::new(px(100.0), px(100.0));
    let padding = Edges::all(px(10.0));

    let content_size = padding.deflate_size(size);

    // Width reduces by left + right padding
    assert_eq!(content_size.width, px(80.0));
    // Height reduces by top + bottom padding
    assert_eq!(content_size.height, px(80.0));
}

#[test]
fn test_edges_inflate_size() {
    let size = Size::new(px(80.0), px(80.0));
    let margin = Edges::all(px(10.0));

    let total_size = margin.inflate_size(size);

    // Width increases by left + right margin
    assert_eq!(total_size.width, px(100.0));
    // Height increases by top + bottom margin
    assert_eq!(total_size.height, px(100.0));
}

// ============================================================================
// Real-world use cases
// ============================================================================

#[test]
fn test_button_with_padding() {
    // Button container
    let button = Rect::from_xywh(px(0.0), px(0.0), px(120.0), px(40.0));

    // Internal padding
    let padding = Edges::symmetric(px(8.0), px(16.0));

    // Content area (where text goes)
    let text_area = padding.deflate_rect(button);

    assert_eq!(text_area.width(), px(88.0)); // 120 - 16*2
    assert_eq!(text_area.height(), px(24.0)); // 40 - 8*2
}

#[test]
fn test_card_with_uniform_padding() {
    let card = Rect::from_xywh(px(10.0), px(10.0), px(300.0), px(200.0));
    let padding = Edges::all(px(20.0));

    let content = padding.deflate_rect(card);

    // Content should be centered with padding on all sides
    assert_eq!(content.left(), px(30.0));
    assert_eq!(content.top(), px(30.0));
    assert_eq!(content.width(), px(260.0));
    assert_eq!(content.height(), px(160.0));
}

#[test]
fn test_list_item_with_asymmetric_padding() {
    let list_item = Rect::from_xywh(px(0.0), px(0.0), px(400.0), px(60.0));

    // More horizontal padding, less vertical
    let padding = Edges::symmetric(px(12.0), px(24.0));

    let content = padding.deflate_rect(list_item);

    assert_eq!(content.left(), px(24.0));
    assert_eq!(content.top(), px(12.0));
    assert_eq!(content.width(), px(352.0)); // 400 - 48
    assert_eq!(content.height(), px(36.0)); // 60 - 24
}

#[test]
fn test_safe_area_insets() {
    // iPhone X screen area
    let screen = Rect::from_xywh(px(0.0), px(0.0), px(375.0), px(812.0));

    // Safe area insets (notch and home indicator)
    let safe_area = Edges::new(
        px(44.0), // top (notch)
        px(0.0),  // right
        px(34.0), // bottom (home indicator)
        px(0.0),  // left
    );

    let safe_content = safe_area.deflate_rect(screen);

    assert_eq!(safe_content.top(), px(44.0));
    assert_eq!(safe_content.bottom(), px(778.0)); // 812 - 34
    assert_eq!(safe_content.height(), px(734.0));
}

#[test]
fn test_nested_padding_and_margin() {
    // Outer container
    let container = Rect::from_xywh(px(0.0), px(0.0), px(400.0), px(300.0));

    // Container padding
    let container_padding = Edges::all(px(20.0));
    let content_area = container_padding.deflate_rect(container);

    // Widget inside container
    let widget_size = Size::new(px(200.0), px(150.0));
    let widget_margin = Edges::all(px(10.0));

    // Widget's total space including margin
    let widget_total = widget_margin.inflate_size(widget_size);

    // Verify widget fits in content area
    assert!(widget_total.width <= content_area.width());
    assert!(widget_total.height <= content_area.height());
}

#[test]
fn test_dialog_box_layout() {
    // Dialog box with title, content, and actions
    let dialog = Rect::from_xywh(px(100.0), px(100.0), px(400.0), px(300.0));

    // Outer padding
    let padding = Edges::all(px(24.0));
    let content = padding.deflate_rect(dialog);

    // Title area (top portion)
    let title_height = px(60.0);
    let title_rect = Rect::from_xywh(content.left(), content.top(), content.width(), title_height);

    // Actions area (bottom portion)
    let actions_height = px(60.0);
    let actions_rect = Rect::from_xywh(
        content.left(),
        content.bottom() - actions_height,
        content.width(),
        actions_height,
    );

    // Content area (middle)
    let content_padding = Edges::vertical(px(8.0));
    let middle_top = title_rect.bottom() + content_padding.top;
    let middle_bottom = actions_rect.top() - content_padding.bottom;
    let middle_rect = Rect::from_xywh(
        content.left(),
        middle_top,
        content.width(),
        middle_bottom - middle_top,
    );

    // Verify layout
    assert_eq!(title_rect.top(), content.top());
    assert_eq!(actions_rect.bottom(), content.bottom());
    assert!(middle_rect.height() > px(0.0));
}

#[test]
fn test_edges_arithmetic() {
    let padding1 = Edges::all(px(10.0));
    let padding2 = Edges::all(px(5.0));

    // Addition
    let combined = padding1 + padding2;
    assert_eq!(combined.top, px(15.0));
    assert_eq!(combined.left, px(15.0));

    // Subtraction
    let difference = padding1 - padding2;
    assert_eq!(difference.top, px(5.0));
    assert_eq!(difference.left, px(5.0));
}

#[test]
fn test_edges_mul_assign() {
    let mut padding = Edges::all(px(10.0));
    padding *= 2.0;

    assert_eq!(padding.top, px(20.0));
    assert_eq!(padding.right, px(20.0));
    assert_eq!(padding.bottom, px(20.0));
    assert_eq!(padding.left, px(20.0));
}

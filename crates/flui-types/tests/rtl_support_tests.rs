//! Tests for RTL (Right-to-Left) layout support (Phase 12: User Story 10)
//!
//! This module tests bidirectional layout support for EdgeInsets/Edges,
//! including start/end semantics and automatic mirroring for RTL languages.

use flui_types::geometry::{px, Edges, Rect};
use flui_types::typography::TextDirection;

// ============================================================================
// TextDirection Basic Tests
// ============================================================================

#[test]
fn test_text_direction_ltr() {
    let dir = TextDirection::Ltr;

    assert!(dir.is_ltr());
    assert!(!dir.is_rtl());
}

#[test]
fn test_text_direction_rtl() {
    let dir = TextDirection::Rtl;

    assert!(!dir.is_ltr());
    assert!(dir.is_rtl());
}

#[test]
fn test_text_direction_opposite() {
    let ltr = TextDirection::Ltr;
    let rtl = TextDirection::Rtl;

    assert_eq!(ltr.opposite(), rtl);
    assert_eq!(rtl.opposite(), ltr);
}

#[test]
fn test_text_direction_default() {
    let default_dir = TextDirection::default();

    assert_eq!(default_dir, TextDirection::Ltr);
}

// ============================================================================
// LTR (Left-to-Right) Layout Tests
// ============================================================================

#[test]
fn test_ltr_padding_left_right() {
    // In LTR, start = left, end = right
    let padding = Edges::new(
        px(10.0), // top
        px(20.0), // right (end in LTR)
        px(10.0), // bottom
        px(30.0), // left (start in LTR)
    );

    assert_eq!(padding.left, px(30.0)); // start in LTR
    assert_eq!(padding.right, px(20.0)); // end in LTR
}

#[test]
fn test_ltr_margin_application() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let margin = Edges::new(
        px(10.0), // top
        px(20.0), // right
        px(10.0), // bottom
        px(30.0), // left
    );

    let with_margin = margin.inflate_rect(rect);

    // In LTR, left margin pushes content to the right
    assert_eq!(with_margin.left(), px(-30.0));
    assert_eq!(with_margin.right(), px(120.0));
}

// ============================================================================
// RTL (Right-to-Left) Layout Tests
// ============================================================================

#[test]
fn test_rtl_conceptual_mirroring() {
    // In RTL layouts, we conceptually mirror start/end
    // start = right, end = left

    // For RTL, we would construct edges with mirrored values
    let rtl_padding = Edges::new(
        px(10.0), // top (same in RTL)
        px(30.0), // right (start in RTL)
        px(10.0), // bottom (same in RTL)
        px(20.0), // left (end in RTL)
    );

    assert_eq!(rtl_padding.right, px(30.0)); // start in RTL
    assert_eq!(rtl_padding.left, px(20.0)); // end in RTL
}

#[test]
fn test_rtl_margin_application() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

    // RTL margin with mirrored values
    let rtl_margin = Edges::new(
        px(10.0), // top
        px(30.0), // right (start margin in RTL)
        px(10.0), // bottom
        px(20.0), // left (end margin in RTL)
    );

    let with_margin = rtl_margin.inflate_rect(rect);

    // In RTL, right margin is the "start" margin
    assert_eq!(with_margin.right(), px(130.0));
    assert_eq!(with_margin.left(), px(-20.0));
}

// ============================================================================
// Start/End Semantic Helper Functions
// ============================================================================

/// Helper function to create edges with start/end semantics
fn edges_from_start_end(
    direction: TextDirection,
    top: flui_types::geometry::Pixels,
    start: flui_types::geometry::Pixels,
    bottom: flui_types::geometry::Pixels,
    end: flui_types::geometry::Pixels,
) -> Edges<flui_types::geometry::Pixels> {
    match direction {
        TextDirection::Ltr => {
            // In LTR: start = left, end = right
            Edges::new(top, end, bottom, start)
        }
        TextDirection::Rtl => {
            // In RTL: start = right, end = left
            Edges::new(top, start, bottom, end)
        }
    }
}

#[test]
fn test_helper_ltr_start_end() {
    let edges = edges_from_start_end(
        TextDirection::Ltr,
        px(10.0), // top
        px(30.0), // start
        px(10.0), // bottom
        px(20.0), // end
    );

    // In LTR: start = left, end = right
    assert_eq!(edges.left, px(30.0));
    assert_eq!(edges.right, px(20.0));
}

#[test]
fn test_helper_rtl_start_end() {
    let edges = edges_from_start_end(
        TextDirection::Rtl,
        px(10.0), // top
        px(30.0), // start
        px(10.0), // bottom
        px(20.0), // end
    );

    // In RTL: start = right, end = left
    assert_eq!(edges.right, px(30.0));
    assert_eq!(edges.left, px(20.0));
}

// ============================================================================
// Real-World RTL Scenarios
// ============================================================================

#[test]
fn test_arabic_text_padding() {
    // Arabic text with start/end padding
    let start_padding = px(16.0);
    let end_padding = px(8.0);

    let ltr_edges = edges_from_start_end(
        TextDirection::Ltr,
        px(0.0),
        start_padding,
        px(0.0),
        end_padding,
    );

    let rtl_edges = edges_from_start_end(
        TextDirection::Rtl,
        px(0.0),
        start_padding,
        px(0.0),
        end_padding,
    );

    // Same semantic padding, different physical application
    assert_eq!(ltr_edges.left, px(16.0));
    assert_eq!(ltr_edges.right, px(8.0));

    assert_eq!(rtl_edges.right, px(16.0));
    assert_eq!(rtl_edges.left, px(8.0));
}

#[test]
fn test_hebrew_list_item_indent() {
    // List item with start indent
    let indent = px(24.0);

    let ltr_indent = edges_from_start_end(TextDirection::Ltr, px(0.0), indent, px(0.0), px(0.0));

    let rtl_indent = edges_from_start_end(TextDirection::Rtl, px(0.0), indent, px(0.0), px(0.0));

    // LTR: indent from left
    assert_eq!(ltr_indent.left, px(24.0));
    assert_eq!(ltr_indent.right, px(0.0));

    // RTL: indent from right
    assert_eq!(rtl_indent.right, px(24.0));
    assert_eq!(rtl_indent.left, px(0.0));
}

#[test]
fn test_chat_bubble_alignment() {
    // Chat app with messages aligned to start/end
    let message_margin = px(48.0); // For profile picture space

    // Sender (aligned to end)
    let ltr_sender = edges_from_start_end(
        TextDirection::Ltr,
        px(4.0),
        px(0.0),
        px(4.0),
        message_margin,
    );

    let rtl_sender = edges_from_start_end(
        TextDirection::Rtl,
        px(4.0),
        px(0.0),
        px(4.0),
        message_margin,
    );

    // LTR: sender on right
    assert_eq!(ltr_sender.right, px(48.0));
    assert_eq!(ltr_sender.left, px(0.0));

    // RTL: sender on left
    assert_eq!(rtl_sender.left, px(48.0));
    assert_eq!(rtl_sender.right, px(0.0));
}

#[test]
fn test_form_label_spacing() {
    // Form with label on start side
    let label_spacing = px(12.0);

    let ltr_form =
        edges_from_start_end(TextDirection::Ltr, px(0.0), label_spacing, px(0.0), px(0.0));

    let rtl_form =
        edges_from_start_end(TextDirection::Rtl, px(0.0), label_spacing, px(0.0), px(0.0));

    // Label spacing mirrors for RTL
    assert_eq!(ltr_form.left, px(12.0));
    assert_eq!(rtl_form.right, px(12.0));
}

// ============================================================================
// Bidirectional Content Tests
// ============================================================================

#[test]
fn test_mixed_content_layout() {
    // Content that might switch direction
    let content_padding = px(16.0);

    // Create padding for both directions
    let ltr = edges_from_start_end(
        TextDirection::Ltr,
        px(8.0),
        content_padding,
        px(8.0),
        content_padding,
    );

    let rtl = edges_from_start_end(
        TextDirection::Rtl,
        px(8.0),
        content_padding,
        px(8.0),
        content_padding,
    );

    // Symmetric padding is the same in both directions
    assert_eq!(ltr.left, ltr.right);
    assert_eq!(rtl.left, rtl.right);

    // But asymmetric would differ
    let ltr_asym = edges_from_start_end(TextDirection::Ltr, px(0.0), px(20.0), px(0.0), px(10.0));

    let rtl_asym = edges_from_start_end(TextDirection::Rtl, px(0.0), px(20.0), px(0.0), px(10.0));

    // Values are mirrored
    assert_eq!(ltr_asym.left, rtl_asym.right);
    assert_eq!(ltr_asym.right, rtl_asym.left);
}

// ============================================================================
// Navigation and Menu Tests
// ============================================================================

#[test]
fn test_drawer_menu_padding() {
    // Navigation drawer with start-side padding
    let drawer_padding = px(16.0);

    let ltr_drawer = edges_from_start_end(
        TextDirection::Ltr,
        px(24.0),
        drawer_padding,
        px(24.0),
        px(8.0),
    );

    let rtl_drawer = edges_from_start_end(
        TextDirection::Rtl,
        px(24.0),
        drawer_padding,
        px(24.0),
        px(8.0),
    );

    // Drawer opens from start side
    assert_eq!(ltr_drawer.left, px(16.0)); // LTR: left side
    assert_eq!(rtl_drawer.right, px(16.0)); // RTL: right side
}

#[test]
fn test_back_button_margin() {
    // Back button positioned at start
    let back_button_margin = px(4.0);

    let ltr_back = edges_from_start_end(
        TextDirection::Ltr,
        px(0.0),
        back_button_margin,
        px(0.0),
        px(0.0),
    );

    let rtl_back = edges_from_start_end(
        TextDirection::Rtl,
        px(0.0),
        back_button_margin,
        px(0.0),
        px(0.0),
    );

    // Back button on start side
    assert_eq!(ltr_back.left, px(4.0));
    assert_eq!(rtl_back.right, px(4.0));
}

// ============================================================================
// Icon and Image Positioning
// ============================================================================

#[test]
fn test_leading_icon_margin() {
    // Icon at start of text field
    let icon_margin = px(12.0);

    let ltr_icon = edges_from_start_end(TextDirection::Ltr, px(0.0), icon_margin, px(0.0), px(8.0));

    let rtl_icon = edges_from_start_end(TextDirection::Rtl, px(0.0), icon_margin, px(0.0), px(8.0));

    // Leading icon position
    assert_eq!(ltr_icon.left, px(12.0));
    assert_eq!(rtl_icon.right, px(12.0));
}

#[test]
fn test_trailing_icon_margin() {
    // Icon at end of text field
    let icon_margin = px(12.0);

    let ltr_icon = edges_from_start_end(TextDirection::Ltr, px(0.0), px(8.0), px(0.0), icon_margin);

    let rtl_icon = edges_from_start_end(TextDirection::Rtl, px(0.0), px(8.0), px(0.0), icon_margin);

    // Trailing icon position
    assert_eq!(ltr_icon.right, px(12.0));
    assert_eq!(rtl_icon.left, px(12.0));
}

// ============================================================================
// Complex Layout Scenarios
// ============================================================================

#[test]
fn test_card_with_action_buttons() {
    // Card with actions on end side
    let action_spacing = px(8.0);
    let card_padding = px(16.0);

    let ltr_card = edges_from_start_end(
        TextDirection::Ltr,
        card_padding,
        card_padding,
        card_padding,
        card_padding,
    );

    let ltr_actions = edges_from_start_end(
        TextDirection::Ltr,
        action_spacing,
        px(0.0),
        action_spacing,
        card_padding,
    );

    // Verify consistent padding
    assert_eq!(ltr_card.left, px(16.0));
    assert_eq!(ltr_actions.right, px(16.0));
}

#[test]
fn test_table_cell_alignment() {
    // Table cell with start/end padding
    let cell_padding = px(12.0);

    for direction in [TextDirection::Ltr, TextDirection::Rtl] {
        let padding = edges_from_start_end(direction, px(8.0), cell_padding, px(8.0), cell_padding);

        // Symmetric horizontal padding
        assert_eq!(padding.left, padding.right);
        assert_eq!(padding.left, px(12.0));
    }
}

// ============================================================================
// Rect Application with RTL
// ============================================================================

#[test]
fn test_rtl_rect_deflation() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(100.0));

    let ltr_padding =
        edges_from_start_end(TextDirection::Ltr, px(10.0), px(30.0), px(10.0), px(20.0));

    let rtl_padding =
        edges_from_start_end(TextDirection::Rtl, px(10.0), px(30.0), px(10.0), px(20.0));

    let ltr_content = ltr_padding.deflate_rect(rect);
    let rtl_content = rtl_padding.deflate_rect(rect);

    // Different physical coordinates, same semantic meaning
    // LTR: start=left=30, end=right=20
    assert_eq!(ltr_content.left(), px(30.0));
    assert_eq!(ltr_content.right(), px(180.0));

    // RTL: start=right=30, end=left=20
    assert_eq!(rtl_content.right(), px(170.0));
    assert_eq!(rtl_content.left(), px(20.0));
}

#[test]
fn test_rtl_consistent_content_size() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(100.0));

    // Same semantic padding in both directions
    let ltr_padding =
        edges_from_start_end(TextDirection::Ltr, px(10.0), px(30.0), px(10.0), px(20.0));

    let rtl_padding =
        edges_from_start_end(TextDirection::Rtl, px(10.0), px(30.0), px(10.0), px(20.0));

    let ltr_content = ltr_padding.deflate_rect(rect);
    let rtl_content = rtl_padding.deflate_rect(rect);

    // Content size should be the same
    assert_eq!(ltr_content.width(), rtl_content.width());
    assert_eq!(ltr_content.height(), rtl_content.height());
}

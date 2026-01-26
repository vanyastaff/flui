//! Tests for Corners type (Phase 11: User Story 9)
//!
//! This module tests per-corner values for rounded rectangles, border radii,
//! and corner-specific styling.

use flui_types::geometry::{Corners, Radius, px};

// ============================================================================
// Construction Tests
// ============================================================================

#[test]
fn test_corners_new() {
    let corners = Corners::new(
        Radius::circular(px(8.0)),
        Radius::circular(px(16.0)),
        Radius::circular(px(12.0)),
        Radius::circular(px(4.0))
    );

    assert_eq!(corners.top_left, Radius::circular(px(8.0)));
    assert_eq!(corners.top_right, Radius::circular(px(16.0)));
    assert_eq!(corners.bottom_right, Radius::circular(px(12.0)));
    assert_eq!(corners.bottom_left, Radius::circular(px(4.0)));
}

#[test]
fn test_corners_all() {
    let radius = Radius::circular(px(16.0));
    let corners = Corners::all(radius);

    assert_eq!(corners.top_left, radius);
    assert_eq!(corners.top_right, radius);
    assert_eq!(corners.bottom_right, radius);
    assert_eq!(corners.bottom_left, radius);
}

#[test]
fn test_corners_top() {
    let radius = Radius::circular(px(16.0));
    let corners = Corners::top(radius);

    assert_eq!(corners.top_left, radius);
    assert_eq!(corners.top_right, radius);
    assert_eq!(corners.bottom_right, Radius::ZERO);
    assert_eq!(corners.bottom_left, Radius::ZERO);
}

#[test]
fn test_corners_bottom() {
    let radius = Radius::circular(px(16.0));
    let corners = Corners::bottom(radius);

    assert_eq!(corners.top_left, Radius::ZERO);
    assert_eq!(corners.top_right, Radius::ZERO);
    assert_eq!(corners.bottom_right, radius);
    assert_eq!(corners.bottom_left, radius);
}

#[test]
fn test_corners_left() {
    let radius = Radius::circular(px(16.0));
    let corners = Corners::left(radius);

    assert_eq!(corners.top_left, radius);
    assert_eq!(corners.top_right, Radius::ZERO);
    assert_eq!(corners.bottom_right, Radius::ZERO);
    assert_eq!(corners.bottom_left, radius);
}

#[test]
fn test_corners_right() {
    let radius = Radius::circular(px(16.0));
    let corners = Corners::right(radius);

    assert_eq!(corners.top_left, Radius::ZERO);
    assert_eq!(corners.top_right, radius);
    assert_eq!(corners.bottom_right, radius);
    assert_eq!(corners.bottom_left, Radius::ZERO);
}

// ============================================================================
// Radius Operations
// ============================================================================

#[test]
fn test_radius_circular() {
    let radius = Radius::circular(px(16.0));

    assert_eq!(radius.x, px(16.0));
    assert_eq!(radius.y, px(16.0));
}

#[test]
fn test_radius_elliptical() {
    let radius = Radius::elliptical(px(20.0), px(10.0));

    assert_eq!(radius.x, px(20.0));
    assert_eq!(radius.y, px(10.0));
}

#[test]
fn test_radius_zero() {
    let radius = Radius::ZERO;

    assert_eq!(radius.x, px(0.0));
    assert_eq!(radius.y, px(0.0));
}

// ============================================================================
// Corners Map and Query Operations
// ============================================================================

#[test]
fn test_corners_map() {
    let corners = Corners::all(Radius::circular(px(16.0)));

    // Double all radii
    let doubled = corners.map(|r| Radius::circular(r.x * 2.0));

    assert_eq!(doubled.top_left.x, px(32.0));
    assert_eq!(doubled.top_right.x, px(32.0));
    assert_eq!(doubled.bottom_right.x, px(32.0));
    assert_eq!(doubled.bottom_left.x, px(32.0));
}

#[test]
fn test_corners_max() {
    let corners = Corners::new(
        Radius::circular(px(8.0)),
        Radius::circular(px(24.0)),
        Radius::circular(px(12.0)),
        Radius::circular(px(16.0))
    );

    let max = corners.max();

    assert_eq!(max, Radius::circular(px(24.0)));
}

#[test]
fn test_corners_min() {
    let corners = Corners::new(
        Radius::circular(px(8.0)),
        Radius::circular(px(24.0)),
        Radius::circular(px(12.0)),
        Radius::circular(px(16.0))
    );

    let min = corners.min();

    assert_eq!(min, Radius::circular(px(8.0)));
}

// ============================================================================
// Real-World Scenarios
// ============================================================================

#[test]
fn test_card_corners() {
    // Material Design card: rounded top corners only
    let card_radius = Corners::top(Radius::circular(px(8.0)));

    assert_eq!(card_radius.top_left.x, px(8.0));
    assert_eq!(card_radius.top_right.x, px(8.0));
    assert_eq!(card_radius.bottom_right.x, px(0.0));
    assert_eq!(card_radius.bottom_left.x, px(0.0));
}

#[test]
fn test_modal_corners() {
    // Bottom sheet modal: rounded top corners
    let modal_radius = Corners::top(Radius::circular(px(16.0)));

    assert!(modal_radius.top_left.x > px(0.0));
    assert!(modal_radius.top_right.x > px(0.0));
    assert_eq!(modal_radius.bottom_right.x, px(0.0));
    assert_eq!(modal_radius.bottom_left.x, px(0.0));
}

#[test]
fn test_button_corners() {
    // Fully rounded button
    let button_radius = Corners::all(Radius::circular(px(24.0)));

    // All corners should be equal
    assert_eq!(button_radius.top_left, button_radius.top_right);
    assert_eq!(button_radius.bottom_left, button_radius.bottom_right);
    assert_eq!(button_radius.top_left, button_radius.bottom_left);
}

#[test]
fn test_chip_corners() {
    // Pill-shaped chip with elliptical corners
    let chip_radius = Corners::all(Radius::elliptical(px(20.0), px(16.0)));

    // Verify elliptical shape
    assert_eq!(chip_radius.top_left.x, px(20.0));
    assert_eq!(chip_radius.top_left.y, px(16.0));
}

#[test]
fn test_notification_corners() {
    // iOS-style notification: all corners rounded
    let notification_radius = Corners::all(Radius::circular(px(12.0)));

    assert_eq!(notification_radius.top_left.x, px(12.0));
    assert_eq!(notification_radius.top_right.x, px(12.0));
    assert_eq!(notification_radius.bottom_right.x, px(12.0));
    assert_eq!(notification_radius.bottom_left.x, px(12.0));
}

#[test]
fn test_tabs_corners() {
    // Tab with rounded top corners
    let tab_radius = Corners::top(Radius::circular(px(8.0)));

    // Top corners rounded
    assert!(tab_radius.top_left.x > px(0.0));
    assert!(tab_radius.top_right.x > px(0.0));

    // Bottom corners square
    assert_eq!(tab_radius.bottom_right.x, px(0.0));
    assert_eq!(tab_radius.bottom_left.x, px(0.0));
}

#[test]
fn test_tooltip_corners() {
    // Small rounded corners for tooltip
    let tooltip_radius = Corners::all(Radius::circular(px(4.0)));

    assert_eq!(tooltip_radius.top_left.x, px(4.0));
    assert_eq!(tooltip_radius.top_right.x, px(4.0));
    assert_eq!(tooltip_radius.bottom_right.x, px(4.0));
    assert_eq!(tooltip_radius.bottom_left.x, px(4.0));
}

// ============================================================================
// Asymmetric Corner Patterns
// ============================================================================

#[test]
fn test_speech_bubble_left() {
    // Speech bubble with pointed left corner
    let bubble = Corners::new(
        Radius::ZERO,           // Pointed corner (top-left)
        Radius::circular(px(12.0)),
        Radius::circular(px(12.0)),
        Radius::circular(px(12.0))
    );

    assert_eq!(bubble.top_left.x, px(0.0));
    assert_eq!(bubble.top_right.x, px(12.0));
    assert_eq!(bubble.bottom_right.x, px(12.0));
    assert_eq!(bubble.bottom_left.x, px(12.0));
}

#[test]
fn test_asymmetric_card() {
    // Card with different corner radii for visual interest
    let card = Corners::new(
        Radius::circular(px(16.0)),  // Large top-left
        Radius::circular(px(4.0)),   // Small top-right
        Radius::circular(px(16.0)),  // Large bottom-right
        Radius::circular(px(4.0))    // Small bottom-left
    );

    assert_eq!(card.top_left.x, px(16.0));
    assert_eq!(card.top_right.x, px(4.0));
    assert_eq!(card.bottom_right.x, px(16.0));
    assert_eq!(card.bottom_left.x, px(4.0));
}

#[test]
fn test_diagonal_rounded() {
    // Only diagonal corners rounded
    let diagonal = Corners::new(
        Radius::circular(px(12.0)),
        Radius::ZERO,
        Radius::circular(px(12.0)),
        Radius::ZERO
    );

    assert_eq!(diagonal.top_left.x, px(12.0));
    assert_eq!(diagonal.top_right.x, px(0.0));
    assert_eq!(diagonal.bottom_right.x, px(12.0));
    assert_eq!(diagonal.bottom_left.x, px(0.0));
}

// ============================================================================
// Elliptical Corners
// ============================================================================

#[test]
fn test_wide_elliptical_corners() {
    // Wide elliptical corners (wider than tall)
    let corners = Corners::all(Radius::elliptical(px(32.0), px(16.0)));

    assert_eq!(corners.top_left.x, px(32.0));
    assert_eq!(corners.top_left.y, px(16.0));
}

#[test]
fn test_tall_elliptical_corners() {
    // Tall elliptical corners (taller than wide)
    let corners = Corners::all(Radius::elliptical(px(16.0), px(32.0)));

    assert_eq!(corners.top_left.x, px(16.0));
    assert_eq!(corners.top_left.y, px(32.0));
}

#[test]
fn test_mixed_circular_elliptical() {
    // Mix of circular and elliptical corners
    let corners = Corners::new(
        Radius::circular(px(16.0)),
        Radius::elliptical(px(20.0), px(10.0)),
        Radius::circular(px(16.0)),
        Radius::elliptical(px(20.0), px(10.0))
    );

    // Circular corners
    assert_eq!(corners.top_left.x, corners.top_left.y);
    assert_eq!(corners.bottom_right.x, corners.bottom_right.y);

    // Elliptical corners
    assert_ne!(corners.top_right.x, corners.top_right.y);
    assert_ne!(corners.bottom_left.x, corners.bottom_left.y);
}

// ============================================================================
// Corner Validation
// ============================================================================

#[test]
fn test_corners_all_same() {
    let corners = Corners::all(Radius::circular(px(16.0)));

    assert_eq!(corners.top_left, corners.top_right);
    assert_eq!(corners.top_left, corners.bottom_right);
    assert_eq!(corners.top_left, corners.bottom_left);
}

#[test]
fn test_corners_all_different() {
    let corners = Corners::new(
        Radius::circular(px(8.0)),
        Radius::circular(px(12.0)),
        Radius::circular(px(16.0)),
        Radius::circular(px(20.0))
    );

    assert_ne!(corners.top_left, corners.top_right);
    assert_ne!(corners.top_right, corners.bottom_right);
    assert_ne!(corners.bottom_right, corners.bottom_left);
}

#[test]
fn test_corners_zero_radius() {
    // All corners with zero radius (square)
    let corners = Corners::all(Radius::ZERO);

    assert_eq!(corners.top_left.x, px(0.0));
    assert_eq!(corners.top_right.x, px(0.0));
    assert_eq!(corners.bottom_right.x, px(0.0));
    assert_eq!(corners.bottom_left.x, px(0.0));
}

// ============================================================================
// Scale Operations
// ============================================================================

#[test]
fn test_corners_scale() {
    let corners = Corners::all(Radius::circular(px(16.0)));

    // Scale by 2x
    let scaled = corners.map(|r| Radius::circular(r.x * 2.0));

    assert_eq!(scaled.top_left.x, px(32.0));
    assert_eq!(scaled.top_right.x, px(32.0));
    assert_eq!(scaled.bottom_right.x, px(32.0));
    assert_eq!(scaled.bottom_left.x, px(32.0));
}

#[test]
fn test_corners_scale_elliptical() {
    let corners = Corners::all(Radius::elliptical(px(20.0), px(10.0)));

    // Scale by 1.5x
    let scaled = corners.map(|r| Radius::elliptical(r.x * 1.5, r.y * 1.5));

    assert_eq!(scaled.top_left.x, px(30.0));
    assert_eq!(scaled.top_left.y, px(15.0));
}

// ============================================================================
// Responsive Design Scenarios
// ============================================================================

#[test]
fn test_mobile_card_corners() {
    // Mobile: smaller corners
    let mobile_corners = Corners::all(Radius::circular(px(8.0)));

    assert_eq!(mobile_corners.top_left.x, px(8.0));
}

#[test]
fn test_desktop_card_corners() {
    // Desktop: larger corners
    let desktop_corners = Corners::all(Radius::circular(px(12.0)));

    assert_eq!(desktop_corners.top_left.x, px(12.0));
}

#[test]
fn test_responsive_corner_scaling() {
    let base_radius = Radius::circular(px(8.0));
    let mobile_corners = Corners::all(base_radius);

    // Scale up for desktop (1.5x)
    let desktop_corners = mobile_corners.map(|r| Radius::circular(r.x * 1.5));

    assert_eq!(desktop_corners.top_left.x, px(12.0));
}

// ============================================================================
// Corner Clamping for Small Rectangles
// ============================================================================

#[test]
fn test_corners_clamp_to_rect_size() {
    // Large corners that need to be clamped
    let desired_corners = Corners::all(Radius::circular(px(50.0)));

    // Rectangle is only 60x60, so corners can be max 30px
    let max_radius = px(30.0);

    let clamped = desired_corners.map(|r| {
        let clamped_value = if r.x > max_radius { max_radius } else { r.x };
        Radius::circular(clamped_value)
    });

    assert_eq!(clamped.top_left.x, px(30.0));
    assert_eq!(clamped.top_right.x, px(30.0));
}

#[test]
fn test_corners_prevent_overlap() {
    // Ensure opposite corners don't overlap
    let rect_width = px(100.0);
    let corner_radius = px(60.0);

    // Two corners on same edge shouldn't exceed half the edge length
    let max_radius = rect_width / 2.0;

    assert!(corner_radius > max_radius); // Would overlap

    let safe_corners = Corners::all(Radius::circular(max_radius));

    assert_eq!(safe_corners.top_left.x, px(50.0));
}

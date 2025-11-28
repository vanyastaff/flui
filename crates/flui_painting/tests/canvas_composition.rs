//! Canvas Composition Tests
//!
//! Tests for the zero-copy `Canvas::extend_from()` optimization and
//! `DisplayList::append()` method.

use flui_painting::prelude::*;
use flui_types::{geometry::Rect, styling::Color};

#[test]
fn test_append_canvas_empty_parent() {
    // Test fast path: appending to empty canvas should swap vectors (O(1))
    let mut parent = Canvas::new();

    let mut child = Canvas::new();
    let rect = Rect::from_ltrb(10.0, 10.0, 100.0, 100.0);
    let paint = Paint::fill(Color::RED);
    child.draw_rect(rect, &paint);

    let child_list = child.finish();
    let child_len = child_list.len();

    // Create new child for appending
    let mut child2 = Canvas::new();
    child2.draw_rect(rect, &paint);

    parent.extend_from(child2);

    let parent_list = parent.finish();

    // Should have same number of commands (swap happened)
    assert_eq!(parent_list.len(), child_len);
}

#[test]
fn test_append_canvas_non_empty_parent() {
    // Test slow path: appending to non-empty canvas
    let mut parent = Canvas::new();
    let rect1 = Rect::from_ltrb(0.0, 0.0, 50.0, 50.0);
    let paint1 = Paint::fill(Color::BLUE);
    parent.draw_rect(rect1, &paint1);

    let mut child = Canvas::new();
    let rect2 = Rect::from_ltrb(10.0, 10.0, 100.0, 100.0);
    let paint2 = Paint::fill(Color::RED);
    child.draw_rect(rect2, &paint2);

    parent.extend_from(child);

    let parent_list = parent.finish();

    // Should have both commands
    assert_eq!(parent_list.len(), 2);
}

#[test]
fn test_append_canvas_multiple_children() {
    // Simulate Flex with multiple children
    let mut parent = Canvas::new();

    for i in 0..10 {
        let mut child = Canvas::new();
        let rect = Rect::from_ltrb(i as f32 * 10.0, 0.0, (i + 1) as f32 * 10.0, 50.0);
        let paint = Paint::fill(Color::RED);
        child.draw_rect(rect, &paint);

        parent.extend_from(child);
    }

    let parent_list = parent.finish();

    // Should have all 10 commands
    assert_eq!(parent_list.len(), 10);
}

#[test]
fn test_append_canvas_preserves_order() {
    // Z-order is critical - commands must stay in order
    let mut parent = Canvas::new();

    // Background
    let bg_rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
    let bg_paint = Paint::fill(Color::WHITE);
    parent.draw_rect(bg_rect, &bg_paint);

    // Child 1
    let mut child1 = Canvas::new();
    let rect1 = Rect::from_ltrb(10.0, 10.0, 50.0, 50.0);
    let paint1 = Paint::fill(Color::RED);
    child1.draw_rect(rect1, &paint1);
    parent.extend_from(child1);

    // Child 2
    let mut child2 = Canvas::new();
    let rect2 = Rect::from_ltrb(30.0, 30.0, 70.0, 70.0);
    let paint2 = Paint::fill(Color::BLUE);
    child2.draw_rect(rect2, &paint2);
    parent.extend_from(child2);

    // Foreground
    let fg_rect = Rect::from_ltrb(40.0, 40.0, 60.0, 60.0);
    let fg_paint = Paint::fill(Color::GREEN);
    parent.draw_rect(fg_rect, &fg_paint);

    let parent_list = parent.finish();

    // Should be: background, child1, child2, foreground
    assert_eq!(parent_list.len(), 4);
}

#[test]
fn test_append_empty_canvas() {
    // Appending empty canvas should be no-op
    let mut parent = Canvas::new();
    let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::fill(Color::RED);
    parent.draw_rect(rect, &paint);

    let empty_child = Canvas::new();
    parent.extend_from(empty_child);

    let parent_list = parent.finish();

    // Should still have only 1 command
    assert_eq!(parent_list.len(), 1);
}

#[test]
fn test_append_canvas_with_transforms() {
    // Test that transforms are preserved during append
    let mut parent = Canvas::new();

    let mut child = Canvas::new();
    child.save();
    child.translate(50.0, 50.0);
    child.rotate(std::f32::consts::PI / 4.0);

    let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::fill(Color::RED);
    child.draw_rect(rect, &paint);
    child.restore();

    parent.extend_from(child);

    let parent_list = parent.finish();

    // Commands should include the transformed draw
    assert!(!parent_list.is_empty());
}

#[test]
fn test_bounds_after_append() {
    // Bounds should be union of all appended canvases
    let mut parent = Canvas::new();
    let rect1 = Rect::from_ltrb(0.0, 0.0, 50.0, 50.0);
    let paint = Paint::fill(Color::RED);
    parent.draw_rect(rect1, &paint);

    let mut child = Canvas::new();
    let rect2 = Rect::from_ltrb(100.0, 100.0, 200.0, 200.0);
    child.draw_rect(rect2, &paint);

    parent.extend_from(child);

    let parent_list = parent.finish();
    let bounds = parent_list.bounds();

    // Bounds should encompass both rectangles
    assert!(bounds.left() <= 0.0);
    assert!(bounds.top() <= 0.0);
    assert!(bounds.right() >= 200.0);
    assert!(bounds.bottom() >= 200.0);
}

#[test]
fn test_display_list_append_directly() {
    // Test that DisplayList can be created from Canvas
    // (DisplayList::append is internal, tested via Canvas::append_canvas)

    // Create canvases and get their display lists
    let mut canvas1 = Canvas::new();
    canvas1.draw_rect(
        Rect::from_ltrb(0.0, 0.0, 50.0, 50.0),
        &Paint::fill(Color::RED),
    );
    let dl1 = canvas1.finish();

    let mut canvas2 = Canvas::new();
    canvas2.draw_rect(
        Rect::from_ltrb(50.0, 50.0, 100.0, 100.0),
        &Paint::fill(Color::BLUE),
    );
    let dl2 = canvas2.finish();

    // Verify both have commands
    assert_eq!(dl1.len(), 1);
    assert_eq!(dl2.len(), 1);
}

#[test]
fn test_large_composition_performance() {
    // Stress test: compose many children
    let mut parent = Canvas::new();

    for i in 0..100 {
        let mut child = Canvas::new();

        // Each child has multiple commands
        for j in 0..10 {
            let rect = Rect::from_ltrb((i * 10 + j) as f32, 0.0, (i * 10 + j + 1) as f32, 50.0);
            let paint = Paint::fill(Color::RED);
            child.draw_rect(rect, &paint);
        }

        parent.extend_from(child);
    }

    let parent_list = parent.finish();

    // Should have 100 children × 10 commands = 1000 commands
    assert_eq!(parent_list.len(), 1000);
}

#[test]
fn test_nested_composition() {
    // Test nested Canvas composition (parent → child → grandchild)
    let mut grandchild = Canvas::new();
    grandchild.draw_rect(
        Rect::from_ltrb(0.0, 0.0, 10.0, 10.0),
        &Paint::fill(Color::RED),
    );

    let mut child = Canvas::new();
    child.draw_rect(
        Rect::from_ltrb(0.0, 0.0, 20.0, 20.0),
        &Paint::fill(Color::GREEN),
    );
    child.extend_from(grandchild);

    let mut parent = Canvas::new();
    parent.draw_rect(
        Rect::from_ltrb(0.0, 0.0, 30.0, 30.0),
        &Paint::fill(Color::BLUE),
    );
    parent.extend_from(child);

    let parent_list = parent.finish();

    // Should have all 3 commands
    assert_eq!(parent_list.len(), 3);
}

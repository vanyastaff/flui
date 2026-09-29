//! Canvas Transform API Tests
//!
//! Tests for the `Canvas::transform()` method integration with the high-level
//! Transform API from `flui_foundation::geometry`.

use std::f64::consts::PI;

use flui_foundation::geometry::{Rect, Transform};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

#[test]
fn test_transform_composition() {
    // Test composing multiple Transform operations
    let mut canvas = Canvas::new();

    let composed = Transform::translate(50.0, 50.0)
        .then(Transform::rotate(PI / 4.0))
        .then(Transform::scale(2.0));

    canvas.transform(composed);

    let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_with_save_restore() {
    // Test that Transform works correctly with save/restore
    let mut canvas = Canvas::new();

    canvas.save();
    canvas.transform(Transform::rotate(PI / 4.0));

    let rect1 = Rect::from_ltrb(0.0, 0.0, 50.0, 50.0);
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect1, &paint);

    canvas.restore();

    let rect2 = Rect::from_ltrb(50.0, 50.0, 100.0, 100.0);
    canvas.draw_rect(rect2, &paint);

    let display_list = canvas.finish();
    // `Save`, the rect drawn under the rotation, `Restore`, then the rect
    // drawn after it — the bracket is what keeps the rotation off the second.
    assert_eq!(display_list.len(), 4);
}

//! Canvas Transform API Tests
//!
//! Tests for the `Canvas::transform()` method integration with the high-level
//! Transform API from `flui_types::geometry`.

use flui_painting::prelude::*;
use flui_types::{
    geometry::{px, Matrix4, Rect, Transform},
    styling::Color,
};
use std::f32::consts::PI;

#[test]
fn test_transform_with_transform_enum() {
    // Test that Canvas::transform() accepts Transform enum
    let mut canvas = Canvas::new();

    let transform = Transform::rotate(PI / 4.0);
    canvas.transform(transform);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_with_matrix4() {
    // Test that Canvas::transform() accepts Matrix4 (backward compatibility)
    let mut canvas = Canvas::new();

    let matrix = Matrix4::rotation_z(PI / 4.0);
    canvas.transform(matrix);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_composition() {
    // Test composing multiple Transform operations
    let mut canvas = Canvas::new();

    let composed = Transform::translate(50.0, 50.0)
        .then(Transform::rotate(PI / 4.0))
        .then(Transform::scale(2.0));

    canvas.transform(composed);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_skew() {
    // Test skew transform (italic text effect)
    let mut canvas = Canvas::new();

    let italic = Transform::skew(0.2, 0.0);
    canvas.transform(italic);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_rotate_around() {
    // Test rotating around a pivot point
    let mut canvas = Canvas::new();

    let center_x = 50.0;
    let center_y = 50.0;
    let rotate_around = Transform::rotate_around(PI / 2.0, center_x, center_y);

    canvas.transform(rotate_around);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
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

    let rect1 = Rect::from_ltrb(px(0.0), px(0.0), px(50.0), px(50.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect1, &paint);

    canvas.restore();

    let rect2 = Rect::from_ltrb(px(50.0), px(50.0), px(100.0), px(100.0));
    canvas.draw_rect(rect2, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 2);
}

#[test]
fn test_transform_multiple_calls() {
    // Test that multiple transform() calls accumulate
    let mut canvas = Canvas::new();

    canvas.transform(Transform::translate(50.0, 50.0));
    canvas.transform(Transform::rotate(PI / 4.0));
    canvas.transform(Transform::scale(2.0));

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_identity_optimization() {
    // Test that identity transforms work correctly
    let mut canvas = Canvas::new();

    canvas.transform(Transform::Identity);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_mixed_with_legacy_methods() {
    // Test that new transform() works alongside legacy translate/rotate/scale
    let mut canvas = Canvas::new();

    // Legacy API
    canvas.translate(10.0, 10.0);

    // New Transform API
    canvas.transform(Transform::rotate(PI / 4.0));

    // New scale API
    canvas.scale_uniform(2.0);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_scale_around() {
    // Test scaling around a pivot point
    let mut canvas = Canvas::new();

    let center_x = 50.0;
    let center_y = 50.0;
    let scale_around = Transform::scale_around(2.0, 2.0, center_x, center_y);

    canvas.transform(scale_around);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_get_current_matrix() {
    // Test that we can get the current transform after applying Transform
    let mut canvas = Canvas::new();

    let transform = Transform::translate(50.0, 100.0);
    canvas.transform(transform);

    let current = canvas.transform_matrix();

    // Verify translation was applied
    let expected = Matrix4::translation(50.0, 100.0, 0.0);
    #[allow(clippy::float_cmp)] // Exact comparison is correct for transform matrices
    {
        assert_eq!(current.m[12], expected.m[12]); // Translation X
        assert_eq!(current.m[13], expected.m[13]); // Translation Y
    }
}

#[test]
fn test_transform_animation_pattern() {
    // Test typical animation pattern: rotate around center over time
    let mut canvas = Canvas::new();

    let button_center_x = 150.0;
    let button_center_y = 75.0;

    // Simulate animation frame at t=0.25 (quarter rotation)
    let t = 0.25;
    let angle = t * PI * 2.0;
    let rotation = Transform::rotate_around(angle, button_center_x, button_center_y);

    canvas.transform(rotation);

    let rect = Rect::from_ltrb(
        px(button_center_x - 25.0),
        px(button_center_y - 25.0),
        px(button_center_x + 25.0),
        px(button_center_y + 25.0),
    );
    let paint = Paint::fill(Color::BLUE);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_card_flip_pattern() {
    // Test card flip effect with rotation + skew
    let mut canvas = Canvas::new();

    let card_flip = Transform::rotate(PI)
        .then(Transform::skew(0.2, 0.0))
        .then(Transform::translate(0.0, 10.0));

    canvas.transform(card_flip);

    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(200.0), px(300.0));
    let paint = Paint::fill(Color::WHITE);
    canvas.draw_rect(rect, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 1);
}

#[test]
fn test_transform_parallax_layers() {
    // Test parallax scrolling pattern with different translation speeds
    let mut parent = Canvas::new();

    // Background layer (slow)
    let mut bg = Canvas::new();
    bg.transform(Transform::translate(0.0, 50.0));
    bg.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(800.0), px(600.0)),
        &Paint::fill(Color::rgba(200, 200, 255, 255)),
    );
    parent.extend_from(bg);

    // Midground layer (medium)
    let mut mg = Canvas::new();
    mg.transform(Transform::translate(0.0, 100.0));
    mg.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(800.0), px(600.0)),
        &Paint::fill(Color::rgba(150, 150, 255, 255)),
    );
    parent.extend_from(mg);

    // Foreground layer (fast)
    let mut fg = Canvas::new();
    fg.transform(Transform::translate(0.0, 150.0));
    fg.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(800.0), px(600.0)),
        &Paint::fill(Color::rgba(100, 100, 255, 255)),
    );
    parent.extend_from(fg);

    let display_list = parent.finish();
    assert_eq!(display_list.len(), 3);
}

//! Tests for closure-based scoped Canvas operations

use flui_painting::prelude::*;
use flui_types::geometry::{Matrix4, Point, RRect, Rect};
use flui_types::painting::Path;
use flui_types::styling::Color;
use std::f32::consts::PI;

#[test]
fn test_with_save_restores_transform() {
    let mut canvas = Canvas::new();
    let original = canvas.transform_matrix();

    canvas.with_save(|c| {
        c.translate(100.0, 100.0);
        c.rotate(PI / 4.0);
        assert_ne!(c.transform_matrix(), original);
    });

    // Transform should be restored
    assert_eq!(canvas.transform_matrix(), original);
}

#[test]
fn test_with_save_returns_value() {
    let mut canvas = Canvas::new();

    let result = canvas.with_save(|c| {
        c.translate(50.0, 50.0);
        42
    });

    assert_eq!(result, 42);
}

#[test]
fn test_with_translate() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.with_translate(100.0, 50.0, |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    });

    // Should have 1 command
    assert_eq!(canvas.len(), 1);

    // Transform should be restored
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_rotate() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::BLUE);

    canvas.with_rotate(PI / 2.0, |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    });

    assert_eq!(canvas.len(), 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_rotate_around() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::GREEN);

    canvas.with_rotate_around(PI / 4.0, 50.0, 50.0, |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
    });

    assert_eq!(canvas.len(), 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_scale() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.with_scale(2.0, |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    });

    assert_eq!(canvas.len(), 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_scale_xy() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.with_scale_xy(2.0, 0.5, |c| {
        c.draw_circle(Point::new(50.0, 50.0), 25.0, &paint);
    });

    assert_eq!(canvas.len(), 1);
}

#[test]
fn test_with_transform() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    let transform = Matrix4::translation(100.0, 100.0, 0.0);

    canvas.with_transform(transform, |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    });

    assert_eq!(canvas.len(), 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_clip_rect() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let clip = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

    canvas.with_clip_rect(clip, |c| {
        c.draw_circle(Point::new(50.0, 50.0), 80.0, &paint);
    });

    // Should have clip command + circle command
    assert_eq!(canvas.len(), 2);
}

#[test]
fn test_with_clip_rrect() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let clip = RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 10.0);

    canvas.with_clip_rrect(clip, |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 200.0, 200.0), &paint);
    });

    assert_eq!(canvas.len(), 2);
}

#[test]
fn test_with_clip_path() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let clip_path = Path::circle(Point::new(50.0, 50.0), 40.0);

    canvas.with_clip_path(&clip_path, |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
    });

    assert_eq!(canvas.len(), 2);
}

#[test]
fn test_nested_with_operations() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.with_translate(100.0, 100.0, |c| {
        c.with_rotate(PI / 4.0, |c| {
            c.with_scale(2.0, |c| {
                c.draw_rect(Rect::from_xywh(-25.0, -25.0, 50.0, 50.0), &paint);
            });
        });
    });

    assert_eq!(canvas.len(), 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_canvas_record() {
    let display_list = Canvas::record(|c| {
        let paint = Paint::fill(Color::RED);
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
        c.draw_circle(Point::new(50.0, 50.0), 25.0, &paint);
    });

    assert_eq!(display_list.len(), 2);
}

#[test]
fn test_canvas_build() {
    let mut canvas = Canvas::build(|c| {
        c.translate(100.0, 100.0);
        let paint = Paint::fill(Color::RED);
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), &paint);
    });

    // Canvas still usable
    let paint = Paint::fill(Color::BLUE);
    canvas.draw_circle(Point::new(200.0, 200.0), 25.0, &paint);

    let display_list = canvas.finish();
    assert_eq!(display_list.len(), 2);
}

#[test]
fn test_with_opacity() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let bounds = Rect::from_xywh(0.0, 0.0, 200.0, 200.0);

    canvas.with_opacity(0.5, Some(bounds), |c| {
        c.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), &paint);
        c.draw_rect(Rect::from_xywh(50.0, 50.0, 100.0, 100.0), &paint);
    });

    // SaveLayer + 2 rects + RestoreLayer
    assert_eq!(canvas.len(), 4);
}

#[test]
fn test_chained_operations() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    // Draw multiple items with different transforms
    for i in 0..5 {
        canvas.with_translate(i as f32 * 50.0, 0.0, |c| {
            c.draw_rect(Rect::from_xywh(0.0, 0.0, 40.0, 40.0), &paint);
        });
    }

    assert_eq!(canvas.len(), 5);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_record_reusable_icon() {
    // Create reusable icon
    let icon = Canvas::record(|c| {
        let outline = Paint::stroke(Color::BLACK, 2.0);
        let fill = Paint::fill(Color::WHITE);

        c.draw_circle(Point::new(16.0, 16.0), 14.0, &fill);
        c.draw_circle(Point::new(16.0, 16.0), 14.0, &outline);
    });

    // Use icon multiple times
    let mut canvas = Canvas::new();

    canvas.with_translate(0.0, 0.0, |c| {
        c.append_display_list(icon.clone());
    });

    canvas.with_translate(50.0, 0.0, |c| {
        c.append_display_list(icon.clone());
    });

    canvas.with_translate(100.0, 0.0, |c| {
        c.append_display_list(icon);
    });

    // 3 icons * 2 commands each = 6 commands
    assert_eq!(canvas.len(), 6);
}

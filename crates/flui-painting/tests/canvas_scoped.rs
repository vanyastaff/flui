//! Tests for closure-based scoped Canvas operations

use std::f32::consts::PI;

use flui_painting::prelude::*;
use flui_types::{
    geometry::{Matrix4, Point, RRect, Rect, px},
    painting::Path,
    styling::Color,
};

/// Marks each recorded command as the scope bracket or as body content.
///
/// Every scoped helper brackets its body with `Save`/`Restore`. That bracket is
/// not bookkeeping: `Save` is what makes a later `Clip*` undoable, and without
/// the pair a clip applied inside the closure keeps applying to everything
/// recorded after it. Asserting the bracket is asserting the scope exists in
/// the recording at all.
fn scope_shape(canvas: &Canvas) -> Vec<&'static str> {
    canvas
        .display_list()
        .iter()
        .map(|command| match command {
            DrawCommand::Save { .. } => "Save",
            DrawCommand::Restore { .. } => "Restore",
            _ => "body",
        })
        .collect()
}

/// Asserts the recording is exactly one scope wrapping `body_len` commands.
fn assert_scope_wraps(canvas: &Canvas, body_len: usize) {
    let shape = scope_shape(canvas);
    let mut expected = vec!["Save"];
    expected.extend(std::iter::repeat_n("body", body_len));
    expected.push("Restore");
    assert_eq!(
        shape, expected,
        "a scoped helper must record its Save/Restore bracket around the body",
    );
}

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
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            &paint,
        );
    });

    // Should have 1 command
    assert_scope_wraps(&canvas, 1);

    // Transform should be restored
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_rotate() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::BLUE);

    canvas.with_rotate(PI / 2.0, |c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            &paint,
        );
    });

    assert_scope_wraps(&canvas, 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_rotate_around() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::GREEN);

    canvas.with_rotate_around(PI / 4.0, 50.0, 50.0, |c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            &paint,
        );
    });

    assert_scope_wraps(&canvas, 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_scale() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.with_scale(2.0, |c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            &paint,
        );
    });

    assert_scope_wraps(&canvas, 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_scale_xy() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.with_scale_xy(2.0, 0.5, |c| {
        c.draw_circle(Point::new(px(50.0), px(50.0)), px(25.0), &paint);
    });

    assert_scope_wraps(&canvas, 1);
}

#[test]
fn test_with_transform() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    let transform = Matrix4::translation(100.0, 100.0, 0.0);

    canvas.with_transform(transform, |c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            &paint,
        );
    });

    assert_scope_wraps(&canvas, 1);
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_clip_rect() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let clip = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

    canvas.with_clip_rect(clip, |c| {
        c.draw_circle(Point::new(px(50.0), px(50.0)), px(80.0), &paint);
    });

    // Should have clip command + circle command
    assert_scope_wraps(&canvas, 2);
}

#[test]
fn test_with_clip_rrect() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let clip = RRect::from_rect_circular(
        Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
        px(10.0),
    );

    canvas.with_clip_rrect(clip, |c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(200.0)),
            &paint,
        );
    });

    assert_scope_wraps(&canvas, 2);
}

#[test]
fn test_with_clip_path() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let clip_path = Path::circle(Point::new(px(50.0), px(50.0)), 40.0);

    canvas.with_clip_path(&clip_path, |c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            &paint,
        );
    });

    assert_scope_wraps(&canvas, 2);
}

#[test]
fn test_nested_with_operations() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.with_translate(100.0, 100.0, |c| {
        c.with_rotate(PI / 4.0, |c| {
            c.with_scale(2.0, |c| {
                c.draw_rect(
                    Rect::from_xywh(px(-25.0), px(-25.0), px(50.0), px(50.0)),
                    &paint,
                );
            });
        });
    });

    // Three nested scopes around one rect: the brackets nest rather than
    // collapsing, so the inner clip/transform of each level can be undone
    // independently.
    assert_eq!(
        scope_shape(&canvas),
        [
            "Save", "Save", "Save", "body", "Restore", "Restore", "Restore"
        ],
    );
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

#[test]
fn test_with_opacity() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let bounds = Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(200.0));

    canvas.with_opacity(0.5, Some(bounds), |c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            &paint,
        );
        c.draw_rect(
            Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0)),
            &paint,
        );
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
            c.draw_rect(
                Rect::from_xywh(px(0.0), px(0.0), px(40.0), px(40.0)),
                &paint,
            );
        });
    }

    // Five scopes, each Save + one rect + Restore.
    assert_eq!(canvas.len(), 15);
    assert_eq!(
        scope_shape(&canvas),
        ["Save", "body", "Restore"].repeat(5),
        "each iteration records its own bracket; they must not nest or merge",
    );
    assert_eq!(canvas.transform_matrix(), Matrix4::identity());
}

/// The reason the bracket exists: a clip must not outlive its scope.
///
/// Before `Save`/`Restore` were recorded, a scoped clip put a `ClipRect` into
/// the stream with nothing after it to undo the narrowing. The CPU-side clip
/// stack unwound correctly, so the canvas *looked* right, while a backend
/// replaying the commands kept clipping every later draw — including siblings
/// that were never inside the closure.
///
/// This pins the shape that makes the difference: the command drawn after the
/// scope is separated from the clip by a `Restore`.
#[test]
fn a_scoped_clip_is_closed_before_later_drawing() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    let clip = Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0));

    canvas.with_clip_rect(clip, |c| {
        c.draw_rect(Rect::from_xywh(px(0.0), px(0.0), px(5.0), px(5.0)), &paint);
    });
    // Drawn outside the closure — it must not inherit the clip.
    canvas.draw_rect(
        Rect::from_xywh(px(100.0), px(100.0), px(5.0), px(5.0)),
        &paint,
    );

    assert_eq!(
        scope_shape(&canvas),
        ["Save", "body", "body", "Restore", "body"],
        "the clip and the rect inside it sit within the bracket; the later \
         rect sits after the Restore that ends it",
    );

    let commands: Vec<_> = canvas.display_list().iter().collect();
    assert!(
        matches!(commands[1], DrawCommand::ClipRect { .. }),
        "the first body command is the clip itself, so the Restore that \
         follows is what bounds it",
    );
}

/// A scope marker is not a draw.
///
/// `kind()` has a `_ => Draw` fallback, so a new variant that is not listed
/// explicitly silently becomes a drawing command — which would make
/// `is_draw()` true for `Save`/`Restore` and inflate every draw count and
/// filter over a recording that uses a scope.
#[test]
fn scope_markers_are_classified_as_layer_not_draw() {
    let mut canvas = Canvas::new();
    canvas.with_save(|c| {
        c.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)),
            &Paint::fill(Color::RED),
        );
    });

    let draws = canvas
        .display_list()
        .iter()
        .filter(|command| command.is_draw())
        .count();
    assert_eq!(
        draws, 1,
        "only the rect is a draw; the Save/Restore bracket is not",
    );

    for command in canvas.display_list() {
        if matches!(
            command,
            DrawCommand::Save { .. } | DrawCommand::Restore { .. }
        ) {
            assert_eq!(
                command.kind(),
                flui_painting::display_list::CommandKind::Layer
            );
        }
    }
}

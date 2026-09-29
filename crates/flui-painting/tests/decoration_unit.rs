//! `BoxDecoration` painter: Flutter's draw order (shadows → background
//! → image → border), gradient resolution against the paint rect, and
//! rounded-rect hit testing.
//!
//! All assertions are sans-IO over the recorded display list — the
//! same contract the fragment paint model relies on.

use flui_foundation::geometry::{Offset, Point, Rect};
use flui_painting::BoxShape;
use flui_painting::{
    Canvas, DecorationPaintOptions, DrawOp, box_decoration_hit_test, paint_box_decoration,
};
use flui_painting::{
    paint::PaintStyle,
    styling::{
        Border, BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, BoxShadow,
        Color,
    },
};

fn rect100() -> Rect<f64> {
    Rect::from_ltrb(0.0, 0.0, 100.0, 50.0)
}

fn commands(decoration: &BoxDecoration<f64>) -> Vec<DrawOp> {
    commands_in(rect100(), decoration)
}

fn commands_in(rect: Rect<f64>, decoration: &BoxDecoration<f64>) -> Vec<DrawOp> {
    let mut canvas = Canvas::new();
    paint_box_decoration(
        &mut canvas,
        rect,
        decoration,
        DecorationPaintOptions::default(),
    );
    canvas.finish().iter().map(|c| c.op.clone()).collect()
}

#[test]
fn flutter_paint_order_shadow_background_border() {
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            2.0,
            BorderStyle::Solid,
        ))))
        .set_box_shadow(Some(vec![BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(0.0, 2.0),
            blur_radius: 4.0,
            spread_radius: 1.0,
            inset: false,
        }]));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 3, "shadow + background + border");
    assert!(
        matches!(cmds[0], DrawOp::Shadow { .. }),
        "shadows paint FIRST (behind everything)"
    );
    assert!(matches!(cmds[1], DrawOp::Rect { .. }));
    assert!(
        matches!(cmds[2], DrawOp::DRRect { .. }),
        "a uniform border strokes inside via an outer/inner pair, LAST"
    );
}

#[test]
fn hit_test_respects_rounded_corners() {
    let decoration =
        BoxDecoration::with_color(Color::RED).set_border_radius(Some(BorderRadius::circular(20.0)));
    let rect = rect100();

    // Center: inside.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(50.0, 25.0)
    ));
    // The exact corner of the BOUNDING rect lies outside the rounded
    // shape (radius 20 cuts it off).
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(1.0, 1.0)
    ));
    // Just inside the corner arc.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(20.0, 20.0)
    ));
    // Outside the rect entirely.
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(150.0, 25.0)
    ));
}

// ============================================================================
// `BoxShape::Circle`
// ============================================================================

/// A square rect (100x100) so the inscribed circle has a clean radius
/// (50) and center (50, 50).
fn square_rect() -> Rect<f64> {
    Rect::from_ltrb(0.0, 0.0, 100.0, 100.0)
}

#[test]
fn circle_uniform_border_is_a_stroked_circle_not_a_drrect() {
    let rect = square_rect(); // r = 50
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_shape(BoxShape::Circle)
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            4.0,
            BorderStyle::Solid,
        ))));
    let cmds = commands_in(rect, &decoration);

    assert_eq!(cmds.len(), 2, "background fill + stroked border");
    assert!(
        !cmds.iter().any(|c| matches!(c, DrawOp::DRRect { .. })),
        "a circular border must stroke a circle, not fill a drrect ring \
         (tessellate_drrect bulges ~6% on the diagonals relative to the \
         exact circle fill); commands: {cmds:?}"
    );
    match &cmds[1] {
        DrawOp::Circle {
            center,
            radius,
            paint,
            ..
        } => {
            assert_eq!(*center, Point::new(50.0, 50.0));
            // Inside-stroke convention (matching the rect/rrect
            // draw_drrect(outer, outer.inflate(-width)) path): the
            // stroke's OUTER edge lands on the fill radius (50), so the
            // stroke is centered at 50 - 4/2 = 48.
            assert_eq!(*radius, 48.0);
            assert_eq!(paint.style, PaintStyle::Stroke);
            assert_eq!(paint.stroke_width, 4.0);
            assert_eq!(paint.color, Color::BLACK);
        }
        other => panic!("expected a stroked DrawCircle border, got {other:?}"),
    }
}

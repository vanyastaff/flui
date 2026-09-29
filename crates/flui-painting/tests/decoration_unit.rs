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
    paint::{PaintStyle, Shader},
    styling::{
        Border, BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, BoxShadow,
        Color, Gradient, LinearGradient,
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
fn radius_switches_to_rounded_primitives() {
    let decoration =
        BoxDecoration::with_color(Color::RED).set_border_radius(Some(BorderRadius::circular(8.0)));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], DrawOp::RRect { .. }));
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
fn gradient_wins_over_color_and_resolves_alignment() {
    let gradient = Gradient::Linear(LinearGradient::new(
        flui_painting::Alignment::CENTER_LEFT,
        flui_painting::Alignment::CENTER_RIGHT,
        vec![Color::RED, Color::BLUE],
        None,
        flui_painting::paint::TileMode::Clamp,
    ));
    let decoration = BoxDecoration::with_color(Color::WHITE).set_gradient(Some(gradient.clone()));
    let cmds = commands(&decoration);
    assert_eq!(
        cmds.len(),
        1,
        "a gradient replaces the flat color entirely (Flutter contract)"
    );
    // A gradient is a shader on an ordinary fill: the same `Rect` op a flat
    // colour records, with the shader in the paint. Default LinearGradient
    // runs centerLeft → centerRight: alignment resolves against the CONCRETE
    // rect, and the recorded shader carries the resolved endpoints.
    let DrawOp::Rect { rect, paint } = &cmds[0] else {
        panic!(
            "a gradient background must record a Rect op; got {:?}",
            cmds[0]
        );
    };
    assert_eq!(*rect, rect100());
    let Some(Shader::LinearGradient { from, to, .. }) = &paint.shader else {
        panic!("a linear gradient must record a linear shader; got {paint:?}");
    };
    assert_eq!(*from, Offset::new(0.0, 25.0));
    assert_eq!(*to, Offset::new(100.0, 25.0));
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
fn circle_hit_test_center_inside_boundary_and_outside() {
    let decoration = BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle);
    let rect = square_rect();

    // Center: inside.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(50.0, 50.0)
    ));

    // center + (30, 40): a 30-40-50 Pythagorean triple, so this point sits
    // EXACTLY on the r=50 boundary. Circle::contains is inclusive (`<=`),
    // matching Flutter's `distance <= radius`.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(80.0, 90.0)
    ));

    // center + (29, 40): distance = sqrt(29^2 + 40^2) ~= 49.4 < 50 -- just
    // inside the circle.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(79.0, 90.0)
    ));

    // center + (31, 40): distance = sqrt(31^2 + 40^2) ~= 50.6 > 50 -- just
    // outside the circle, but still well within the 100x100 bounding rect,
    // so this exercises the circle branch itself, not the outer
    // `rect.contains` guard.
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(81.0, 90.0)
    ));
}

#[test]
fn circle_color_paints_a_circle_command_not_rect_or_rrect() {
    let decoration = BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle);
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        DrawOp::Circle { paint, .. } => {
            assert_eq!(paint.color, Color::RED);
        }
        other => panic!("expected DrawCircle, got {other:?}"),
    }
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

#[test]
fn circle_zero_size_and_negative_area_rects_do_not_panic() {
    let decoration = BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle);

    // (rect, expected geometry). `None` means the background pass records
    // nothing at all, which is the contract for a rect covering zero area
    // (see `paint_box_decoration`'s background guard). Pinning the resolved
    // geometry of the cases that DO paint, rather than only "did not panic",
    // is what actually exercises `shortest_side`'s `.abs()` branch -- an
    // assertion-free version of this test would pass even if the degenerate
    // cases silently produced a negative radius or a NaN center.
    let cases = [
        // The three zero-extent shapes cover no area, so the background is
        // not recorded — Flutter's `size > Size.zero` guard, applied to the
        // fill rather than to a caller.
        (Rect::from_ltrb(0.0, 0.0, 0.0, 0.0), None),
        (
            // Zero height, 100 wide.
            Rect::from_ltrb(0.0, 0.0, 100.0, 0.0),
            None,
        ),
        (
            // Zero width, 100 tall: symmetric to the above.
            Rect::from_ltrb(0.0, 0.0, 0.0, 100.0),
            None,
        ),
        (
            // Inverted (min > max on both axes): `width()`/`height()`
            // (`max - min`) are both -100, so this is the ONLY case here
            // that exercises `shortest_side`'s `.abs()` branch -- without
            // it this would resolve to a negative radius instead of the
            // r=50 an upright 100x100 rect produces.
            Rect::from_ltrb(100.0, 100.0, 0.0, 0.0),
            Some((Point::new(50.0, 50.0), 50.0)),
        ),
    ];

    for (rect, expected) in cases {
        let cmds = commands_in(rect, &decoration); // must not panic
        match (cmds.as_slice(), expected) {
            ([], None) => {}
            ([DrawOp::Circle { center, radius, .. }], Some((expected_center, expected_radius))) => {
                assert_eq!(*center, expected_center, "rect: {rect:?}");
                assert_eq!(*radius, expected_radius, "rect: {rect:?}");
            }
            (other, expected) => panic!("rect {rect:?}: expected {expected:?}, got {other:?}"),
        }
        // Must not panic on the hit-test path either.
        let _ = box_decoration_hit_test(rect, &decoration, Offset::new(0.0, 0.0));
    }
}

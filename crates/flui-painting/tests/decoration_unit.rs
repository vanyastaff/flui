//! `BoxDecoration` painter: Flutter's draw order (shadows → background
//! → image → border), gradient resolution against the paint rect, and
//! rounded-rect hit testing.
//!
//! All assertions are sans-IO over the recorded display list — the
//! same contract the fragment paint model relies on.

use flui_painting::{
    Canvas, DisplayListCore, DrawCommand, box_decoration_hit_test, paint_box_decoration,
    resolve_gradient,
};
use flui_types::{
    Offset, Pixels, Point,
    geometry::{Rect, px},
    layout::BoxShape,
    painting::{Image, PaintStyle, PathCommand, Shader},
    styling::{
        Border, BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, BoxShadow,
        Color, DecorationImage, Gradient, LinearGradient,
    },
};

fn rect100() -> Rect<Pixels> {
    Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0))
}

fn commands(decoration: &BoxDecoration<Pixels>) -> Vec<DrawCommand> {
    commands_in(rect100(), decoration)
}

/// Like [`commands`], but against a caller-supplied rect rather than the
/// fixed 100x50 `rect100()` — needed for the `BoxShape::Circle` cases,
/// which care about the rect's aspect ratio (the inscribed circle) and
/// about non-degenerate vs. degenerate sizes.
fn commands_in(rect: Rect<Pixels>, decoration: &BoxDecoration<Pixels>) -> Vec<DrawCommand> {
    let mut canvas = Canvas::new();
    paint_box_decoration(&mut canvas, rect, decoration);
    canvas.finish().commands().cloned().collect()
}

#[test]
fn color_only_paints_a_rect() {
    let cmds = commands(&BoxDecoration::with_color(Color::RED));
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], DrawCommand::DrawRect { .. }));
}

#[test]
fn radius_switches_to_rounded_primitives() {
    let decoration = BoxDecoration::with_color(Color::RED)
        .set_border_radius(Some(BorderRadius::circular(px(8.0))));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], DrawCommand::DrawRRect { .. }));
}

#[test]
fn flutter_paint_order_shadow_background_border() {
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            px(2.0),
            BorderStyle::Solid,
        ))))
        .set_box_shadow(Some(vec![BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(px(0.0), px(2.0)),
            blur_radius: px(4.0),
            spread_radius: px(1.0),
            inset: false,
        }]));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 3, "shadow + background + border");
    assert!(
        matches!(cmds[0], DrawCommand::DrawShadow { .. }),
        "shadows paint FIRST (behind everything)"
    );
    assert!(matches!(cmds[1], DrawCommand::DrawRect { .. }));
    assert!(
        matches!(cmds[2], DrawCommand::DrawDRRect { .. }),
        "a uniform border strokes inside via an outer/inner pair, LAST"
    );
}

#[test]
fn gradient_wins_over_color_and_resolves_alignment() {
    let gradient = Gradient::Linear(LinearGradient::new(
        flui_types::Alignment::CENTER_LEFT,
        flui_types::Alignment::CENTER_RIGHT,
        vec![Color::RED, Color::BLUE],
        None,
        flui_types::painting::TileMode::Clamp,
    ));
    let decoration = BoxDecoration::with_color(Color::WHITE).set_gradient(Some(gradient.clone()));
    let cmds = commands(&decoration);
    assert_eq!(
        cmds.len(),
        1,
        "a gradient replaces the flat color entirely (Flutter contract)"
    );
    assert!(matches!(cmds[0], DrawCommand::DrawGradient { .. }));

    // Default LinearGradient runs centerLeft → centerRight: alignment
    // resolves against the CONCRETE rect.
    let Shader::LinearGradient { from, to, .. } = resolve_gradient(&gradient, rect100()) else {
        panic!("linear gradient must resolve to a linear shader");
    };
    assert_eq!(from, Offset::new(px(0.0), px(25.0)));
    assert_eq!(to, Offset::new(px(100.0), px(25.0)));
}

#[test]
fn non_uniform_border_paints_per_side_rects() {
    let decoration = BoxDecoration::<Pixels>::new().set_border(Some(Border {
        top: Some(BorderSide::new(Color::RED, px(2.0), BorderStyle::Solid)),
        right: None,
        bottom: Some(BorderSide::new(Color::BLUE, px(4.0), BorderStyle::Solid)),
        left: None,
    }));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 2, "one rect per non-empty side");
    assert!(
        cmds.iter()
            .all(|c| matches!(c, DrawCommand::DrawRect { .. }))
    );
}

#[test]
fn hit_test_respects_rounded_corners() {
    let decoration = BoxDecoration::with_color(Color::RED)
        .set_border_radius(Some(BorderRadius::circular(px(20.0))));
    let rect = rect100();

    // Center: inside.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(50.0), px(25.0))
    ));
    // The exact corner of the BOUNDING rect lies outside the rounded
    // shape (radius 20 cuts it off).
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(1.0), px(1.0))
    ));
    // Just inside the corner arc.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(20.0), px(20.0))
    ));
    // Outside the rect entirely.
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(150.0), px(25.0))
    ));
}

// ============================================================================
// `BoxShape::Circle`
// ============================================================================

/// A square rect (100x100) so the inscribed circle has a clean radius
/// (50) and center (50, 50).
fn square_rect() -> Rect<Pixels> {
    Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0))
}

#[test]
fn circle_hit_test_center_inside_boundary_and_outside() {
    let decoration = BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle);
    let rect = square_rect();

    // Center: inside.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(50.0), px(50.0))
    ));

    // center + (30, 40): a 30-40-50 Pythagorean triple, so this point sits
    // EXACTLY on the r=50 boundary. Circle::contains is inclusive (`<=`),
    // matching Flutter's `distance <= radius`.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(80.0), px(90.0))
    ));

    // center + (29, 40): distance = sqrt(29^2 + 40^2) ~= 49.4 < 50 -- just
    // inside the circle.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(79.0), px(90.0))
    ));

    // center + (31, 40): distance = sqrt(31^2 + 40^2) ~= 50.6 > 50 -- just
    // outside the circle, but still well within the 100x100 bounding rect,
    // so this exercises the circle branch itself, not the outer
    // `rect.contains` guard.
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(81.0), px(90.0))
    ));
}

#[test]
fn circle_inscribes_in_the_shorter_side_and_centers() {
    // 200x100: shortest side is the height (100), so r=50, centered at
    // (100, 50) -- NOT an ellipse fit to both dimensions.
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(200.0), px(100.0));
    let decoration = BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle);
    let cmds = commands_in(rect, &decoration);

    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        DrawCommand::DrawCircle { center, radius, .. } => {
            assert_eq!(*center, Point::new(px(100.0), px(50.0)));
            assert_eq!(*radius, px(50.0));
        }
        other => panic!("expected DrawCircle, got {other:?}"),
    }
}

#[test]
fn circle_color_paints_a_circle_command_not_rect_or_rrect() {
    let decoration = BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle);
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        DrawCommand::DrawCircle { paint, .. } => {
            assert_eq!(paint.color, Color::RED);
        }
        other => panic!("expected DrawCircle, got {other:?}"),
    }
}

#[test]
fn circle_gradient_paints_a_circle_carrying_the_shader_and_stops() {
    let gradient = Gradient::Linear(LinearGradient::new(
        flui_types::Alignment::CENTER_LEFT,
        flui_types::Alignment::CENTER_RIGHT,
        vec![Color::RED, Color::BLUE],
        Some(vec![0.0, 1.0]),
        flui_types::painting::TileMode::Clamp,
    ));
    // `color` is set too, to prove the gradient (not the color) wins, same
    // as the rect-path `gradient_wins_over_color_and_resolves_alignment`.
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_gradient(Some(gradient))
        .set_shape(BoxShape::Circle);
    let cmds = commands(&decoration);

    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        DrawCommand::DrawCircle {
            center,
            radius,
            paint,
            ..
        } => {
            // rect100() is 100x50: shortest side 50, so r=25 centered at
            // (50, 25) -- catches a shape regression the shader match below
            // cannot.
            assert_eq!(*center, Point::new(px(50.0), px(25.0)));
            assert_eq!(*radius, px(25.0));
            // The wgpu shader dispatch (`dispatch_shader_rect`) is only
            // called when `paint.style == Fill`; a stroke paint carrying
            // the same shader would silently never reach it, so this must
            // be pinned alongside the shader/stops assertions below.
            assert_eq!(paint.style, PaintStyle::Fill);
            let shader = paint
                .shader
                .as_ref()
                .expect("a gradient circle must carry a shader, not just a flat color");
            match shader {
                Shader::LinearGradient { colors, stops, .. } => {
                    assert_eq!(colors, &vec![Color::RED, Color::BLUE]);
                    assert_eq!(stops, &Some(vec![0.0, 1.0]));
                }
                other => panic!("expected a LinearGradient shader, got {other:?}"),
            }
        }
        other => panic!("expected DrawCircle, got {other:?}"),
    }
}

#[test]
fn circle_gradient_without_stops_falls_back_to_transparent_like_the_rect_path() {
    // `LinearGradient::new` does not validate `colors`; an empty list
    // constructs fine and makes the wgpu backend's `dispatch_shader_rect`
    // return `false` (`gradients.rs`: `stops.is_empty()`), falling through
    // to a solid `paint.color` fill. The rect/rrect gradient paths
    // (`render_gradient` / `render_gradient_rrect`) early-return and paint
    // NOTHING for an empty color list, so the circle's fallback color must
    // be `Color::TRANSPARENT` to match -- `Color::BLACK` would make the
    // circle silently paint a solid black disc where its siblings paint
    // nothing.
    let gradient = Gradient::Linear(LinearGradient::new(
        flui_types::Alignment::CENTER_LEFT,
        flui_types::Alignment::CENTER_RIGHT,
        vec![],
        None,
        flui_types::painting::TileMode::Clamp,
    ));
    let decoration = BoxDecoration::with_color(Color::RED) // must not show through either
        .set_gradient(Some(gradient))
        .set_shape(BoxShape::Circle);
    let cmds = commands(&decoration);

    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        DrawCommand::DrawCircle { paint, .. } => {
            assert_eq!(paint.color, Color::TRANSPARENT);
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
            px(4.0),
            BorderStyle::Solid,
        ))));
    let cmds = commands_in(rect, &decoration);

    assert_eq!(cmds.len(), 2, "background fill + stroked border");
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, DrawCommand::DrawDRRect { .. })),
        "a circular border must stroke a circle, not fill a drrect ring \
         (tessellate_drrect bulges ~6% on the diagonals relative to the \
         exact circle fill); commands: {cmds:?}"
    );
    match &cmds[1] {
        DrawCommand::DrawCircle {
            center,
            radius,
            paint,
            ..
        } => {
            assert_eq!(*center, Point::new(px(50.0), px(50.0)));
            // Inside-stroke convention (matching the rect/rrect
            // draw_drrect(outer, outer.inflate(-width)) path): the
            // stroke's OUTER edge lands on the fill radius (50), so the
            // stroke is centered at 50 - 4/2 = 48.
            assert_eq!(*radius, px(48.0));
            assert_eq!(paint.style, PaintStyle::Stroke);
            assert_eq!(paint.stroke_width, 4.0);
            assert_eq!(paint.color, Color::BLACK);
        }
        other => panic!("expected a stroked DrawCircle border, got {other:?}"),
    }
}

#[test]
fn circle_border_exactly_at_the_diameter_still_paints_a_full_disc() {
    let rect = square_rect(); // r = 50, diameter = 100

    // A width equal to the diameter is the boundary case, and it IS
    // satisfiable: the stroke centers at `radius - width / 2 == 0` and a
    // stroke of width 100 there spans ±50, landing its outer edge exactly
    // on the r=50 fill. That renders a full disc in the border color --
    // the right answer for a border as thick as the shape. Skipping it
    // would silently drop the border color instead.
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_shape(BoxShape::Circle)
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            px(100.0),
            BorderStyle::Solid,
        ))));
    let cmds = commands_in(rect, &decoration);

    assert_eq!(
        cmds.len(),
        2,
        "background fill plus the border stroke; commands: {cmds:?}"
    );
    let DrawCommand::DrawCircle {
        center,
        radius,
        paint,
        ..
    } = &cmds[1]
    else {
        panic!("border must be a stroked circle, got {:?}", cmds[1]);
    };
    assert_eq!(*center, Point::new(px(50.0), px(50.0)));
    assert_eq!(radius.get(), 0.0, "stroke centers at radius - width / 2");
    assert_eq!(paint.stroke_width, 100.0, "full width is retained");
    assert_eq!(paint.color, Color::BLACK);
}

#[test]
fn circle_border_past_the_diameter_skips_instead_of_bulging_past_the_fill() {
    let rect = square_rect(); // r = 50, diameter = 100

    // Beyond the diameter the invariant is unsatisfiable: the stroke center
    // would need a negative radius, and pinning it at 0 while keeping the
    // full width puts the outer edge at `width / 2 > 50` -- a disc bulging
    // OUTSIDE the r=50 fill. Declining to paint beats painting that.
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_shape(BoxShape::Circle)
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            px(200.0),
            BorderStyle::Solid,
        ))));
    let cmds = commands_in(rect, &decoration);

    assert_eq!(
        cmds.len(),
        1,
        "only the background fill, no border stroke; commands: {cmds:?}"
    );
    assert!(matches!(cmds[0], DrawCommand::DrawCircle { .. }));
}

#[test]
fn circle_non_uniform_border_paints_nothing_not_a_square_frame() {
    let rect = square_rect();
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_shape(BoxShape::Circle)
        .set_border(Some(Border {
            top: Some(BorderSide::new(Color::RED, px(2.0), BorderStyle::Solid)),
            right: None,
            bottom: Some(BorderSide::new(Color::BLUE, px(4.0), BorderStyle::Solid)),
            left: None,
        }));
    let cmds = commands_in(rect, &decoration);

    // Background fill only -- the non-uniform border on a circle is
    // unimplemented (warns, paints nothing) and must NOT fall through to
    // the rect/rrect per-side strip painter, which would draw a square
    // frame around the circular fill.
    assert_eq!(cmds.len(), 1, "commands: {cmds:?}");
    assert!(matches!(cmds[0], DrawCommand::DrawCircle { .. }));
}

#[test]
fn circle_image_paints_unclipped_with_no_recorded_clip_command() {
    // Circular image clipping is UNIMPLEMENTED (see `paint_box_decoration`'s
    // image step): `Canvas::save()` never records anything and `restore()`
    // only closes a `save_layer()` scope (`is_layer`), so a `ClipRRect`
    // pushed here would never be closed -- it would leak onto the image and
    // every later sibling command in the same `PictureLayer`. Separately,
    // even a properly scoped `clip_rrect` would not clip an IMAGE: the
    // rounded-SDF clip only reaches a draw through
    // `GpuStateStack::apply_active_clip`, and the wgpu image batch never
    // calls it (only the `RectInstance` paths in `batches/shapes.rs` do) --
    // an image would only ever get the coarse bounding-box scissor, a
    // SQUARE, not a circle. So the honest behavior is: no clip command
    // recorded at all, and the image paints unclipped.
    let rect = square_rect(); // r = 50, side = 100
    let image = Image::from_rgba8(2, 2, vec![255; 2 * 2 * 4]);
    let decoration =
        BoxDecoration::with_image(DecorationImage::new(image)).set_shape(BoxShape::Circle);
    let cmds = commands_in(rect, &decoration);

    assert!(
        !cmds.iter().any(|c| matches!(
            c,
            DrawCommand::ClipRect { .. }
                | DrawCommand::ClipRRect { .. }
                | DrawCommand::ClipRSuperellipse { .. }
                | DrawCommand::ClipPath { .. }
        )),
        "a circular decoration image must not record ANY clip command until \
         circular image clipping is actually implemented (an unbalanced \
         save()/clip_rrect()/restore() pair would leak onto later commands, \
         and clip_rrect does not reach the image batch anyway); commands: {cmds:?}"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, DrawCommand::DrawImage { .. })),
        "the image must still be painted, unclipped; commands: {cmds:?}"
    );
}

#[test]
fn circle_shadow_spread_inflates_the_radius_translates_the_center_and_clamps_at_zero() {
    let rect = square_rect(); // r = 50, center (50, 50)
    // A non-zero offset: with `Offset::ZERO` (the previous fixture),
    // `Circle::translate` could be deleted from `paint_circle_shadow` and
    // this test would stay green, since the un-translated center is
    // identical.
    let decoration = BoxDecoration::with_color(Color::RED)
        .set_shape(BoxShape::Circle)
        .set_box_shadow(Some(vec![BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(px(5.0), px(-3.0)),
            blur_radius: px(4.0),
            spread_radius: px(10.0),
            inset: false,
        }]));
    let cmds = commands_in(rect, &decoration);

    let DrawCommand::DrawShadow {
        path, elevation, ..
    } = &cmds[0]
    else {
        panic!("expected DrawShadow first (shadows paint behind everything); commands: {cmds:?}");
    };
    assert_eq!(
        *elevation, 4.0,
        "blur_radius must ride as the shadow's elevation input"
    );
    match path.commands() {
        [PathCommand::AddOval(oval)] => {
            // r + spread = 50 + 10 = 60 -> bounding square side 120,
            // centered at (50 + 5, 50 - 3) = (55, 47) after the offset
            // translate.
            assert_eq!(
                *oval,
                Rect::from_ltrb(px(-5.0), px(-13.0), px(115.0), px(107.0))
            );
        }
        other => panic!("expected a single AddOval command, got {other:?}"),
    }

    // A spread radius that would drive the radius negative clamps to 0
    // (`Circle::inflate`'s clamp) instead of panicking in
    // `Canvas::draw_shadow` / `Canvas::draw_circle`.
    let inverting = BoxDecoration::with_color(Color::RED)
        .set_shape(BoxShape::Circle)
        .set_box_shadow(Some(vec![BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(px(0.0), px(0.0)),
            blur_radius: px(4.0),
            spread_radius: px(-1000.0),
            inset: false,
        }]));
    let cmds = commands_in(rect, &inverting); // must not panic
    let DrawCommand::DrawShadow { path, .. } = &cmds[0] else {
        panic!("expected DrawShadow first; commands: {cmds:?}");
    };
    match path.commands() {
        [PathCommand::AddOval(oval)] => {
            assert_eq!(
                *oval,
                Rect::from_ltrb(px(50.0), px(50.0), px(50.0), px(50.0)),
                "radius clamped to 0: a zero-size oval centered on the circle's own center"
            );
        }
        other => panic!("expected a single AddOval command, got {other:?}"),
    }
}

#[test]
fn circle_ignores_border_radius_and_still_paints_the_circle() {
    let rect = square_rect();
    let decoration = BoxDecoration::with_color(Color::RED)
        .set_shape(BoxShape::Circle)
        .set_border_radius(Some(BorderRadius::circular(px(20.0))));
    let cmds = commands_in(rect, &decoration);

    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        DrawCommand::DrawCircle { center, radius, .. } => {
            // The plain inscribed circle (r=50, centered) -- completely
            // unaffected by the ignored `border_radius: 20`. A variant-only
            // check would also pass if the radius silently changed.
            assert_eq!(*center, Point::new(px(50.0), px(50.0)));
            assert_eq!(*radius, px(50.0));
        }
        other => panic!("expected DrawCircle, got {other:?}"),
    }
}

#[test]
fn circle_zero_size_and_negative_area_rects_do_not_panic() {
    let decoration = BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle);

    // (rect, expected center, expected radius). Pinning the resolved
    // geometry (not just "did not panic") is what actually exercises
    // `shortest_side`'s `.abs()` branch -- an assertion-free version of
    // this test would pass even if the degenerate cases silently produced
    // a negative radius or NaN center.
    let cases = [
        (
            Rect::from_ltrb(px(0.0), px(0.0), px(0.0), px(0.0)),
            Point::new(px(0.0), px(0.0)),
            px(0.0),
        ),
        (
            // Zero height: shortest_side is 0 regardless of the 100-wide side.
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(0.0)),
            Point::new(px(50.0), px(0.0)),
            px(0.0),
        ),
        (
            // Zero width: symmetric to the above.
            Rect::from_ltrb(px(0.0), px(0.0), px(0.0), px(100.0)),
            Point::new(px(0.0), px(50.0)),
            px(0.0),
        ),
        (
            // Inverted (min > max on both axes): `width()`/`height()`
            // (`max - min`) are both -100, so this is the ONLY case here
            // that exercises `shortest_side`'s `.abs()` branch -- without
            // it this would resolve to a negative radius instead of the
            // r=50 an upright 100x100 rect produces.
            Rect::from_ltrb(px(100.0), px(100.0), px(0.0), px(0.0)),
            Point::new(px(50.0), px(50.0)),
            px(50.0),
        ),
    ];

    for (rect, expected_center, expected_radius) in cases {
        let cmds = commands_in(rect, &decoration); // must not panic
        match cmds.as_slice() {
            [DrawCommand::DrawCircle { center, radius, .. }] => {
                assert_eq!(*center, expected_center, "rect: {rect:?}");
                assert_eq!(*radius, expected_radius, "rect: {rect:?}");
            }
            other => panic!("expected a single DrawCircle, got {other:?}; rect: {rect:?}"),
        }
        // Must not panic on the hit-test path either.
        let _ = box_decoration_hit_test(rect, &decoration, Offset::new(px(0.0), px(0.0)));
    }
}

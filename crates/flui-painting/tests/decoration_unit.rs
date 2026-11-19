//! `BoxDecoration` painter: draw order (shadows → background
//! → image → border), gradient resolution against the paint rect, and
//! rounded-rect hit testing.
//!
//! All assertions are sans-IO over the recorded display list — the
//! same contract the fragment paint model relies on.

use flui_foundation::geometry::{Offset, Point, Rect};
use flui_painting::BoxShape;
use flui_painting::{Canvas, DrawOp, styling::DecorationPaintOptions};
use flui_painting::{
    paint::PaintStyle,
    styling::{
        Border, BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, BoxShadow,
        Color, TableBorder,
    },
};

fn rect100() -> Rect<f64> {
    Rect::from_ltrb(0.0, 0.0, 100.0, 50.0)
}

fn commands(decoration: &BoxDecoration) -> Vec<DrawOp> {
    commands_in(rect100(), decoration)
}

fn commands_in(rect: Rect<f64>, decoration: &BoxDecoration) -> Vec<DrawOp> {
    let mut canvas = Canvas::new();
    decoration.paint(&mut canvas, rect, DecorationPaintOptions::default());
    canvas.finish().iter().map(|c| c.op.clone()).collect()
}

fn hidden_uniform_border(shape: BoxShape) {
    let hidden = BorderSide::new(Color::RED, 5.0, BorderStyle::None);
    let decoration = BoxDecoration::new()
        .set_shape(shape)
        .set_border(Some(Border::all(hidden)));
    assert!(
        commands_in(square_rect(), &decoration).is_empty(),
        "{shape:?}"
    );
}

pub(crate) fn hidden_uniform_rectangle_borders_do_not_paint() {
    hidden_uniform_border(BoxShape::Rectangle);
}

pub(crate) fn hidden_uniform_circle_borders_do_not_paint() {
    hidden_uniform_border(BoxShape::Circle);
}

pub(crate) fn hidden_table_borders_do_not_paint() {
    let hidden = BorderSide::new(Color::RED, 5.0, BorderStyle::None);
    let mut canvas = Canvas::new();
    TableBorder::all(hidden).paint(&mut canvas, rect100(), &[25.0], &[50.0]);
    assert!(
        canvas.finish().is_empty(),
        "hidden interior and outer table borders"
    );
}

pub(crate) fn hidden_edges_do_not_shorten_visible_neighboring_edges() {
    let decoration = BoxDecoration::new().set_border(Some(Border {
        top: Some(BorderSide::new(Color::RED, 10.0, BorderStyle::None)),
        bottom: Some(BorderSide::new(Color::BLUE, 12.0, BorderStyle::None)),
        left: Some(BorderSide::new(Color::GREEN, 3.0, BorderStyle::Solid)),
        right: None,
    }));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 1, "only the visible left side paints: {cmds:?}");
    match &cmds[0] {
        DrawOp::Rect { rect, paint } => {
            assert_eq!(*rect, Rect::from_ltrb(0.0, 0.0, 3.0, 50.0));
            assert_eq!(paint.color, Color::GREEN);
        }
        other => panic!("expected a full-height left strip, got {other:?}"),
    }
}

pub(crate) fn paint_order_is_shadow_background_border() {
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

pub(crate) fn hit_test_respects_rounded_corners() {
    let decoration =
        BoxDecoration::with_color(Color::RED).set_border_radius(Some(BorderRadius::circular(20.0)));
    let rect = rect100();

    // Center: inside.
    assert!(decoration.hit_test(rect, Offset::new(50.0, 25.0)));
    // The exact corner of the BOUNDING rect lies outside the rounded
    // shape (radius 20 cuts it off).
    assert!(!decoration.hit_test(rect, Offset::new(1.0, 1.0)));
    // Just inside the corner arc.
    assert!(decoration.hit_test(rect, Offset::new(20.0, 20.0)));
    // Outside the rect entirely.
    assert!(!decoration.hit_test(rect, Offset::new(150.0, 25.0)));
}

// ============================================================================
// `BoxShape::Circle`
// ============================================================================

/// A square rect (100x100) so the inscribed circle has a clean radius
/// (50) and center (50, 50).
fn square_rect() -> Rect<f64> {
    Rect::from_ltrb(0.0, 0.0, 100.0, 100.0)
}

pub(crate) fn circular_hit_testing_preserves_finite_distance_ranges() {
    let decoration = BoxDecoration::new().set_shape(BoxShape::Circle);
    for radius in [
        1.0,
        2.0_f64.powi(600),
        2.0_f64.powi(-600),
        f64::MIN_POSITIVE,
    ] {
        let rect = Rect::from_ltrb(-radius, -radius, radius, radius);
        // These relative positions have fixed geometric meanings at every scale:
        // the center and half-radius diagonal are inside; the three-quarter
        // diagonal is outside the inscribed circle but inside the paint rect.
        for (position, expected) in [
            (Offset::new(0.0, 0.0), true),
            (Offset::new(radius * 0.5, radius * 0.5), true),
            (Offset::new(radius, 0.0), true),
            (Offset::new(radius * 0.75, radius * 0.75), false),
        ] {
            assert_eq!(
                decoration.hit_test(rect, position),
                expected,
                "radius {radius}, position {position:?}"
            );
        }
    }
}

pub(crate) fn circle_uniform_border_is_a_stroked_circle_not_a_drrect() {
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

/// A lerped decoration crossfades its images: at `t = 0.25` it paints `a`'s at
/// 75% opacity, then `b`'s over it at 25%. A lone image fades on its own, and
/// replacing the lerped decoration's image drops the image it was fading out.
pub(crate) fn a_lerped_decoration_crossfades_its_images() {
    use flui_painting::paint::Image;
    use flui_painting::styling::DecorationImage;

    let red = Image::solid_color(4, 4, Color::RED);
    let blue = Image::solid_color(4, 4, Color::BLUE);
    let with = |image: &Image| BoxDecoration::with_image(DecorationImage::new(image.clone()));
    // Each image the decoration paints, in order, with its paint's opacity.
    let painted = |decoration: &BoxDecoration| -> Vec<(Image, f32)> {
        commands(decoration)
            .into_iter()
            .filter_map(|op| match op {
                DrawOp::Image { image, paint, .. }
                | DrawOp::ImageRegion { image, paint, .. }
                | DrawOp::ImageRepeat { image, paint, .. } => {
                    Some((image, paint.map_or(1.0, |paint| paint.color.opacity())))
                }
                _ => None,
            })
            .collect()
    };
    let near = |actual: f32, expected: f32| (actual - expected).abs() < 0.01;

    let crossfaded = BoxDecoration::lerp(&with(&red), &with(&blue), 0.25);
    let images = painted(&crossfaded);
    assert_eq!(images.len(), 2, "both images are painted: {images:?}");
    assert!(
        images[0].0 == red && near(images[0].1, 0.75),
        "a's image first, at 75%: {:?}",
        images[0].1
    );
    assert!(
        images[1].0 == blue && near(images[1].1, 0.25),
        "b's image over it, at 25%: {:?}",
        images[1].1
    );

    let fading = painted(&BoxDecoration::lerp(
        &with(&red),
        &BoxDecoration::new(),
        0.25,
    ));
    assert!(
        fading.len() == 1 && fading[0].0 == red && near(fading[0].1, 0.75),
        "a lone image fades out on its own: {fading:?}"
    );

    let mut replaced = crossfaded;
    replaced.image = Some(DecorationImage::new(blue.clone()));
    let images = painted(&replaced);
    assert!(
        images.len() == 1 && images[0].0 == blue && near(images[0].1, 1.0),
        "a replaced image is painted alone: {images:?}"
    );
}

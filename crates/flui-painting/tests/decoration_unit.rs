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

/// A lerped decoration crossfades its images at constant coverage: at
/// `t = 0.25` it paints `a`'s image at 75% and adds `b`'s at 25% in a layer of
/// their own. A lone image fades in or out over the background, at its own
/// opacity times its share; the same image on both sides stays whole; a lerp
/// from a lerped decoration keeps the image that one was fading out; and
/// replacing the lerped decoration's image paints the new image alone.
pub(crate) fn a_lerped_decoration_crossfades_its_images() {
    use flui_painting::paint::{BlendMode, Image};
    use flui_painting::styling::DecorationImage;

    let red = Image::solid_color(4, 4, Color::RED);
    let blue = Image::solid_color(4, 4, Color::BLUE);
    let green = Image::solid_color(4, 4, Color::GREEN);
    let with = |image: &Image| BoxDecoration::with_image(DecorationImage::new(image.clone()));
    // Each image the decoration paints, in order, with its opacity and blend.
    let painted = |decoration: &BoxDecoration| -> Vec<(Image, f32, BlendMode)> {
        commands(decoration)
            .into_iter()
            .filter_map(|op| match op {
                DrawOp::Image { image, paint, .. }
                | DrawOp::ImageRegion { image, paint, .. }
                | DrawOp::ImageRepeat { image, paint, .. } => Some((
                    image,
                    paint.as_ref().map_or(1.0, |paint| paint.color.opacity()),
                    paint.map_or(BlendMode::SrcOver, |paint| paint.blend_mode),
                )),
                _ => None,
            })
            .collect()
    };
    let near = |actual: f32, expected: f32| (actual - expected).abs() < 0.01;
    let layers = |decoration: &BoxDecoration| {
        commands(decoration)
            .iter()
            .filter(|op| matches!(op, DrawOp::SaveLayer { .. }))
            .count()
    };

    let crossfaded = BoxDecoration::lerp(&with(&red), &with(&blue), 0.25);
    let images = painted(&crossfaded);
    assert_eq!(images.len(), 2, "both images are painted: {images:?}");
    assert!(
        images[0].0 == red && near(images[0].1, 0.75) && images[0].2 == BlendMode::SrcOver,
        "a's image first, at 75%: {:?}",
        images[0]
    );
    assert!(
        images[1].0 == blue && near(images[1].1, 0.25) && images[1].2 == BlendMode::Plus,
        "b's image added to it, at 25%: {:?}",
        images[1]
    );
    assert_eq!(layers(&crossfaded), 1, "the blend has a layer of its own");

    let fading_in = painted(&BoxDecoration::lerp(
        &BoxDecoration::new(),
        &with(&red),
        0.25,
    ));
    assert!(
        fading_in.len() == 1 && near(fading_in[0].1, 0.25) && fading_in[0].2 == BlendMode::SrcOver,
        "a lone image fades in over the background: {fading_in:?}"
    );
    let half = BoxDecoration::with_image(DecorationImage {
        opacity: 0.5,
        ..DecorationImage::new(red.clone())
    });
    let fading_out = painted(&BoxDecoration::lerp(&half, &BoxDecoration::new(), 0.5));
    assert!(
        fading_out.len() == 1 && near(fading_out[0].1, 0.25),
        "a lone image fades out from its own opacity: {fading_out:?}"
    );

    let recolored = BoxDecoration::lerp(
        &with(&red).set_color(Some(Color::RED)),
        &with(&red).set_color(Some(Color::BLUE)),
        0.5,
    );
    let images = painted(&recolored);
    assert!(
        images.len() == 1 && near(images[0].1, 1.0) && layers(&recolored) == 0,
        "the same image on both sides stays whole: {images:?}"
    );

    let retargeted = BoxDecoration::lerp(&crossfaded, &with(&green), 0.5);
    let images = painted(&retargeted);
    let shares: Vec<(bool, bool, bool, f32)> = images
        .iter()
        .map(|(image, opacity, _)| (*image == red, *image == blue, *image == green, *opacity))
        .collect();
    assert!(
        images.len() == 3
            && images[0].0 == red
            && near(images[0].1, 0.375)
            && images[1].0 == blue
            && near(images[1].1, 0.125)
            && images[2].0 == green
            && near(images[2].1, 0.5),
        "a lerp from a blend keeps the image it was fading out: {shares:?}"
    );

    // Back to an image the blend still holds: its two shares are one layer.
    let returned = painted(&BoxDecoration::lerp(&crossfaded, &with(&red), 0.5));
    assert!(
        returned.len() == 2
            && returned[0].0 == red
            && near(returned[0].1, 0.875)
            && returned[1].0 == blue
            && near(returned[1].1, 0.125),
        "an image on both sides of a lerp is one layer: {returned:?}"
    );

    let circle = BoxDecoration::lerp(
        &with(&red).set_shape(BoxShape::Circle),
        &with(&blue).set_shape(BoxShape::Circle),
        0.5,
    );
    let ops = commands_in(square_rect(), &circle);
    let opens = ops
        .iter()
        .filter(|op| matches!(op, DrawOp::Save | DrawOp::SaveLayer { .. }))
        .count();
    let closes = ops
        .iter()
        .filter(|op| matches!(op, DrawOp::Restore | DrawOp::RestoreLayer))
        .count();
    assert!(
        painted(&circle).len() == 2 && opens == 2 && opens == closes,
        "a circle's blend sits in its clip, every scope closed: {ops:?}"
    );

    let mut replaced = crossfaded;
    replaced.image = Some(DecorationImage::new(blue.clone()));
    let images = painted(&replaced);
    assert!(
        images.len() == 1 && images[0].0 == blue && near(images[0].1, 1.0),
        "a replaced image is painted alone: {images:?}"
    );
}

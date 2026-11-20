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
    let scopes: Vec<&str> = commands_in(square_rect(), &circle)
        .iter()
        .filter_map(|op| match op {
            DrawOp::Save => Some("save"),
            DrawOp::ClipRRect { .. } => Some("clip"),
            DrawOp::SaveLayer { .. } => Some("layer"),
            DrawOp::Image { .. } | DrawOp::ImageRegion { .. } | DrawOp::ImageRepeat { .. } => {
                Some("image")
            }
            DrawOp::RestoreLayer => Some("end layer"),
            DrawOp::Restore => Some("restore"),
            _ => None,
        })
        .collect();
    assert_eq!(
        scopes,
        [
            "save",
            "clip",
            "layer",
            "image",
            "image",
            "end layer",
            "restore"
        ],
        "a circle's blend sits in a layer inside its clip"
    );

    // The published image is b's at its share, also when the blend still holds
    // a's.
    assert!(
        crossfaded
            .image
            .as_ref()
            .is_some_and(|image| image.image == blue && (image.opacity - 0.25).abs() < 1e-9),
        "the published image is b's at 25%: {:?}",
        crossfaded.image.as_ref().map(|image| image.opacity)
    );
    let back_to_red = BoxDecoration::lerp(&crossfaded, &with(&red), 0.5);
    assert!(
        back_to_red
            .image
            .as_ref()
            .is_some_and(|image| image.image == red && (image.opacity - 0.875).abs() < 1e-9),
        "the published image is b's, merged with a's share of it"
    );

    // An endpoint whose layers are the target's keeps its blend as it is.
    let tinted = BoxDecoration::lerp(
        &crossfaded.clone().set_color(Some(Color::RED)),
        &crossfaded.clone().set_color(Some(Color::BLUE)),
        // A share where `0.75 * (1 - t) + 0.75 * t` rounds away from 0.75.
        0.002,
    );
    assert_eq!(
        tinted.image, crossfaded.image,
        "the same layers keep their image exactly"
    );
    assert_eq!(painted(&tinted), painted(&crossfaded), "and their blend");
    assert_eq!(
        BoxDecoration::lerp(&crossfaded, &crossfaded, 0.002),
        crossfaded,
        "a lerp between equal decorations is that decoration"
    );

    // A layer with no opacity is no layer.
    let invisible = BoxDecoration::with_image(DecorationImage {
        opacity: 0.0,
        ..DecorationImage::new(red.clone())
    });
    let images = painted(&BoxDecoration::lerp(&invisible, &with(&blue), 0.5));
    assert!(
        images.len() == 1
            && images[0].0 == blue
            && layers(&BoxDecoration::lerp(&invisible, &with(&blue), 0.5)) == 0,
        "an image with no opacity adds no layer: {images:?}"
    );

    // With no image on b's side, the published image is the strongest layer.
    let fading_blend = BoxDecoration::lerp(&crossfaded, &BoxDecoration::new(), 0.5);
    assert!(
        fading_blend
            .image
            .as_ref()
            .is_some_and(|image| image.image == red),
        "the strongest layer is published when b has no image"
    );

    // The strongest layer, not the first: blue leads a three-to-one blend.
    let blue_heavy = BoxDecoration::lerp(&with(&red), &with(&blue), 0.75);
    let fading_heavy = BoxDecoration::lerp(&blue_heavy, &BoxDecoration::new(), 0.5);
    assert!(
        fading_heavy
            .image
            .as_ref()
            .is_some_and(|image| image.image == blue),
        "the strongest layer, not the first, is published"
    );

    // A NaN `t` adds no layer rather than one of unknown opacity.
    let undefined = painted(&BoxDecoration::lerp(
        &with(&red),
        &BoxDecoration::new(),
        f64::NAN,
    ));
    assert!(
        undefined.is_empty(),
        "a NaN share paints no image: {undefined:?}"
    );

    // b publishes the same image as a blend but holds no blend: the two are
    // mixed, not taken as equal, so a's layers fade to half.
    let quarter_blue = BoxDecoration::with_image(DecorationImage {
        opacity: 0.25,
        ..DecorationImage::new(blue.clone())
    });
    let halved = painted(&BoxDecoration::lerp(&crossfaded, &quarter_blue, 0.5));
    assert!(
        halved.len() == 2 && halved[0].0 == red && near(halved[0].1, 0.375),
        "the same published image over a different blend is mixed: {halved:?}"
    );

    // A blend whose image was replaced is stale: an interior lerp toward the
    // same new image drops it, and writing the old published image back
    // paints that image alone.
    let mut stale = crossfaded.clone();
    stale.image = Some(DecorationImage::new(green.clone()));
    let mut through = BoxDecoration::lerp(&stale, &with(&green), 0.3);
    through.image.clone_from(&crossfaded.image);
    assert_eq!(
        painted(&through).len(),
        1,
        "a stale blend does not come back through a lerp"
    );
    // Nor through a new gradient or an extrapolated endpoint: both drop it.
    assert_eq!(
        stale.clone().set_gradient(None),
        with(&green),
        "a new gradient drops a stale blend"
    );
    assert_eq!(
        BoxDecoration::lerp(&stale, &with(&blue), -1.0),
        with(&green),
        "an extrapolated endpoint drops a stale blend"
    );

    // b's image invisible and in no layer of a's: the strongest layer is
    // published instead.
    let unseen = BoxDecoration::with_image(DecorationImage {
        opacity: 0.0,
        ..DecorationImage::new(green.clone())
    });
    let toward_unseen = BoxDecoration::lerp(&crossfaded, &unseen, 0.5);
    assert!(
        toward_unseen
            .image
            .as_ref()
            .is_some_and(|image| image.image == red),
        "an invisible target leaves the strongest layer published"
    );

    // An opacity past one counts as one, as paint draws it, so the shares
    // still add up to full coverage.
    let overdriven = BoxDecoration::with_image(DecorationImage {
        opacity: 2.0,
        ..DecorationImage::new(red.clone())
    });
    let images = painted(&BoxDecoration::lerp(&overdriven, &with(&blue), 0.5));
    assert!(
        images.len() == 2 && near(images[0].1, 0.5) && near(images[1].1, 0.5),
        "an overdriven image takes its share of one: {images:?}"
    );

    // An infinite opacity counts as one, as paint draws it.
    let boundless = BoxDecoration::with_image(DecorationImage {
        opacity: f64::INFINITY,
        ..DecorationImage::new(red.clone())
    });
    let images = painted(&BoxDecoration::lerp(&boundless, &BoxDecoration::new(), 0.5));
    assert!(
        images.len() == 1 && near(images[0].1, 0.5),
        "an infinite opacity fades as an opaque image: {images:?}"
    );

    // When b is itself a blend, the published image is b's own, not its last
    // layer: back_to_red publishes red, though blue is its last layer.
    let onto_blend = BoxDecoration::lerp(&with(&green), &back_to_red, 0.5);
    assert!(
        onto_blend
            .image
            .as_ref()
            .is_some_and(|image| image.image == red),
        "b's own published image is published: {:?}",
        onto_blend.image.as_ref().map(|image| image.opacity)
    );

    // Four images blended evenly, then lerped a little toward back_to_red:
    // six layers, b's own red among the weakest and blue weaker still, after
    // it. The cap keeps red, drops blue and the weakest of the four, and the
    // kept layers still cover the box.
    let even: Vec<Image> = [Color::GREEN, Color::BLACK, Color::WHITE, Color::TRANSPARENT]
        .iter()
        .map(|color| Image::solid_color(4, 4, *color))
        .collect();
    let mut four = with(&even[0]);
    for (index, image) in even.iter().enumerate().skip(1) {
        #[expect(clippy::cast_precision_loss, reason = "a share of four images")]
        let t = 1.0 / (index as f64 + 1.0);
        four = BoxDecoration::lerp(&four, &with(image), t);
    }
    let toward_red = BoxDecoration::lerp(&four, &back_to_red, 0.1);
    let kept = painted(&toward_red);
    let coverage: f32 = kept.iter().map(|(_, opacity, _)| opacity).sum();
    assert!(
        kept.len() == 4
            && kept.iter().any(|(image, _, _)| *image == red)
            && !kept.iter().any(|(image, _, _)| *image == blue)
            && (coverage - 1.0).abs() < 0.01
            && toward_red
                .image
                .as_ref()
                .is_some_and(|image| image.image == red),
        "b's own image kept and published, a later weaker layer dropped: {:?}",
        kept.iter()
            .map(|(_, opacity, _)| opacity)
            .collect::<Vec<_>>()
    );

    // A blend keeps its images when its gradient is replaced.
    let gradient =
        flui_painting::styling::Gradient::Linear(flui_painting::styling::LinearGradient::new(
            flui_painting::Alignment::CENTER_LEFT,
            flui_painting::Alignment::CENTER_RIGHT,
            vec![Color::RED, Color::BLUE],
            None,
            flui_painting::paint::TileMode::Clamp,
        ));
    let regraded = crossfaded.clone().set_gradient(Some(gradient));
    assert_eq!(
        painted(&regraded).len(),
        2,
        "the blend outlives a new gradient"
    );

    // Past four layers the weakest other one goes, the incoming image stays,
    // and the kept layers still cover the box.
    let colors = [
        Color::RED,
        Color::BLUE,
        Color::GREEN,
        Color::BLACK,
        Color::WHITE,
    ];
    let images: Vec<Image> = colors
        .iter()
        .map(|color| Image::solid_color(4, 4, *color))
        .collect();
    let mut chained = with(&images[0]);
    for (index, image) in images.iter().enumerate().skip(1) {
        let t = if index == images.len() - 1 { 0.1 } else { 0.5 };
        chained = BoxDecoration::lerp(&chained, &with(image), t);
    }
    let kept = painted(&chained);
    let coverage: f32 = kept.iter().map(|(_, opacity, _)| opacity).sum();
    assert!(
        kept.len() == 4
            && kept.iter().any(|(image, _, _)| *image == images[4])
            && (coverage - 1.0).abs() < 0.01,
        "four layers, the incoming one among them, still covering the box: {:?}",
        kept.iter()
            .map(|(_, opacity, _)| opacity)
            .collect::<Vec<_>>()
    );

    let mut replaced = crossfaded;
    replaced.image = Some(DecorationImage::new(blue.clone()));
    let images = painted(&replaced);
    assert!(
        images.len() == 1 && images[0].0 == blue && near(images[0].1, 1.0),
        "a replaced image is painted alone: {images:?}"
    );
}

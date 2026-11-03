//! A text style's paint attributes reach the display list: span
//! backgrounds, the shadows spans cast and their blur (ADR-0309).

use std::sync::Arc;

use flui_foundation::geometry::{Offset, Rect};
use flui_painting::styling::{Color, Shadow};
use flui_painting::typography::{TextDirection, TextSpan, TextStyle};
use flui_painting::{
    BlurSigma, Canvas, DamageExtent, DisplayList, DrawOp, FontCollection, Invalidation,
    ShapedParagraph, TextContext, TextPainter,
};

/// What one recorded op paints, in the order a paragraph records them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Painted {
    Background(Color),
    Shadow(Color),
    Text,
}

fn base() -> TextStyle {
    TextStyle::default().with_font_size(20.0)
}

fn laid_out(fonts: &FontCollection, painter: TextPainter, max_width: f64) -> TextPainter {
    let mut painter = painter.with_text_direction(TextDirection::Ltr);
    painter
        .layout(&mut TextContext::new(fonts), 0.0, max_width)
        .expect("valid fixture lays out");
    painter
}

fn painted(painter: &TextPainter) -> DisplayList {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    canvas.finish()
}

fn sequence(list: &DisplayList) -> Vec<Painted> {
    list.iter()
        .map(|command| match &command.op {
            DrawOp::Rect { paint, .. } => Painted::Background(paint.color),
            DrawOp::ParagraphShadow { color, .. } => Painted::Shadow(*color),
            DrawOp::Paragraph { .. } => Painted::Text,
            other => panic!("a paragraph records only its own ops, found {other:?}"),
        })
        .collect::<Vec<Painted>>()
}

fn text_paragraph(list: &DisplayList) -> Arc<ShapedParagraph> {
    list.iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter records its paragraph")
}

/// Each recorded shadow: its glyphs, offset and sigma.
fn shadows(list: &DisplayList) -> Vec<(Arc<ShapedParagraph>, Offset<f64>, BlurSigma)> {
    list.iter()
        .filter_map(|command| match &command.op {
            DrawOp::ParagraphShadow {
                paragraph,
                offset,
                sigma,
                ..
            } => Some((Arc::clone(paragraph), *offset, *sigma)),
            _ => None,
        })
        .collect::<Vec<(Arc<ShapedParagraph>, Offset<f64>, BlurSigma)>>()
}

fn backgrounds(list: &DisplayList) -> Vec<Rect<f64>> {
    list.iter()
        .filter_map(|command| match &command.op {
            DrawOp::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect::<Vec<Rect<f64>>>()
}

/// Every glyph's pen position, run by run.
fn pens(paragraph: &ShapedParagraph) -> Vec<f32> {
    paragraph
        .runs()
        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.x))
        .collect::<Vec<f32>>()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// A shadowed span, a span differing from it only in paint (so the two
/// share a glyph run) and a coloured span: each span's background fills its
/// own glyphs, and each shadow carries exactly the glyphs of the span that
/// casts it, in list order, before the glyphs themselves.
pub(crate) fn span_shadows_and_backgrounds_reach_the_display_list() {
    let fonts = FontCollection::new();
    let near = Shadow::new(Color::RED, Offset::new(2.0, 3.0), 4.0);
    let sharp = Shadow::new(Color::BLUE, Offset::new(-1.0, 0.0), 0.0);
    let cast = TextStyle {
        background_color: Some(Color::YELLOW),
        shadows: Some(vec![near, sharp]),
        ..base()
    };
    let paint_only = TextStyle {
        background_color: Some(Color::GREEN),
        ..base()
    };
    let text = TextSpan::new("")
        .with_child(TextSpan::styled("AB", cast))
        .with_child(TextSpan::styled("CD", paint_only))
        .with_child(TextSpan::styled("EF", base().with_color(Color::BLUE)));
    let painter = laid_out(&fonts, TextPainter::new().with_text(text), 400.0);
    let list = painted(&painter);

    assert_eq!(
        sequence(&list),
        [
            Painted::Background(Color::YELLOW),
            Painted::Background(Color::GREEN),
            Painted::Shadow(Color::RED),
            Painted::Shadow(Color::BLUE),
            Painted::Text,
        ],
        "backgrounds, then each shadow in list order, then the glyphs"
    );

    let glyphs = pens(&text_paragraph(&list));
    assert_eq!(glyphs.len(), 6, "one glyph per letter: {glyphs:?}");
    let recorded = shadows(&list);
    let (silhouette, offset, sigma) = &recorded[0];
    assert_eq!(
        pens(silhouette),
        glyphs[..2],
        "the shadow carries the shadowed span's glyphs, where they are painted"
    );
    assert!(
        Arc::ptr_eq(silhouette, &recorded[1].0),
        "one silhouette serves every shadow of a list"
    );
    assert_eq!(
        *offset,
        Offset::new(2.0, 3.0),
        "the shadow is offset from the text"
    );
    assert_eq!(Some(*sigma), BlurSigma::from_blur_radius(4.0));
    assert_eq!(recorded[1].1, Offset::new(-1.0, 0.0));
    assert_eq!(
        recorded[1].2,
        BlurSigma::ZERO,
        "a zero blur radius is sharp"
    );
    assert!(
        silhouette
            .runs()
            .all(|run| run.glyphs().iter().all(|glyph| glyph.id != 0)),
        "the silhouette keeps the glyphs it was shaped with"
    );

    let rects = backgrounds(&list);
    assert_eq!(rects.len(), 2, "one box per span background: {rects:?}");
    let (yellow, green) = (rects[0], rects[1]);
    assert!(
        close(yellow.left(), 0.0) && close(yellow.right(), f64::from(glyphs[2])),
        "the yellow box spans `AB`: {yellow:?}, pens {glyphs:?}"
    );
    assert!(
        close(green.left(), yellow.right()) && close(green.right(), f64::from(glyphs[4])),
        "the green box spans `CD`: {green:?}, pens {glyphs:?}"
    );
    assert!(
        close(yellow.top(), 0.0) && close(yellow.bottom(), painter.height()),
        "a background is as tall as its line: {yellow:?}, height {}",
        painter.height()
    );
}

/// An ellipsis takes the style of the span it follows: it casts that span's
/// shadow and sits on its background.
pub(crate) fn an_ellipsis_takes_its_spans_shadow_and_background() {
    let fonts = FontCollection::new();
    let shadowed = TextStyle {
        background_color: Some(Color::YELLOW),
        shadows: Some(vec![Shadow::new(Color::RED, Offset::new(1.0, 1.0), 0.0)]),
        ..base()
    };
    let text = TextSpan::new("")
        .with_child(TextSpan::styled("Plain ", base()))
        .with_child(TextSpan::styled(
            "words that wrap onto more lines than one",
            shadowed,
        ));
    let painter = laid_out(
        &fonts,
        TextPainter::new()
            .with_text(text)
            .with_max_lines(Some(1))
            .with_ellipsis(Some("\u{2026}".to_owned())),
        180.0,
    );
    assert!(painter.did_exceed_max_lines(), "the fixture is ellipsized");
    let list = painted(&painter);
    let glyphs = pens(&text_paragraph(&list));
    let ellipsis = *glyphs.last().expect("the line paints glyphs");
    let recorded = shadows(&list);
    assert_eq!(recorded.len(), 1);
    let cast = pens(&recorded[0].0);
    assert_eq!(
        cast.last(),
        Some(&ellipsis),
        "the ellipsis casts the shadow of the span it follows: shadow {cast:?}, text {glyphs:?}"
    );
    let reach = backgrounds(&list)
        .iter()
        .map(Rect::right)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        reach > f64::from(ellipsis) + 1.0,
        "the background runs under the ellipsis: ends at {reach}, ellipsis at {ellipsis}"
    );
}

/// Adding, changing or clearing a shadow or a background, on the root span
/// or on a child, keeps the measured layout: the painter repaints without
/// laying out, and the new paint shows.
pub(crate) fn a_shadow_only_change_repaints_without_relayout() {
    let fonts = FontCollection::new();
    let red = Shadow::new(Color::RED, Offset::new(1.0, 1.0), 2.0);
    let yellow = TextStyle {
        background_color: Some(Color::YELLOW),
        ..base()
    };
    let root = |style: TextStyle| TextSpan::styled("Shadow", style);
    let child =
        |style: TextStyle| TextSpan::new("Text ").with_child(TextSpan::styled("Shadow", style));
    for (tree, wrap) in [
        ("root", &root as &dyn Fn(TextStyle) -> TextSpan),
        ("child", &child),
    ] {
        let mut painter = laid_out(&fonts, TextPainter::new().with_text(wrap(base())), 400.0);
        let measured = text_paragraph(&painted(&painter));
        let steps: [(TextStyle, &[Painted]); 4] = [
            (
                base().with_shadow(red),
                &[Painted::Shadow(Color::RED), Painted::Text],
            ),
            (
                yellow.clone().with_shadow(red.with_color(Color::BLUE)),
                &[
                    Painted::Background(Color::YELLOW),
                    Painted::Shadow(Color::BLUE),
                    Painted::Text,
                ],
            ),
            (
                yellow.clone(),
                &[Painted::Background(Color::YELLOW), Painted::Text],
            ),
            (base(), &[Painted::Text]),
        ];
        for (index, (style, expected)) in steps.into_iter().enumerate() {
            assert_eq!(
                painter.set_text(Some(wrap(style).into())),
                Invalidation::Paint,
                "{tree} step {index}"
            );
            let list = painted(&painter);
            assert_eq!(sequence(&list), expected, "{tree} step {index}");
            assert!(
                Arc::ptr_eq(&text_paragraph(&list), &measured),
                "{tree} step {index} paints the paragraph it measured"
            );
        }
    }
}

/// A transparent shadow, a non-finite blur or offset are dropped; a blur
/// wider than [`BlurSigma::MAX`] is clamped; the other shadows and the text
/// still paint.
pub(crate) fn refused_shadows_are_dropped_and_the_rest_painted() {
    let fonts = FontCollection::new();
    let at = Offset::new(1.0, 1.0);
    let list = vec![
        Shadow::new(Color::TRANSPARENT, at, 1.0),
        Shadow::new(Color::RED, at, f64::NAN),
        Shadow::new(Color::RED, at, f64::INFINITY),
        Shadow::new(Color::RED, Offset::new(f64::NAN, 0.0), 1.0),
        Shadow::new(Color::RED, Offset::new(0.0, f64::INFINITY), 1.0),
        Shadow::new(Color::GREEN, at, 1e9),
        Shadow::new(Color::BLUE, at, 2.0),
    ];
    let text = TextSpan::styled(
        "Refused",
        TextStyle {
            shadows: Some(list),
            ..base()
        },
    );
    let painter = laid_out(&fonts, TextPainter::new().with_text(text), 400.0);
    let recorded = painted(&painter);
    assert_eq!(
        sequence(&recorded),
        [
            Painted::Shadow(Color::GREEN),
            Painted::Shadow(Color::BLUE),
            Painted::Text,
        ]
    );
    let sigmas = shadows(&recorded)
        .iter()
        .map(|(_, _, sigma)| *sigma)
        .collect::<Vec<BlurSigma>>();
    assert_eq!(
        sigmas,
        [
            BlurSigma::MAX,
            BlurSigma::from_blur_radius(2.0).expect("finite radius")
        ]
    );
}

/// A shadow's blur radius converts to a sigma as Flutter's does, bounded to
/// [`BlurSigma::MAX`]; a non-finite radius has none.
pub(crate) fn blur_sigma_converts_and_bounds_a_blur_radius() {
    let radii: [(f64, Option<f32>); 9] = [
        (0.0, Some(0.0)),
        (-3.0, Some(0.0)),
        (1.0, Some(1.077_35)),
        (4.0, Some(2.809_4)),
        (1e9, Some(128.0)),
        (f64::MAX, Some(128.0)),
        (f64::NAN, None),
        (f64::INFINITY, None),
        (f64::NEG_INFINITY, None),
    ];
    for (radius, expected) in radii {
        let sigma = BlurSigma::from_blur_radius(radius).map(BlurSigma::get);
        match (sigma, expected) {
            (Some(sigma), Some(expected)) => assert!(
                (sigma - expected).abs() < 1e-4,
                "radius {radius}: sigma {sigma}, expected {expected}"
            ),
            (sigma, expected) => assert_eq!(sigma, expected, "radius {radius}"),
        }
    }
    let sigmas: [(f64, bool); 7] = [
        (0.0, true),
        (2.5, true),
        (128.0, true),
        (128.001, false),
        (-1e-9, false),
        (f64::NAN, false),
        (f64::INFINITY, false),
    ];
    for (sigma, accepted) in sigmas {
        assert_eq!(
            BlurSigma::try_new(sigma).is_some(),
            accepted,
            "sigma {sigma}"
        );
    }
}

/// A shadow's damage covers its glyphs' ink at its offset, grown by three
/// sigmas; a sharp shadow's is the ink at its offset alone.
pub(crate) fn a_text_shadow_extent_covers_its_blur() {
    let fonts = FontCollection::new();
    let text = TextSpan::styled("Hgjpqy", base().with_font_size(40.0).with_height(0.5));
    let painter = laid_out(&fonts, TextPainter::new().with_text(text), 400.0);
    let glyphs = text_paragraph(&painted(&painter));
    let ink = glyphs
        .ink_bounds()
        .expect("the bundled face gives glyph bounds");
    let offset = Offset::new(5.0, 7.0);
    for radius in [0.0, 10.0] {
        let sigma = BlurSigma::from_blur_radius(radius).expect("finite radius");
        let mut canvas = Canvas::new();
        canvas.draw_paragraph_shadow(&glyphs, offset, Color::RED, sigma);
        let extent = canvas
            .finish()
            .damage_extent()
            .expect("a shadow changes pixels");
        let DamageExtent::Bounded { rect, spread } = extent else {
            panic!("a shadow's damage is bounded: {extent:?}");
        };
        let size = glyphs.size();
        let layout = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);
        assert!(
            rect.contains_rect(&ink.translate_offset(offset)) && rect.contains_rect(&layout),
            "radius {radius}: {rect:?} covers the ink {ink:?} at {offset:?}"
        );
        assert!(
            spread >= 3.0 * f64::from(sigma.get()),
            "radius {radius}: spread {spread} reaches three sigmas of {}",
            sigma.get()
        );
        if radius == 0.0 {
            assert!(spread == 0.0, "a sharp shadow has no spread: {spread}");
        }
    }
    let mut canvas = Canvas::new();
    canvas.draw_paragraph_shadow(
        &glyphs,
        Offset::new(f64::NAN, 0.0),
        Color::RED,
        BlurSigma::ZERO,
    );
    assert!(
        canvas.finish().damage_extent().is_none(),
        "a non-finite offset records nothing"
    );
}

//! A text style's decoration draws its lines where the text's font puts
//! them, in the order CSS paints them (ADR-0310).

use std::sync::Arc;

use flui_foundation::geometry::{Lerp, Offset, Rect};
use flui_painting::paint::PaintStyle;
use flui_painting::styling::Color;
use flui_painting::typography::{
    DecorationThickness, TextBaseline, TextDecoration, TextDecorationLines, TextDecorationStyle,
    TextDirection, TextSpan, TextStyle,
};
use flui_painting::{
    Canvas, DisplayList, DrawOp, FontCollection, Invalidation, ShapedParagraph, TextContext,
    TextPainter,
};

/// Large enough that a font's underline is several pixels thick and well
/// below the baseline, so a one-pixel bar a pixel under it cannot pass.
const SIZE: f64 = 80.0;

/// What one recorded op draws.
#[derive(Clone, Debug, PartialEq)]
enum Drawn {
    Bar {
        rect: Rect<f64>,
        color: Color,
    },
    Pattern {
        bounds: Rect<f64>,
        style: PaintStyle,
        stroke: f64,
        color: Color,
    },
    Text,
}

fn style() -> TextStyle {
    TextStyle::default().with_font_size(SIZE)
}

fn decorated(decoration: TextDecoration) -> TextStyle {
    style().with_decoration(decoration)
}

fn laid_out(fonts: &FontCollection, painter: TextPainter, max_width: f64) -> TextPainter {
    let mut painter = painter.with_text_direction(TextDirection::Ltr);
    painter
        .layout(&mut TextContext::new(fonts), 0.0, max_width)
        .expect("valid fixture lays out");
    painter
}

fn painter_of(fonts: &FontCollection, text: TextSpan) -> TextPainter {
    laid_out(fonts, TextPainter::new().with_text(text), 2000.0)
}

fn record(painter: &TextPainter, offset: Offset<f64>) -> DisplayList {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, offset);
    canvas.finish()
}

fn drawn(list: &DisplayList) -> Vec<Drawn> {
    list.iter()
        .map(|command| match &command.op {
            DrawOp::Rect { rect, paint } => Drawn::Bar {
                rect: *rect,
                color: paint.color,
            },
            DrawOp::Path { path, paint } => Drawn::Pattern {
                bounds: path.compute_bounds(),
                style: paint.style,
                stroke: paint.stroke_width,
                color: paint.color,
            },
            DrawOp::Paragraph { .. } => Drawn::Text,
            other => panic!("a decorated paragraph records only its own ops, found {other:?}"),
        })
        .collect::<Vec<Drawn>>()
}

fn bars(list: &DisplayList) -> Vec<Rect<f64>> {
    drawn(list)
        .into_iter()
        .filter_map(|op| match op {
            Drawn::Bar { rect, .. } => Some(rect),
            _ => None,
        })
        .collect::<Vec<Rect<f64>>>()
}

fn text_paragraph(list: &DisplayList) -> Arc<ShapedParagraph> {
    list.iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter records its paragraph")
}

fn baseline(painter: &TextPainter) -> f64 {
    painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// The horizontal extent of the bytes `start..end`, from the painter's own
/// selection boxes.
fn extent(painter: &TextPainter, start: usize, end: usize) -> (f64, f64) {
    let boxes = painter.get_boxes_for_selection(start, end);
    let left = boxes
        .iter()
        .map(|b| b.rect.left())
        .fold(f64::INFINITY, f64::min);
    let right = boxes
        .iter()
        .map(|b| b.rect.right())
        .fold(f64::NEG_INFINITY, f64::max);
    (left, right)
}

/// "ab" then "cd" styled `child`, then "ef".
fn middle_span(child: TextStyle) -> TextSpan {
    TextSpan::styled("", style())
        .with_child(TextSpan::new("ab"))
        .with_child(TextSpan::styled("cd", child))
        .with_child(TextSpan::new("ef"))
}

/// An underline lies under its span alone, below the baseline, as thick as
/// its font says: several pixels at this size, not a flat one-pixel bar,
/// recorded before the glyphs.
pub(crate) fn an_underline_lies_where_the_font_puts_it() {
    let fonts = FontCollection::new();
    let painter = painter_of(
        &fonts,
        middle_span(TextStyle {
            color: Some(Color::BLUE),
            ..decorated(TextDecoration::UNDERLINE)
        }),
    );
    let at = Offset::new(10.0, 20.0);
    let list = record(&painter, at);
    let ops = drawn(&list);
    let [Drawn::Bar { rect, color }, Drawn::Text] = ops.as_slice() else {
        panic!("an underline is one bar before the glyphs: {ops:?}");
    };
    assert_eq!(
        *color,
        Color::BLUE,
        "the underline takes its glyphs' colour"
    );
    let (left, right) = extent(&painter, 2, 4);
    assert!(
        close(rect.left(), left + at.dx) && close(rect.right(), right + at.dx),
        "the underline runs under \"cd\" alone: {rect:?}, span {left}..{right}"
    );
    let below = rect.top() - (baseline(&painter) + at.dy);
    assert!(
        below > 2.0 && below < SIZE / 4.0,
        "the underline sits below the baseline where the font puts it: {below} px"
    );
    assert!(
        rect.height() > 2.0 && rect.height() < SIZE / 4.0,
        "the underline is as thick as the font's: {} px",
        rect.height()
    );
}

/// Overlines are drawn under the glyphs and line-throughs over them; an
/// overline sits at the ascent, a line-through between it and the baseline.
pub(crate) fn a_line_through_is_drawn_over_the_glyphs() {
    let fonts = FontCollection::new();
    let painter = painter_of(
        &fonts,
        TextSpan::styled(
            "Hxh",
            decorated(TextDecoration::new(
                TextDecorationLines::OVERLINE | TextDecorationLines::LINE_THROUGH,
            )),
        ),
    );
    let list = record(&painter, Offset::ZERO);
    let ops = drawn(&list);
    let [
        Drawn::Bar { rect: over, .. },
        Drawn::Text,
        Drawn::Bar { rect: through, .. },
    ] = ops.as_slice()
    else {
        panic!("overline, glyphs, line-through: {ops:?}");
    };
    // The line box adds half a leading above the font's ascent.
    let line = &painter.get_line_metrics()[0];
    let line_top = line.baseline - line.ascent;
    assert!(
        over.top() >= line_top - 1e-3 && over.top() < line_top + SIZE * 0.05,
        "the overline sits at the font's ascent, at the line's top: {over:?}, line top {line_top}"
    );
    assert!(
        through.top() > over.bottom() && through.bottom() < line.baseline,
        "the line-through crosses the glyphs above the baseline: {through:?}, baseline {}",
        line.baseline
    );
}

/// Each style draws its shape along the same stretch at the font's
/// thickness: a bar, two bars a thickness apart, a filled path of dots or
/// dashes, a stroked wave.
pub(crate) fn each_style_draws_its_shape() {
    let fonts = FontCollection::new();
    let paint = |style: TextDecorationStyle| {
        let painter = painter_of(
            &fonts,
            TextSpan::styled(
                "decorated",
                decorated(TextDecoration::UNDERLINE.with_style(style)),
            ),
        );
        drawn(&record(&painter, Offset::ZERO))
    };
    let solid = paint(TextDecorationStyle::Solid);
    let [Drawn::Bar { rect: bar, .. }, Drawn::Text] = solid.as_slice() else {
        panic!("a solid underline is one bar: {solid:?}");
    };
    let thickness = bar.height();

    let double = paint(TextDecorationStyle::Double);
    let [
        Drawn::Bar { rect: first, .. },
        Drawn::Bar { rect: second, .. },
        Drawn::Text,
    ] = double.as_slice()
    else {
        panic!("a double underline is two bars: {double:?}");
    };
    assert_eq!(
        first, bar,
        "the first line of a double underline is the solid one"
    );
    assert!(
        close(second.top() - first.bottom(), thickness)
            && close(second.height(), thickness)
            && close(second.left(), bar.left())
            && close(second.right(), bar.right()),
        "the second line is a thickness below: {first:?}, {second:?}"
    );

    for (style, wanted, stroke) in [
        (TextDecorationStyle::Dotted, PaintStyle::Fill, None),
        (TextDecorationStyle::Dashed, PaintStyle::Fill, None),
        (
            TextDecorationStyle::Wavy,
            PaintStyle::Stroke,
            Some(thickness),
        ),
    ] {
        let ops = paint(style);
        let [
            Drawn::Pattern {
                bounds,
                style: painted,
                stroke: width,
                ..
            },
            Drawn::Text,
        ] = ops.as_slice()
        else {
            panic!("a {style:?} underline is one path: {ops:?}");
        };
        assert_eq!(*painted, wanted, "{style:?} paints as {wanted:?}");
        if let Some(stroke) = stroke {
            assert!(close(*width, stroke), "{style:?} strokes a thickness wide");
        }
        assert!(
            bounds.left() >= bar.left() - 1e-3 && bounds.right() <= bar.right() + 1e-3,
            "{style:?} stays on the stretch: {bounds:?}, stretch {bar:?}"
        );
        assert!(
            bounds.width() > bar.width() * 0.8,
            "{style:?} runs the stretch's length: {bounds:?}, stretch {bar:?}"
        );
    }
}

/// A line's trailing spaces and its hard break are not decorated: each
/// line's underline ends at its last visible glyph.
pub(crate) fn trailing_whitespace_and_breaks_are_not_decorated() {
    let fonts = FontCollection::new();
    let text = "ab  \ncd ";
    let painter = painter_of(
        &fonts,
        TextSpan::styled(text, decorated(TextDecoration::UNDERLINE)),
    );
    let lines = bars(&record(&painter, Offset::ZERO));
    assert_eq!(lines.len(), 2, "one underline per line: {lines:?}");
    let first = extent(&painter, 0, 2);
    let second = extent(&painter, 5, 7);
    assert!(
        close(lines[0].left(), first.0) && close(lines[0].right(), first.1),
        "the first line's underline ends at \"ab\": {:?}, {first:?}",
        lines[0]
    );
    assert!(
        close(lines[1].left(), second.0) && close(lines[1].right(), second.1),
        "the second line's underline ends at \"cd\": {:?}, {second:?}",
        lines[1]
    );
}

/// An ellipsis takes the decoration of the span it follows.
pub(crate) fn an_ellipsis_takes_its_spans_decoration() {
    let fonts = FontCollection::new();
    let text = TextSpan::styled("", style())
        .with_child(TextSpan::new("Plain "))
        .with_child(TextSpan::styled(
            "words that wrap onto more lines than one",
            decorated(TextDecoration::UNDERLINE),
        ));
    let painter = laid_out(
        &fonts,
        TextPainter::new()
            .with_text(text)
            .with_max_lines(Some(1))
            .with_ellipsis(Some("\u{2026}".to_owned())),
        700.0,
    );
    assert!(painter.did_exceed_max_lines(), "the fixture is ellipsized");
    let list = record(&painter, Offset::ZERO);
    let ellipsis = text_paragraph(&list)
        .runs()
        .flat_map(|run| run.glyphs().iter().map(|glyph| f64::from(glyph.x)))
        .fold(f64::NEG_INFINITY, f64::max);
    let lines = bars(&list);
    assert_eq!(lines.len(), 1, "one underline: {lines:?}");
    assert!(
        lines[0].right() > ellipsis + 1.0,
        "the underline runs under the ellipsis: ends at {}, ellipsis at {ellipsis}",
        lines[0].right()
    );
    let plain = extent(&painter, 0, 5);
    assert!(
        lines[0].left() >= plain.1 - 1e-3,
        "the undecorated span stays bare: {:?}, \"Plain\" {plain:?}",
        lines[0]
    );
}

/// Adding, changing or clearing a decoration keeps the measured layout: the
/// painter repaints without laying out, and the new paint shows.
pub(crate) fn a_decoration_only_change_repaints_without_relayout() {
    let fonts = FontCollection::new();
    let steps = [
        Some(TextDecoration::UNDERLINE),
        Some(TextDecoration::LINE_THROUGH.with_color(Color::RED)),
        Some(TextDecoration::NONE),
        None,
    ];
    let mut painter = painter_of(&fonts, middle_span(style()));
    let measured = text_paragraph(&record(&painter, Offset::ZERO));
    for decoration in steps {
        let child = TextStyle {
            decoration,
            ..style()
        };
        let answer = painter.set_text(Some(middle_span(child).into()));
        assert_eq!(answer, Invalidation::Paint, "{decoration:?} is paint only");
        assert!(painter.has_layout(), "{decoration:?} keeps the layout");
        let list = record(&painter, Offset::ZERO);
        assert!(
            Arc::ptr_eq(&text_paragraph(&list), &measured),
            "{decoration:?} repaints the measured paragraph"
        );
        let wanted = usize::from(decoration.is_some_and(|d| !d.is_none()));
        assert_eq!(
            bars(&list).len(),
            wanted,
            "{decoration:?} paints what it now says"
        );
    }
}

/// A decoration is drawn in its glyphs' colour unless it names its own; a
/// transparent one draws nothing.
pub(crate) fn a_decoration_takes_its_glyphs_colour_unless_it_names_one() {
    let fonts = FontCollection::new();
    let rows = [
        (
            Some(Color::BLUE),
            TextDecoration::UNDERLINE,
            Some(Color::BLUE),
        ),
        (None, TextDecoration::UNDERLINE, Some(Color::BLACK)),
        (
            Some(Color::BLUE),
            TextDecoration::UNDERLINE.with_color(Color::RED),
            Some(Color::RED),
        ),
        (
            Some(Color::BLUE),
            TextDecoration::UNDERLINE.with_color(Color::TRANSPARENT),
            None,
        ),
    ];
    for (glyphs, decoration, wanted) in rows {
        let painter = painter_of(
            &fonts,
            middle_span(TextStyle {
                color: glyphs,
                ..decorated(decoration)
            }),
        );
        let colors = drawn(&record(&painter, Offset::ZERO))
            .into_iter()
            .filter_map(|op| match op {
                Drawn::Bar { color, .. } => Some(color),
                _ => None,
            })
            .collect::<Vec<Color>>();
        assert_eq!(
            colors,
            wanted.into_iter().collect::<Vec<Color>>(),
            "glyphs {glyphs:?}, {decoration:?}"
        );
    }
}

/// A child span styled [`TextDecoration::NONE`] draws no line under its own
/// text, though its parent's decoration runs on either side.
pub(crate) fn a_child_clears_an_inherited_decoration() {
    let fonts = FontCollection::new();
    let text = TextSpan::styled("", decorated(TextDecoration::UNDERLINE))
        .with_child(TextSpan::new("ab"))
        .with_child(TextSpan::styled(
            "cd",
            TextStyle::new().with_decoration(TextDecoration::NONE),
        ))
        .with_child(TextSpan::new("ef"));
    let painter = painter_of(&fonts, text);
    let lines = bars(&record(&painter, Offset::ZERO));
    let (ab, ef) = (extent(&painter, 0, 2), extent(&painter, 4, 6));
    assert_eq!(lines.len(), 2, "the underline breaks at \"cd\": {lines:?}");
    assert!(
        close(lines[0].right(), ab.1) && close(lines[1].left(), ef.0),
        "the line runs under \"ab\" and \"ef\" alone: {lines:?}, {ab:?}, {ef:?}"
    );
}

/// Adjacent spans that draw one decoration in one colour draw one line;
/// a colour change splits it.
pub(crate) fn adjacent_spans_with_one_decoration_draw_one_line() {
    let fonts = FontCollection::new();
    for (second, wanted) in [(Color::BLACK, 1), (Color::RED, 2)] {
        let text = TextSpan::styled("", decorated(TextDecoration::UNDERLINE))
            .with_child(TextSpan::new("ab"))
            .with_child(TextSpan::styled("cd", TextStyle::new().with_color(second)));
        let painter = painter_of(&fonts, text);
        let lines = bars(&record(&painter, Offset::ZERO));
        assert_eq!(
            lines.len(),
            wanted,
            "a second span in {second:?}: {lines:?}"
        );
    }
}

/// A decoration drawn over a byte range is the decoration a span over those
/// bytes draws, and an empty range or no line draws nothing.
pub(crate) fn a_range_decoration_is_the_spans_decoration() {
    let fonts = FontCollection::new();
    let spanned = painter_of(&fonts, middle_span(decorated(TextDecoration::UNDERLINE)));
    let plain = painter_of(&fonts, TextSpan::styled("abcdef", style()));
    let at = Offset::new(3.0, 4.0);
    let ranged = |ranges: &[(std::ops::Range<usize>, TextDecoration)]| {
        let mut canvas = Canvas::new();
        plain.paint_with_decorations(&mut canvas, at, ranges);
        canvas.finish()
    };
    let list = ranged(&[(2..4, TextDecoration::UNDERLINE)]);
    assert_eq!(
        drawn(&list),
        drawn(&record(&spanned, at)),
        "a range's underline is a span's"
    );
    for ranges in [
        vec![(2..2, TextDecoration::UNDERLINE)],
        vec![(2..4, TextDecoration::NONE)],
        vec![],
    ] {
        assert_eq!(
            drawn(&ranged(&ranges)),
            vec![Drawn::Text],
            "{ranges:?} draws nothing"
        );
    }
}

/// A thickness scales the font's: twice as thick from the same top for an
/// underline, about the same centre for a line-through.
pub(crate) fn a_thickness_scales_the_fonts() {
    let fonts = FontCollection::new();
    let line = |lines: TextDecorationLines, scale: f64| {
        let thickness = DecorationThickness::try_new(scale).expect("the fixture's scale is valid");
        let painter = painter_of(
            &fonts,
            TextSpan::styled(
                "Hxh",
                decorated(TextDecoration::new(lines).with_thickness(thickness)),
            ),
        );
        bars(&record(&painter, Offset::ZERO))[0]
    };
    let (under, thick_under) = (
        line(TextDecorationLines::UNDERLINE, 1.0),
        line(TextDecorationLines::UNDERLINE, 2.0),
    );
    assert!(
        close(thick_under.height(), 2.0 * under.height()) && close(thick_under.top(), under.top()),
        "an underline grows down from its top: {under:?} to {thick_under:?}"
    );
    let (through, thick_through) = (
        line(TextDecorationLines::LINE_THROUGH, 1.0),
        line(TextDecorationLines::LINE_THROUGH, 2.0),
    );
    assert!(
        close(thick_through.height(), 2.0 * through.height())
            && close(thick_through.center().y, through.center().y),
        "a line-through grows about its centre: {through:?} to {thick_through:?}"
    );
}

/// A thickness is finite, positive and at most sixteen times the font's.
pub(crate) fn a_decoration_thickness_is_bounded() {
    for (scale, accepted) in [
        (1.0, true),
        (0.25, true),
        (16.0, true),
        (0.0, false),
        (-1.0, false),
        (16.5, false),
        (f64::NAN, false),
        (f64::INFINITY, false),
    ] {
        assert_eq!(
            DecorationThickness::try_new(scale).is_some(),
            accepted,
            "scale {scale}"
        );
    }
    assert_eq!(
        DecorationThickness::default(),
        DecorationThickness::FROM_FONT
    );
}

/// Decorations of the same lines and shape blend their colour and
/// thickness; others switch at the midpoint.
pub(crate) fn decorations_interpolate_their_colour_and_thickness() {
    let thickness = |scale: f64| DecorationThickness::try_new(scale).expect("valid scale");
    let from = TextStyle::new().with_decoration(
        TextDecoration::UNDERLINE
            .with_color(Color::rgb(0, 0, 0))
            .with_thickness(thickness(1.0)),
    );
    let to = TextStyle::new().with_decoration(
        TextDecoration::UNDERLINE
            .with_color(Color::rgb(200, 100, 0))
            .with_thickness(thickness(3.0)),
    );
    let mid = from
        .lerp_to(&to, 0.5)
        .decoration
        .expect("both ends decorate");
    assert_eq!(
        mid.color,
        Some(Color::rgb(0, 0, 0).lerp_to(&Color::rgb(200, 100, 0), 0.5)),
        "the colour blends as colours do"
    );
    assert!(close(f64::from(mid.thickness.get()), 2.0), "{mid:?}");
    let other = TextStyle::new().with_decoration(TextDecoration::LINE_THROUGH);
    assert_eq!(from.lerp_to(&other, 0.4).decoration, from.decoration);
    assert_eq!(from.lerp_to(&other, 0.6).decoration, other.decoration);
}

/// A pattern is placed step by step from its stretch's start, so text
/// painted far from the origin, where a step is smaller than the spacing
/// of the coordinates, still records a bounded path and returns.
pub(crate) fn a_pattern_far_from_the_origin_is_drawn_in_bounded_steps() {
    let fonts = FontCollection::new();
    // At 2^52 consecutive coordinates are a pixel apart, more than a step of
    // a one-pixel font's pattern.
    let far = Offset::new(2f64.powi(52), 0.0);
    for style in [
        TextDecorationStyle::Dotted,
        TextDecorationStyle::Dashed,
        TextDecorationStyle::Wavy,
    ] {
        let painter = painter_of(
            &fonts,
            TextSpan::styled(
                "decorated text",
                TextStyle::default()
                    .with_font_size(1.0)
                    .with_decoration(TextDecoration::UNDERLINE.with_style(style)),
            ),
        );
        let ops = drawn(&record(&painter, far));
        assert!(
            matches!(
                ops.as_slice(),
                [Drawn::Pattern { .. } | Drawn::Bar { .. }, Drawn::Text]
            ),
            "{style:?} far from the origin draws one line: {ops:?}"
        );
    }
}

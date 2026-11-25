//! The contrast rule over a painted frame, through the public API: a text's colour against
//! the fill painted behind it, read from the frame's scene, at WCAG's large-text thresholds with
//! the ratio unrounded; a transparent run not judged; a text whose colours the commands do not
//! determine reported as not
//! checked, with the reason its layers, clips, save layers, transforms and later paints give.

use std::fmt;
use std::rc::Rc;

use flui_foundation::TextDirection;
use flui_foundation::geometry::{
    DevicePixelRatio, EdgeInsets, Matrix4, Offset, Point, RRect, RSuperellipse, Rect, Size,
};
use flui_objects::{
    RenderBackdropFilter, RenderClipPath, RenderClipRRect, RenderClipRSuperellipse, RenderClipRect,
    RenderCustomPaint, RenderFlex, RenderFollowerLayer, RenderLeaderLayer, RenderOpacity,
    RenderPadding, RenderRepaintBoundary, RenderShaderMask, RenderTransform,
};
use flui_painting::TextPainter;
use flui_painting::paint::{
    BlendMode, Clip, ClipOp, Image, ImageFilter, Paint, Path, PointMode, Shader,
};
use flui_painting::styling::{Color, linear_to_srgb};
use flui_painting::typography::{FontWeight, TextSpan, TextStyle};
use flui_rendering::audit::{ContrastOutcome, ContrastReading, ContrastUnknown, TextContrast};
use flui_rendering::{
    PipelineOwner, TextContextHandle,
    constraints::BoxConstraints,
    delegates::{CustomPaintContext, CustomPainter},
    testing::{SceneHost, TreeNode, box_node, tree},
};

const TEXT: &str = "Contrast";

/// Where every text of this file is painted, in its painter's space.
const AT: Offset<f64> = Offset::new(4.0, 4.0);

/// Every painter's box.
fn area_size() -> Size {
    Size::new(320.0, 80.0)
}

/// A painter's whole box.
fn area(ctx: &CustomPaintContext<'_>) -> Rect<f64> {
    Rect::from_origin_size(Point::ZERO, ctx.size())
}

/// The box's left 30 px: the text starts at x = 4 and runs past it.
fn left_part(ctx: &CustomPaintContext<'_>) -> Rect<f64> {
    Rect::from_ltrb(0.0, 0.0, 30.0, ctx.size().height)
}

/// The box right of x = 30, where the text runs on.
fn right_part(ctx: &CustomPaintContext<'_>) -> Rect<f64> {
    Rect::from_ltrb(30.0, 0.0, ctx.size().width, ctx.size().height)
}

/// A painter that runs a closure: one per case.
struct Draw(Box<dyn Fn(&mut CustomPaintContext<'_>)>);

impl fmt::Debug for Draw {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("Draw")
    }
}

impl CustomPainter for Draw {
    fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
        (self.0)(ctx);
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        false
    }
}

fn draw(paint: impl Fn(&mut CustomPaintContext<'_>) + 'static) -> Rc<dyn CustomPainter> {
    Rc::new(Draw(Box::new(paint)))
}

/// A leaf that paints with `paint`.
fn leaf(paint: impl Fn(&mut CustomPaintContext<'_>) + 'static) -> TreeNode {
    box_node(RenderCustomPaint::new(Some(draw(paint)), None, area_size()))
}

/// A node that paints `behind`, then its `child`, then `over`.
fn around(
    behind: impl Fn(&mut CustomPaintContext<'_>) + 'static,
    child: TreeNode,
    over: impl Fn(&mut CustomPaintContext<'_>) + 'static,
) -> TreeNode {
    box_node(RenderCustomPaint::new(
        Some(draw(behind)),
        Some(draw(over)),
        area_size(),
    ))
    .child(child)
}

/// Paints nothing.
fn nothing(_: &mut CustomPaintContext<'_>) {}

/// Paints `span` at [`AT`].
fn write_span(ctx: &mut CustomPaintContext<'_>, span: TextSpan) {
    let width = ctx.size().width;
    let mut label = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);
    ctx.text()
        .expect("the paint pass lends the text context")
        .measurement()
        .layout(&mut label, 0.0, width)
        .expect("the label lays out");
    label.paint(ctx.canvas(), AT);
}

/// Paints [`TEXT`] in `color` at `size`, `weight`.
fn write(ctx: &mut CustomPaintContext<'_>, color: Color, size: f64, weight: FontWeight) {
    let style = TextStyle::default()
        .with_color(color)
        .with_font_size(size)
        .with_font_weight(weight);
    write_span(ctx, TextSpan::styled(TEXT, style));
}

/// Paints [`TEXT`] in black at 16 px.
fn write_black(ctx: &mut CustomPaintContext<'_>) {
    write(ctx, Color::BLACK, 16.0, FontWeight::NORMAL);
}

/// Fills the whole box with `color`.
fn fill(ctx: &mut CustomPaintContext<'_>, color: Color) {
    let area = area(ctx);
    ctx.canvas().draw_rect(area, &Paint::fill(color));
}

/// An oval over the whole box: a path the walk knows only by its bounding box.
fn oval(ctx: &CustomPaintContext<'_>) -> Path {
    Path::oval(area(ctx))
}

/// The texts one frame of `spec` paints at a device pixel ratio of `ratio`, judged; then the
/// same version judged again after the pipeline's ratio moved to `later` with no frame painted.
fn judged_at(spec: TreeNode, ratio: f64, later: f64) -> (Vec<TextContrast>, Vec<TextContrast>) {
    let text = TextContextHandle::standalone();
    let mut owner = PipelineOwner::new(text);
    owner.set_device_pixel_ratio(DevicePixelRatio::try_new(ratio).expect("a ratio"));
    let (root, _) = tree::mount(&mut owner, spec);
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::loose(Size::new(400.0, 400.0))));
    let mut host = SceneHost::new();
    host.run_frame(&mut owner)
        .expect("the frame paints")
        .expect("the first frame produces a layer tree");
    let painted = owner
        .audit_text_contrast(host.version())
        .expect("the version holds the owner's frame");
    owner.set_device_pixel_ratio(DevicePixelRatio::try_new(later).expect("a ratio"));
    let read_later = owner
        .audit_text_contrast(host.version())
        .expect("the version still holds the owner's frame");
    (painted, read_later)
}

/// The texts one frame of `spec` paints, judged.
fn judged(spec: TreeNode) -> Vec<TextContrast> {
    judged_at(spec, 1.0, 1.0).0
}

/// The grey whose contrast against white is `ratio`: WCAG's ratio solved for the relative
/// luminance, taken back to an sRGB channel.
fn grey_against_white(ratio: f64) -> Color {
    let luminance = 1.05 / ratio - 0.05;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a luminance in 0..1 narrows to the colour's f32 channel"
    )]
    let channel = linear_to_srgb(luminance as f32);
    Color::from_rgba_f32_array([channel, channel, channel, 1.0])
}

/// What the rule should find for the frame's texts.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Expect {
    Meets(f64),
    Below(f64),
    NotChecked(ContrastUnknown),
    /// The frame shows no text to judge.
    NoText,
}

/// What the rule found for the one text `texts` holds, or [`Expect::NoText`].
fn found(name: &str, texts: &[TextContrast]) -> Expect {
    match texts {
        [] => Expect::NoText,
        [text] => {
            assert_eq!(text.text, TEXT, "{name}");
            match text.outcome {
                ContrastOutcome::Meets(reading) => Expect::Meets(reading.required),
                ContrastOutcome::Below(reading) => Expect::Below(reading.required),
                ContrastOutcome::NotChecked(why) => Expect::NotChecked(why),
                _ => panic!("{name}: an outcome this test does not know: {text:#?}"),
            }
        }
        _ => panic!("{name}: at most one text is painted: {texts:#?}"),
    }
}

/// One row per input: the pair, the size, the weight and what is behind decide the outcome.
#[test]
fn contrast_is_judged_against_the_threshold_its_size_and_weight_need() {
    let regular = FontWeight::NORMAL;
    let bold = FontWeight::BOLD;
    type Row = (&'static str, Color, f64, FontWeight, Expect);
    let table: [Row; 10] = [
        (
            "a failing pair",
            grey_against_white(2.0),
            16.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "a passing pair",
            Color::BLACK,
            16.0,
            regular,
            Expect::Meets(4.5),
        ),
        (
            "24 px is large text",
            grey_against_white(3.5),
            24.0,
            regular,
            Expect::Meets(3.0),
        ),
        (
            "23 px regular is not",
            grey_against_white(3.5),
            23.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "19 px bold is large text",
            grey_against_white(3.5),
            19.0,
            bold,
            Expect::Meets(3.0),
        ),
        (
            "18 px bold is not",
            grey_against_white(3.5),
            18.0,
            bold,
            Expect::Below(4.5),
        ),
        (
            "19 px regular is not",
            grey_against_white(3.5),
            19.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "4.495:1 is under 4.5: the ratio is not rounded",
            grey_against_white(4.495),
            16.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "4.505:1 meets 4.5",
            grey_against_white(4.505),
            16.0,
            regular,
            Expect::Meets(4.5),
        ),
        (
            "a half-transparent black is the grey it shows over white, under 4.5",
            Color::BLACK.with_alpha(128),
            16.0,
            regular,
            Expect::Below(4.5),
        ),
    ];
    for (name, color, size, weight, expect) in table {
        let texts = judged(leaf(move |ctx| {
            fill(ctx, Color::WHITE);
            write(ctx, color, size, weight);
        }));
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// A glyph run that paints nothing, its colour fully transparent, is not judged: a text of
/// transparent runs alone is left out of the answer, and one with a visible run is judged by it.
#[test]
fn a_transparent_run_is_not_judged() {
    let transparent = Color::BLACK.with_alpha(0);
    let styled = |text: &'static str, color: Color| {
        TextSpan::styled(
            text,
            TextStyle::default().with_color(color).with_font_size(16.0),
        )
    };
    for (name, spans, expect) in [
        (
            "a transparent text",
            vec![styled(TEXT, transparent)],
            Expect::NoText,
        ),
        (
            "a transparent span beside a black one",
            vec![styled("Con", transparent), styled("trast", Color::BLACK)],
            Expect::Meets(4.5),
        ),
    ] {
        let texts = judged(leaf(move |ctx| {
            fill(ctx, Color::WHITE);
            write_span(ctx, TextSpan::with_children(spans.clone()));
        }));
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// One row per scene: what a fill, a gradient, a clip, a save layer, a layer, a transform or a
/// later paint does to what the rule can prove is behind a text, and so to its outcome.
#[test]
fn a_text_is_judged_only_against_a_colour_the_scene_proves_is_behind_it() {
    let white = Color::WHITE;
    let half_white = Color::WHITE.with_alpha(128);
    let unresolved = Expect::NotChecked(ContrastUnknown::UnresolvedEffect);
    let rows: Vec<(&'static str, TreeNode, Expect)> = vec![
        // What is painted before the text.
        (
            "a fill over part of the text",
            leaf(|ctx| {
                let part = left_part(ctx);
                ctx.canvas().draw_rect(part, &Paint::fill(Color::WHITE));
                write_black(ctx);
            }),
            Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop),
        ),
        (
            "a gradient behind the text",
            leaf(|ctx| {
                let area = area(ctx);
                let mut paint = Paint::fill(Color::WHITE);
                paint.shader = Some(Shader::simple_linear(
                    Offset::ZERO,
                    Offset::new(area.width(), 0.0),
                    vec![Color::WHITE, Color::BLACK],
                ));
                ctx.canvas().draw_rect(area, &paint);
                write_black(ctx);
            }),
            Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop),
        ),
        (
            "nothing behind the text",
            leaf(write_black),
            Expect::NotChecked(ContrastUnknown::NothingOpaqueBehind),
        ),
        (
            "a translucent white over black is the grey white text fails on",
            leaf(move |ctx| {
                fill(ctx, Color::BLACK);
                fill(ctx, half_white);
                write(ctx, Color::WHITE, 16.0, FontWeight::NORMAL);
            }),
            Expect::Below(4.5),
        ),
        // Save layers.
        (
            "a save layer's bounds cut the fill it holds away from the text",
            leaf(|ctx| {
                let (left, area) = (left_part(ctx), area(ctx));
                ctx.canvas()
                    .save_layer(Some(left), &Paint::fill(Color::BLACK));
                ctx.canvas().draw_rect(area, &Paint::fill(Color::WHITE));
                ctx.canvas().restore();
                write_black(ctx);
            }),
            Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop),
        ),
        (
            "a save layer's bounds cut away the text it holds",
            leaf(|ctx| {
                let right = right_part(ctx);
                fill(ctx, Color::WHITE);
                ctx.canvas().save_layer(
                    Some(Rect::from_ltrb(
                        right.right() - 1.0,
                        0.0,
                        right.right(),
                        1.0,
                    )),
                    &Paint::fill(Color::BLACK),
                );
                write_black(ctx);
                ctx.canvas().restore();
            }),
            Expect::NoText,
        ),
        (
            "a text inside a translucent save layer",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                ctx.canvas().save_layer_alpha(None, 128);
                write_black(ctx);
                ctx.canvas().restore();
            }),
            unresolved,
        ),
        (
            "a fill inside a translucent save layer, behind the text",
            leaf(|ctx| {
                ctx.canvas().save_layer_alpha(None, 128);
                fill(ctx, Color::WHITE);
                ctx.canvas().restore();
                write_black(ctx);
            }),
            unresolved,
        ),
        (
            "a save layer that replaces its region over the fill behind the text",
            leaf(|ctx| {
                let area = area(ctx);
                fill(ctx, Color::WHITE);
                ctx.canvas().save_layer_blend(Some(area), BlendMode::Src);
                ctx.canvas().restore();
                write_black(ctx);
            }),
            unresolved,
        ),
        // Clips drawn on the canvas.
        (
            "a fill under a path clip, behind the text",
            leaf(|ctx| {
                let oval = oval(ctx);
                ctx.canvas().save();
                ctx.canvas().clip_path(&oval);
                fill(ctx, Color::WHITE);
                ctx.canvas().restore();
                write_black(ctx);
            }),
            unresolved,
        ),
        (
            "a text under a path clip, over a fill painted before it",
            leaf(|ctx| {
                let oval = oval(ctx);
                fill(ctx, Color::WHITE);
                ctx.canvas().clip_path(&oval);
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        // Transforms drawn on the canvas.
        (
            "a text under a rotation",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                ctx.canvas().rotate(0.3);
                write_black(ctx);
            }),
            unresolved,
        ),
        (
            "12 px scaled by 2 is large text",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                ctx.canvas().scale(2.0, 2.0);
                write(ctx, grey_against_white(3.5), 12.0, FontWeight::NORMAL);
            }),
            Expect::Meets(3.0),
        ),
        (
            "12 px squashed to half its height counts at its smaller size",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                ctx.canvas().scale(2.0, 1.0);
                write(ctx, grey_against_white(3.5), 12.0, FontWeight::NORMAL);
            }),
            Expect::Below(4.5),
        ),
        // Paints after the text.
        (
            "an opaque fill over the text hides it",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                write_black(ctx);
                fill(ctx, Color::WHITE);
            }),
            Expect::NotChecked(ContrastUnknown::PaintedOver),
        ),
        (
            "a translucent fill over part of the text",
            leaf(move |ctx| {
                let part = left_part(ctx);
                fill(ctx, Color::WHITE);
                write_black(ctx);
                ctx.canvas().draw_rect(part, &Paint::fill(half_white));
            }),
            Expect::NotChecked(ContrastUnknown::PaintedOver),
        ),
        // Layers.
        (
            "a text in a child painted over its parent's fill",
            around(move |ctx| fill(ctx, white), leaf(write_black), nothing),
            Expect::Meets(4.5),
        ),
        (
            "a text under a half opacity layer",
            around(
                move |ctx| fill(ctx, white),
                box_node(RenderOpacity::new(0.5)).child(leaf(write_black)),
                nothing,
            ),
            unresolved,
        ),
        (
            "a fill under a half opacity layer, behind the text",
            around(
                nothing,
                box_node(RenderOpacity::new(0.5)).child(leaf(move |ctx| fill(ctx, white))),
                write_black,
            ),
            unresolved,
        ),
        (
            "a fill under a path clip layer, behind the text",
            around(
                nothing,
                box_node(
                    RenderClipPath::hard_edge()
                        .with_clip_shape(Path::oval(Rect::from_ltrb(0.0, 0.0, 320.0, 80.0))),
                )
                .child(leaf(move |ctx| fill(ctx, white))),
                write_black,
            ),
            unresolved,
        ),
        (
            "a fill a clip rect layer cuts to part of the text",
            around(
                nothing,
                box_node(
                    RenderClipRect::hard_edge()
                        .with_clip_shape(Rect::from_ltrb(0.0, 0.0, 30.0, 80.0)),
                )
                .child(leaf(move |ctx| fill(ctx, white))),
                write_black,
            ),
            Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop),
        ),
        (
            "a backdrop filter over the fill behind the text",
            around(
                move |ctx| fill(ctx, white),
                box_node(RenderBackdropFilter::new(ImageFilter::blur(4.0))).child(leaf(nothing)),
                write_black,
            ),
            unresolved,
        ),
        (
            "12 px under a layer that scales by 2 is large text",
            around(
                move |ctx| fill(ctx, white),
                box_node(RenderTransform::new(Matrix4::scaling(2.0, 2.0, 1.0))).child(leaf(
                    |ctx| write(ctx, grey_against_white(3.5), 12.0, FontWeight::NORMAL),
                )),
                nothing,
            ),
            Expect::Meets(3.0),
        ),
        (
            "a text under a layer that rotates",
            around(
                move |ctx| fill(ctx, white),
                box_node(RenderTransform::new(Matrix4::rotation_z(0.3))).child(leaf(write_black)),
                nothing,
            ),
            unresolved,
        ),
    ];
    for (name, spec, expect) in rows {
        let texts = judged(spec);
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// A text whose spans mix bold and regular at a size where only bold text is large is not
/// judged; below that size both need 4.5:1 and it is.
#[test]
fn a_text_that_mixes_weights_is_judged_only_where_its_weight_does_not_decide() {
    for (name, size, expect) in [
        (
            "20 px: bold is large, regular is not",
            20.0,
            Expect::NotChecked(ContrastUnknown::MixedWeight),
        ),
        ("16 px: neither is large", 16.0, Expect::Below(4.5)),
    ] {
        let texts = judged(leaf(move |ctx| {
            fill(ctx, Color::WHITE);
            let grey = grey_against_white(3.5);
            let span = TextSpan::with_children(vec![
                TextSpan::styled(
                    "Con",
                    TextStyle::default()
                        .with_color(grey)
                        .with_font_size(size)
                        .with_font_weight(FontWeight::BOLD),
                ),
                TextSpan::styled(
                    "trast",
                    TextStyle::default()
                        .with_color(grey)
                        .with_font_size(size)
                        .with_font_weight(FontWeight::NORMAL),
                ),
            ]);
            write_span(ctx, span);
        }));
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// The reading names the ratio, the composited colours and the size it judged.
#[test]
fn a_reading_names_the_ratio_and_the_colours_it_compared() {
    let texts = judged(leaf(|ctx| {
        fill(ctx, Color::WHITE);
        write(ctx, grey_against_white(3.0), 16.0, FontWeight::NORMAL);
    }));
    let [text] = texts.as_slice() else {
        panic!("one text is painted: {texts:#?}");
    };
    let ContrastOutcome::Below(reading) = text.outcome else {
        panic!("a 3:1 grey at 16 px is below 4.5: {text:#?}");
    };
    assert!((reading.ratio - 3.0).abs() < 1e-3, "{reading:?}");
    assert_eq!(reading.background, Color::WHITE);
    assert_eq!(reading.foreground, grey_against_white(3.0));
    assert!((reading.font_size - 16.0).abs() < 1e-6, "{reading:?}");
    assert!(!reading.bold);
}

/// A translucent fill painted over the whole text after it (a scrim) tints its glyphs and its
/// background alike, and the reading compares the two as they show.
#[test]
fn a_translucent_fill_over_the_text_tints_both_colours() {
    let scrim = Color::BLACK.with_alpha(128);
    let texts = judged(leaf(move |ctx| {
        fill(ctx, Color::WHITE);
        write(ctx, grey_against_white(5.0), 16.0, FontWeight::NORMAL);
        fill(ctx, scrim);
    }));
    let [text] = texts.as_slice() else {
        panic!("one text is painted: {texts:#?}");
    };
    let (ContrastOutcome::Meets(reading) | ContrastOutcome::Below(reading)) = text.outcome else {
        panic!("the text is judged: {text:#?}");
    };
    assert_eq!(reading.background, scrim.blend_over(Color::WHITE));
    assert_eq!(
        reading.foreground,
        scrim.blend_over(grey_against_white(5.0))
    );
}

/// A frame is read in the logical pixels it was laid out in, at the ratio it was painted at:
/// a frame painted at 2 reads its 16 px text as 16 px, and so does a read of the same frame
/// after the pipeline's ratio moved, before the next frame is painted.
#[test]
fn a_frame_is_read_at_the_ratio_it_was_painted_at() {
    let page = || {
        leaf(|ctx| {
            fill(ctx, Color::WHITE);
            write(ctx, grey_against_white(3.5), 16.0, FontWeight::NORMAL);
        })
    };
    for (name, painted_at, read_at) in [
        ("painted at 2, read at 2", 2.0, 2.0),
        ("painted at 2, read once the ratio is 1", 2.0, 1.0),
        ("painted at 1, read once the ratio is 2", 1.0, 2.0),
    ] {
        let (_, texts) = judged_at(page(), painted_at, read_at);
        let [text] = texts.as_slice() else {
            panic!("{name}: one text is painted: {texts:#?}");
        };
        let ContrastOutcome::Below(reading) = text.outcome else {
            panic!("{name}: a 3.5:1 grey at 16 px is below 4.5: {text:#?}");
        };
        assert!(
            (reading.font_size - 16.0).abs() < 1e-6,
            "{name}: {reading:?}"
        );
    }
}

/// The reading of the one text `texts` holds, whether it meets its requirement or falls below.
fn reading_of(name: &str, texts: &[TextContrast]) -> (bool, ContrastReading) {
    match texts {
        [text] => match text.outcome {
            ContrastOutcome::Meets(reading) => (true, reading),
            ContrastOutcome::Below(reading) => (false, reading),
            _ => panic!("{name}: the text is judged: {text:#?}"),
        },
        _ => panic!("{name}: one text is painted: {texts:#?}"),
    }
}

/// The box [`write_black`] sets its glyphs in, read from a layout of the same text.
fn black_text_box(ctx: &mut CustomPaintContext<'_>) -> Rect<f64> {
    let width = ctx.size().width;
    let style = TextStyle::default()
        .with_color(Color::BLACK)
        .with_font_size(16.0)
        .with_font_weight(FontWeight::NORMAL);
    let mut label = TextPainter::new()
        .with_text(TextSpan::styled(TEXT, style))
        .with_text_direction(TextDirection::Ltr);
    ctx.text()
        .expect("the paint pass lends the text context")
        .measurement()
        .layout(&mut label, 0.0, width)
        .expect("the label lays out");
    Rect::from_origin_size(Point::new(AT.dx, AT.dy), label.size())
}

/// Each reason a text is not checked says its own thing.
#[test]
fn each_reason_a_text_is_not_checked_is_said_in_its_own_words() {
    for (why, phrase) in [
        (ContrastUnknown::NothingOpaqueBehind, "no opaque fill"),
        (ContrastUnknown::UnresolvedBackdrop, "not one solid colour"),
        (ContrastUnknown::UnresolvedEffect, "under an effect"),
        (
            ContrastUnknown::PaintedOver,
            "painted over the text after it",
        ),
        (ContrastUnknown::MixedWeight, "mixes bold and regular"),
        (ContrastUnknown::NonFinite, "not a finite positive number"),
    ] {
        assert!(why.to_string().contains(phrase), "{why:?}: {why}");
    }
}

/// A text of one run per `(ratio of its grey against white, size)`, in regular weight on white.
fn judged_runs(runs: &[(f64, f64)]) -> Vec<TextContrast> {
    let runs = runs.to_vec();
    judged(leaf(move |ctx| {
        fill(ctx, Color::WHITE);
        let spans = runs
            .iter()
            .map(|&(ratio, size)| {
                TextSpan::styled(
                    "A",
                    TextStyle::default()
                        .with_color(grey_against_white(ratio))
                        .with_font_size(size)
                        .with_font_weight(FontWeight::NORMAL),
                )
            })
            .collect();
        write_span(ctx, TextSpan::with_children(spans));
    }))
}

/// A text of several runs reads as the run closest to failing when every run meets its
/// requirement, and the run furthest below it when one falls below; a margin is the ratio less
/// what the size needs, and equal margins keep the first run.
#[test]
fn a_text_of_several_runs_reads_as_its_worst_run() {
    // (name, runs as (ratio, size), whether the outcome is below, the size and ratio read)
    type Row = (&'static str, Vec<(f64, f64)>, bool, f64, f64);
    let rows: Vec<Row> = vec![
        (
            "meeting: the smallest margin first",
            vec![(5.0, 16.0), (8.0, 16.0)],
            false,
            16.0,
            5.0,
        ),
        (
            "meeting: the smallest margin last",
            vec![(8.0, 16.0), (5.0, 16.0)],
            false,
            16.0,
            5.0,
        ),
        (
            "meeting: equal margins keep the first run",
            vec![(6.0, 16.0), (6.0, 18.0)],
            false,
            16.0,
            6.0,
        ),
        (
            "meeting: a margin is a difference, not a sum",
            vec![(3.5, 24.0), (4.9, 16.0)],
            false,
            16.0,
            4.9,
        ),
        (
            "meeting: a margin is a difference, not a quotient",
            vec![(9.0, 16.0), (7.0, 24.0)],
            false,
            24.0,
            7.0,
        ),
        (
            "below: the furthest below first",
            vec![(2.0, 16.0), (4.0, 16.0)],
            true,
            16.0,
            2.0,
        ),
        (
            "below: the furthest below last",
            vec![(4.0, 16.0), (2.0, 16.0)],
            true,
            16.0,
            2.0,
        ),
        (
            "below: equal margins keep the first run",
            vec![(3.0, 16.0), (3.0, 18.0)],
            true,
            16.0,
            3.0,
        ),
        (
            "below: a margin is a difference, not a sum or a quotient",
            vec![(3.9, 16.0), (2.5, 24.0)],
            true,
            16.0,
            3.9,
        ),
        (
            "a run below decides over a run that meets",
            vec![(9.0, 16.0), (3.0, 16.0)],
            true,
            16.0,
            3.0,
        ),
    ];
    for (name, runs, below, size, ratio) in rows {
        let texts = judged_runs(&runs);
        let (meets, reading) = reading_of(name, &texts);
        assert_eq!(!meets, below, "{name}: {texts:#?}");
        assert!(
            (reading.font_size - size).abs() < 1e-6,
            "{name}: {reading:?}"
        );
        assert!((reading.ratio - ratio).abs() < 5e-3, "{name}: {reading:?}");
    }
}

/// The scale `c` with `base * c + epsilon == threshold` in `f64`: a size the large-text rule
/// reads as at its threshold, one `epsilon` short of it.
fn scale_one_epsilon_short(base: f64, threshold: f64, epsilon: f64) -> f64 {
    let mut scale = ((threshold - epsilon) / base).next_down();
    for _ in 0..64 {
        if base * scale + epsilon == threshold {
            return scale;
        }
        scale = scale.next_up();
    }
    panic!("no scale puts {base} px one {epsilon} short of {threshold}");
}

/// A run's size and weight decide what it needs and whether it counts as bold, at and just
/// under each large-text threshold.
#[test]
fn a_reading_names_the_requirement_and_boldness_its_size_and_weight_give() {
    let (regular, bold) = (FontWeight::NORMAL, FontWeight::BOLD);
    let at_24 = scale_one_epsilon_short(12.0, 24.0, 1e-3);
    let at_18_66 = scale_one_epsilon_short(10.0, 18.66, 1e-3);
    // (name, size painted, the scale above it, weight, the requirement, counts as bold)
    type Row = (&'static str, f64, f64, FontWeight, f64, bool);
    let rows: [Row; 9] = [
        ("24 px bold", 24.0, 1.0, bold, 3.0, true),
        ("24 px regular", 24.0, 1.0, regular, 3.0, false),
        ("19 px bold", 19.0, 1.0, bold, 3.0, true),
        ("19 px regular", 19.0, 1.0, regular, 4.5, false),
        ("16 px bold", 16.0, 1.0, bold, 4.5, true),
        ("16 px regular", 16.0, 1.0, regular, 4.5, false),
        ("18.66 px bold", 18.66, 1.0, bold, 3.0, true),
        (
            "a size one epsilon short of 24 px is at 24 px",
            12.0,
            at_24,
            regular,
            3.0,
            false,
        ),
        (
            "a size one epsilon short of 18.66 px bold is at 18.66 px",
            10.0,
            at_18_66,
            bold,
            3.0,
            true,
        ),
    ];
    for (name, size, scale, weight, required, counts_bold) in rows {
        let texts = judged(leaf(move |ctx| {
            fill(ctx, Color::WHITE);
            ctx.canvas().scale(scale, scale);
            write(ctx, grey_against_white(3.5), size, weight);
        }));
        let (meets, reading) = reading_of(name, &texts);
        assert_eq!(reading.required, required, "{name}: {reading:?}");
        assert_eq!(meets, required == 3.0, "{name}: {reading:?}");
        assert_eq!(reading.bold, counts_bold, "{name}: {reading:?}");
    }
}

/// The ratio is WCAG's: relative luminance with the sRGB linear segment, the lighter colour
/// over the darker whichever of the two the text is.
#[test]
fn the_ratio_is_wcags_whichever_colour_is_lighter() {
    let low = Color::from_rgba_f32_array([0.03, 0.03, 0.03, 1.0]);
    let low_ratio = 1.05 / (f64::from(0.03_f32) / 12.92 + 0.05);
    for (name, text, behind, ratio) in [
        ("black on white", Color::BLACK, Color::WHITE, 21.0),
        ("white on black", Color::WHITE, Color::BLACK, 21.0),
        ("a dark grey on white", low, Color::WHITE, low_ratio),
        ("white on a dark grey", Color::WHITE, low, low_ratio),
        ("a colour on itself", low, low, 1.0),
    ] {
        let texts = judged(leaf(move |ctx| {
            fill(ctx, behind);
            write(ctx, text, 16.0, FontWeight::NORMAL);
        }));
        let (_, reading) = reading_of(name, &texts);
        assert!((reading.ratio - ratio).abs() < 1e-9, "{name}: {reading:?}");
    }
}

/// A fill whose clip or transform leaves what it covers known only in part, a flood, a rounded
/// fill and a paint that touches only the edge of a text: what each makes of the colour behind
/// the text or of what is painted over it.
#[test]
fn a_fill_is_the_colour_behind_a_text_only_where_it_surely_covers_the_whole_box() {
    let backdrop = Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop);
    let unresolved = Expect::NotChecked(ContrastUnknown::UnresolvedEffect);
    let big = Rect::from_ltrb(-200.0, -200.0, 600.0, 400.0);
    let gradient = || {
        let mut paint = Paint::fill(Color::WHITE);
        paint.shader = Some(Shader::simple_linear(
            Offset::ZERO,
            Offset::new(300.0, 0.0),
            vec![Color::WHITE, Color::BLACK],
        ));
        paint
    };
    let rows: Vec<(&'static str, TreeNode, Expect)> = vec![
        (
            "a flood under no clip covers the box",
            leaf(|ctx| {
                ctx.canvas().draw_paint(&Paint::fill(Color::WHITE));
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        (
            "a flood under a clip that leaves part of the box",
            leaf(|ctx| {
                let part = left_part(ctx);
                ctx.canvas().save();
                ctx.canvas().clip_rect(part);
                ctx.canvas().draw_paint(&Paint::fill(Color::WHITE));
                ctx.canvas().restore();
                write_black(ctx);
            }),
            backdrop,
        ),
        (
            "a colour flood in source-over is a solid colour",
            leaf(|ctx| {
                ctx.canvas().draw_color(Color::WHITE, BlendMode::SrcOver);
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        (
            "a colour flood in another blend mode is not",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                ctx.canvas().draw_color(Color::WHITE, BlendMode::Multiply);
                write_black(ctx);
            }),
            backdrop,
        ),
        (
            "a flood that is not one colour, under no clip",
            leaf(move |ctx| {
                fill(ctx, Color::WHITE);
                ctx.canvas().draw_paint(&gradient());
                write_black(ctx);
            }),
            backdrop,
        ),
        (
            "a transparent flood under a clip is skipped",
            leaf(|ctx| {
                let part = left_part(ctx);
                fill(ctx, Color::WHITE);
                ctx.canvas().save();
                ctx.canvas().clip_rect(part);
                ctx.canvas()
                    .draw_paint(&Paint::fill(Color::WHITE.with_alpha(0)));
                ctx.canvas().restore();
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        (
            "a transparent fill over part of the text is skipped",
            leaf(|ctx| {
                let part = left_part(ctx);
                fill(ctx, Color::WHITE);
                ctx.canvas()
                    .draw_rect(part, &Paint::fill(Color::WHITE.with_alpha(0)));
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        (
            "a fill under a difference clip",
            leaf(|ctx| {
                let part = left_part(ctx);
                ctx.canvas().save();
                ctx.canvas()
                    .clip_rect_with(part, ClipOp::Difference, Clip::HardEdge);
                fill(ctx, Color::WHITE);
                ctx.canvas().restore();
                write_black(ctx);
            }),
            unresolved,
        ),
        (
            "a fill under a rotation",
            leaf(move |ctx| {
                ctx.canvas().save();
                ctx.canvas().rotate(0.3);
                ctx.canvas().draw_rect(big, &Paint::fill(Color::WHITE));
                ctx.canvas().restore();
                write_black(ctx);
            }),
            unresolved,
        ),
        (
            "a rounded fill whose corners are clear of the box",
            leaf(|ctx| {
                let rounded = RRect::from_rect_circular(area(ctx), 4.0);
                ctx.canvas().draw_rrect(rounded, &Paint::fill(Color::WHITE));
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        (
            "a rounded fill whose corners reach the box",
            leaf(|ctx| {
                let rounded = RRect::from_rect_circular(area(ctx), 30.0);
                ctx.canvas().draw_rrect(rounded, &Paint::fill(Color::WHITE));
                write_black(ctx);
            }),
            backdrop,
        ),
        (
            "a rounded fill under a rotation",
            leaf(move |ctx| {
                ctx.canvas().save();
                ctx.canvas().rotate(0.3);
                ctx.canvas().draw_rrect(
                    RRect::from_rect_circular(big, 4.0),
                    &Paint::fill(Color::WHITE),
                );
                ctx.canvas().restore();
                write_black(ctx);
            }),
            unresolved,
        ),
        (
            "an opaque fill after the text that meets only its top edge",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                write_black(ctx);
                ctx.canvas().draw_rect(
                    Rect::from_ltrb(0.0, 0.0, 320.0, AT.dy),
                    &Paint::fill(Color::WHITE),
                );
            }),
            Expect::Meets(4.5),
        ),
        (
            "an opaque fill after the text that meets only its left edge",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                write_black(ctx);
                ctx.canvas().draw_rect(
                    Rect::from_ltrb(0.0, 0.0, AT.dx, 80.0),
                    &Paint::fill(Color::WHITE),
                );
            }),
            Expect::Meets(4.5),
        ),
    ];
    for (name, spec, expect) in rows {
        let texts = judged(spec);
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// What a clip, opacity, shader mask, leader or follower layer does to what is painted under it:
/// a clip at `Clip::None` or an opacity of 1 leaves it as it was, the others narrow it or leave
/// it unresolved, and a layer that moves its content moves where it is read.
#[test]
fn a_layer_changes_what_is_behind_a_text_only_as_it_clips_moves_or_blends() {
    let white = Color::WHITE;
    let backdrop = Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop);
    let unresolved = Expect::NotChecked(ContrastUnknown::UnresolvedEffect);
    let part = Rect::from_ltrb(0.0, 0.0, 30.0, 80.0);
    // The fill of the whole box, held by `wrapper`, behind a text its parent paints after it.
    let under = |wrapper: TreeNode| {
        around(
            nothing,
            wrapper.child(leaf(move |ctx| fill(ctx, white))),
            write_black,
        )
    };
    let full_opacity = || {
        let mut opacity = RenderOpacity::new(1.0);
        let _ = opacity.set_always_needs_compositing(true);
        opacity
    };
    let (link, follow) = (flui_layer::LayerLink::new(), flui_layer::LayerLink::new());
    let rows: Vec<(&'static str, TreeNode, Expect)> = vec![
        (
            "a clip rect layer that clips cuts the fill",
            under(box_node(RenderClipRect::hard_edge().with_clip_shape(part))),
            backdrop,
        ),
        (
            "a clip rect layer at Clip::None does not",
            under(box_node(
                RenderClipRect::new(Clip::None).with_clip_shape(part),
            )),
            Expect::Meets(4.5),
        ),
        (
            "a clip rrect layer that clips cuts the fill",
            under(box_node(
                RenderClipRRect::hard_edge().with_clip_shape(RRect::from_rect_circular(part, 4.0)),
            )),
            backdrop,
        ),
        (
            "a clip rrect layer at Clip::None does not",
            under(box_node(
                RenderClipRRect::new(Clip::None)
                    .with_clip_shape(RRect::from_rect_circular(part, 4.0)),
            )),
            Expect::Meets(4.5),
        ),
        (
            "a clip path layer at Clip::None does not",
            under(box_node(
                RenderClipPath::new(Clip::None).with_clip_shape(Path::oval(part)),
            )),
            Expect::Meets(4.5),
        ),
        (
            "a clip superellipse layer that clips leaves the fill unresolved",
            under(box_node(
                RenderClipRSuperellipse::hard_edge()
                    .with_clip_shape(RSuperellipse::from_rect_circular(part, 4.0)),
            )),
            unresolved,
        ),
        (
            "a clip superellipse layer at Clip::None does not",
            under(box_node(
                RenderClipRSuperellipse::new(Clip::None)
                    .with_clip_shape(RSuperellipse::from_rect_circular(part, 4.0)),
            )),
            Expect::Meets(4.5),
        ),
        (
            "a fill under an opacity layer of 1, behind the text",
            under(box_node(full_opacity())),
            Expect::Meets(4.5),
        ),
        (
            "a text under an opacity layer of 1",
            around(
                move |ctx| fill(ctx, white),
                box_node(full_opacity()).child(leaf(write_black)),
                nothing,
            ),
            Expect::Meets(4.5),
        ),
        (
            "a text under a shader mask",
            around(
                move |ctx| fill(ctx, white),
                box_node(RenderShaderMask::new(Shader::simple_linear(
                    Offset::ZERO,
                    Offset::new(100.0, 0.0),
                    vec![Color::WHITE, Color::BLACK],
                )))
                .child(leaf(write_black)),
                nothing,
            ),
            unresolved,
        ),
        (
            "a backdrop filter clear of the text",
            around(
                move |ctx| fill(ctx, white),
                box_node(RenderPadding::new(EdgeInsets::only_left(200.0))).child(
                    box_node(RenderBackdropFilter::new(ImageFilter::blur(4.0)))
                        .child(leaf(nothing)),
                ),
                write_black,
            ),
            Expect::Meets(4.5),
        ),
        (
            "a text the padding moves, with no leader around it",
            around(
                |ctx| {
                    ctx.canvas().draw_rect(
                        Rect::from_ltrb(100.0, 0.0, 200.0, 80.0),
                        &Paint::fill(Color::WHITE),
                    );
                },
                box_node(RenderPadding::new(EdgeInsets::only_left(100.0))).child(leaf(write_black)),
                nothing,
            ),
            Expect::Meets(4.5),
        ),
        (
            "a text under a leader is read where the scene draws it: shifted by the leader's offset",
            around(
                |ctx| {
                    ctx.canvas().draw_rect(
                        Rect::from_ltrb(200.0, 0.0, 300.0, 80.0),
                        &Paint::fill(Color::WHITE),
                    );
                },
                box_node(RenderPadding::new(EdgeInsets::only_left(100.0)))
                    .child(box_node(RenderLeaderLayer::new(link)).child(leaf(write_black))),
                nothing,
            ),
            Expect::Meets(4.5),
        ),
        (
            "a text under a follower the layer places against its leader",
            around(
                |ctx| {
                    ctx.canvas().draw_rect(
                        Rect::from_ltrb(100.0, 0.0, 300.0, 80.0),
                        &Paint::fill(Color::WHITE),
                    );
                },
                box_node(RenderFlex::row())
                    .child(box_node(RenderLeaderLayer::new(follow)).child(box_node(
                        RenderCustomPaint::new(None, None, Size::new(40.0, 40.0)),
                    )))
                    .child(
                        box_node(RenderRepaintBoundary::new()).child(
                            box_node(
                                RenderFollowerLayer::new(follow)
                                    .with_offset(Offset::new(100.0, 0.0)),
                            )
                            .child(leaf(write_black)),
                        ),
                    ),
                nothing,
            ),
            Expect::Meets(4.5),
        ),
    ];
    for (name, spec, expect) in rows {
        let texts = judged(spec);
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// What the canvas ops of one chunk do to the scope the text is read in: a save layer's
/// restore, the bounds of a layer that composites like source-over, and each clip shape.
#[test]
fn the_canvas_scope_a_text_is_read_in_follows_its_saves_and_clips() {
    let backdrop = Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop);
    let unresolved = Expect::NotChecked(ContrastUnknown::UnresolvedEffect);
    let rows: Vec<(&'static str, TreeNode, Expect)> = vec![
        (
            "a translucent save layer is closed by its restore",
            leaf(|ctx| {
                fill(ctx, Color::WHITE);
                ctx.canvas().save_layer_alpha(None, 128);
                ctx.canvas().draw_rect(
                    Rect::from_ltrb(200.0, 30.0, 210.0, 40.0),
                    &Paint::fill(Color::BLACK),
                );
                ctx.canvas().restore();
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        (
            "a save layer's restore returns to the state the layer opened over, not the one before it",
            leaf(|ctx| {
                let left = left_part(ctx);
                fill(ctx, Color::WHITE);
                ctx.canvas().clip_rect(left);
                ctx.canvas().save_layer_alpha(None, 128);
                ctx.canvas().draw_rect(
                    Rect::from_ltrb(10.0, 30.0, 20.0, 40.0),
                    &Paint::fill(Color::BLACK),
                );
                ctx.canvas().restore();
                write_black(ctx);
                ctx.canvas().draw_rect(
                    Rect::from_ltrb(40.0, 0.0, 320.0, 80.0),
                    &Paint::fill(Color::WHITE),
                );
            }),
            Expect::Meets(4.5),
        ),
        (
            "a source-over save layer holding nothing changes nothing under its bounds",
            leaf(|ctx| {
                let left = left_part(ctx);
                fill(ctx, Color::WHITE);
                ctx.canvas()
                    .save_layer(Some(left), &Paint::fill(Color::BLACK));
                ctx.canvas().restore();
                write_black(ctx);
            }),
            Expect::Meets(4.5),
        ),
        (
            "a fill under a rect clip",
            leaf(|ctx| {
                let part = left_part(ctx);
                ctx.canvas().save();
                ctx.canvas().clip_rect(part);
                fill(ctx, Color::WHITE);
                ctx.canvas().restore();
                write_black(ctx);
            }),
            backdrop,
        ),
        (
            "a fill under a rounded rect clip",
            leaf(|ctx| {
                let part = RRect::from_rect_circular(left_part(ctx), 2.0);
                ctx.canvas().save();
                ctx.canvas().clip_rrect(part);
                fill(ctx, Color::WHITE);
                ctx.canvas().restore();
                write_black(ctx);
            }),
            backdrop,
        ),
        (
            "a fill under a superellipse clip",
            leaf(|ctx| {
                let part = RSuperellipse::from_rect_circular(left_part(ctx), 2.0);
                ctx.canvas().save();
                ctx.canvas().clip_rsuperellipse(part);
                fill(ctx, Color::WHITE);
                ctx.canvas().restore();
                write_black(ctx);
            }),
            unresolved,
        ),
    ];
    for (name, spec, expect) in rows {
        let texts = judged(spec);
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// What a shape the walk does not resolve to one colour does to a text: painted over its box
/// before the text it leaves the text unjudged, and painted away from it, it does not.
#[test]
fn a_shape_painted_over_the_text_box_is_unresolved_and_one_away_from_it_is_not() {
    type Shape = fn(&mut CustomPaintContext<'_>, Rect<f64>);
    let shapes: Vec<(&'static str, Shape)> = vec![
        ("a circle", |ctx, at| {
            ctx.canvas()
                .draw_circle(at.center(), at.width() / 2.0, &Paint::fill(Color::BLACK));
        }),
        ("an oval", |ctx, at| {
            ctx.canvas().draw_oval(at, &Paint::fill(Color::BLACK));
        }),
        ("an arc", |ctx, at| {
            ctx.canvas()
                .draw_arc(at, 0.0, 3.0, true, &Paint::fill(Color::BLACK));
        }),
        ("a line", |ctx, at| {
            ctx.canvas().draw_line(
                at.top_left(),
                at.bottom_right(),
                &Paint::stroke(Color::BLACK, 1.0),
            );
        }),
        ("a path", |ctx, at| {
            ctx.canvas()
                .draw_path(&Path::rectangle(at), &Paint::fill(Color::BLACK));
        }),
        ("a ring", |ctx, at| {
            ctx.canvas().draw_drrect(
                RRect::from_rect_circular(at, 2.0),
                RRect::from_rect_circular(at.inflate(-1.0, -1.0), 1.0),
                &Paint::fill(Color::BLACK),
            );
        }),
        ("a shadow", |ctx, at| {
            ctx.canvas()
                .draw_shadow(&Path::rectangle(at), Color::BLACK, 1.0);
        }),
        ("an image", |ctx, at| {
            ctx.canvas()
                .draw_image(Image::from_rgba8(1, 1, vec![0, 0, 0, 255]), at, None);
        }),
        ("points", |ctx, at| {
            ctx.canvas().draw_points_with_mode(
                PointMode::Points,
                vec![at.top_left(), at.bottom_right()],
                &Paint::stroke(Color::BLACK, 1.0),
            );
        }),
        ("vertices", |ctx, at| {
            ctx.canvas().draw_vertices(
                vec![at.top_left(), at.top_right(), at.bottom_right()],
                None,
                None,
                vec![0, 1, 2],
                &Paint::fill(Color::BLACK),
            );
        }),
    ];
    // Paints whose sprites land where their own transforms put them touch every text.
    let anywhere: Vec<(&'static str, Shape)> = vec![
        ("an atlas", |ctx, at| {
            ctx.canvas().draw_atlas(
                Image::from_rgba8(1, 1, vec![0, 0, 0, 255]),
                vec![Rect::from_ltrb(0.0, 0.0, 1.0, 1.0)],
                vec![Matrix4::translation(at.left(), at.top(), 0.0)],
                None,
                BlendMode::SrcOver,
                None,
            );
        }),
        ("an image region", |ctx, at| {
            ctx.canvas().draw_image_region(
                Image::from_rgba8(1, 1, vec![0, 0, 0, 255]),
                Rect::from_ltrb(0.0, 0.0, 1.0, 1.0),
                at,
                None,
            );
        }),
    ];
    let backdrop = Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop);
    let scene = |shape: Shape, at_text: bool| {
        leaf(move |ctx| {
            fill(ctx, Color::WHITE);
            let text = black_text_box(ctx);
            let at = if at_text {
                Rect::from_ltrb(
                    text.left() + 5.0,
                    text.top() + 5.0,
                    text.left() + 15.0,
                    text.top() + 10.0,
                )
            } else {
                Rect::from_ltrb(200.0, 30.0, 210.0, 40.0)
            };
            shape(ctx, at);
            write_black(ctx);
        })
    };
    for (name, shape) in shapes {
        for (place, at_text, expect) in [
            ("over the text", true, backdrop),
            ("away from it", false, Expect::Meets(4.5)),
        ] {
            let texts = judged(scene(shape, at_text));
            assert_eq!(found(name, &texts), expect, "{name} {place}: {texts:#?}");
        }
    }
    for (name, shape) in anywhere {
        for at_text in [true, false] {
            let texts = judged(scene(shape, at_text));
            assert_eq!(found(name, &texts), backdrop, "{name}: {texts:#?}");
        }
    }
}

/// A shape's extent is where it may touch: each edge of a tiny shape reaches the text box by
/// its own coordinate, and a stroke, a spread or a line's width reaches further.
#[test]
fn a_shapes_reach_is_its_extent_plus_its_stroke_width_or_spread() {
    let backdrop = Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop);
    // Each probe paints one shape placed against the text box (left, top, right, bottom).
    type Probe = fn(&mut CustomPaintContext<'_>, Rect<f64>);
    let probes: Vec<(&'static str, Probe, Expect)> = vec![
        (
            "a circle just right of the box reaches back into it",
            |ctx, text| {
                let mid = f64::midpoint(text.top(), text.bottom());
                ctx.canvas().draw_circle(
                    Point::new(text.right() + 0.25, mid),
                    0.5,
                    &Paint::fill(Color::BLACK),
                );
            },
            backdrop,
        ),
        (
            "a circle just left of the box reaches into it",
            |ctx, text| {
                let mid = f64::midpoint(text.top(), text.bottom());
                ctx.canvas().draw_circle(
                    Point::new(text.left() - 0.25, mid),
                    0.5,
                    &Paint::fill(Color::BLACK),
                );
            },
            backdrop,
        ),
        (
            "a circle just under the box reaches into it",
            |ctx, text| {
                let mid = f64::midpoint(text.left(), text.right());
                ctx.canvas().draw_circle(
                    Point::new(mid, text.bottom() + 0.25),
                    0.5,
                    &Paint::fill(Color::BLACK),
                );
            },
            backdrop,
        ),
        (
            "a circle just over the box reaches into it",
            |ctx, text| {
                let mid = f64::midpoint(text.left(), text.right());
                ctx.canvas().draw_circle(
                    Point::new(mid, text.top() - 0.25),
                    0.5,
                    &Paint::fill(Color::BLACK),
                );
            },
            backdrop,
        ),
        (
            "a circle a radius clear of the box does not touch it",
            |ctx, text| {
                let mid = f64::midpoint(text.top(), text.bottom());
                ctx.canvas().draw_circle(
                    Point::new(text.right() + 1.0, mid),
                    0.5,
                    &Paint::fill(Color::BLACK),
                );
            },
            Expect::Meets(4.5),
        ),
        (
            "a stroked circle reaches its radius plus its stroke",
            |ctx, text| {
                let mid = f64::midpoint(text.top(), text.bottom());
                ctx.canvas().draw_circle(
                    Point::new(text.right() + 2.0, mid),
                    0.5,
                    &Paint::stroke(Color::BLACK, 2.0),
                );
            },
            backdrop,
        ),
        (
            "a line reaches its stroke width even in a fill paint",
            |ctx, text| {
                let mut paint = Paint::fill(Color::BLACK);
                paint.stroke_width = 6.0;
                let y = text.top() - 2.0;
                ctx.canvas().draw_line(
                    Point::new(text.left(), y),
                    Point::new(text.right(), y),
                    &paint,
                );
            },
            backdrop,
        ),
        (
            "a stroked rect over the box reaches its stroke width",
            |ctx, text| {
                ctx.canvas().draw_rect(
                    Rect::from_ltrb(
                        text.left(),
                        text.top() - 12.0,
                        text.right(),
                        text.top() - 2.0,
                    ),
                    &Paint::stroke(Color::BLACK, 6.0),
                );
            },
            backdrop,
        ),
        (
            "a shadow reaches four pixels per unit of elevation",
            |ctx, text| {
                ctx.canvas().draw_shadow(
                    &Path::rectangle(Rect::from_ltrb(
                        text.left(),
                        text.top() - 20.0,
                        text.right(),
                        text.top() - 7.0,
                    )),
                    Color::BLACK,
                    2.0,
                );
            },
            backdrop,
        ),
    ];
    for (name, probe, expect) in probes {
        let texts = judged(leaf(move |ctx| {
            fill(ctx, Color::WHITE);
            let text = black_text_box(ctx);
            probe(ctx, text);
            write_black(ctx);
        }));
        assert_eq!(found(name, &texts), expect, "{name}: {texts:#?}");
    }
}

/// A text is judged only when it has glyphs to read, and what it paints is something later
/// texts and fills are read against.
#[test]
fn a_text_is_judged_when_it_paints_and_is_itself_painted_over_what_is_behind_it() {
    let black_over_white = |ctx: &mut CustomPaintContext<'_>| {
        fill(ctx, Color::WHITE);
        write_black(ctx);
    };
    // A white-space text paints no ink to read.
    let spaces = judged(leaf(|ctx| {
        fill(ctx, Color::WHITE);
        write_span(
            ctx,
            TextSpan::styled(
                "   ",
                TextStyle::default()
                    .with_color(Color::BLACK)
                    .with_font_size(16.0),
            ),
        );
    }));
    assert_eq!(found("a white-space text", &spaces), Expect::NoText);

    // A second text painted over the first hides it, and is itself over something unresolved.
    let two = judged(leaf(move |ctx| {
        black_over_white(ctx);
        write_black(ctx);
    }));
    let outcomes: Vec<&ContrastOutcome> = two.iter().map(|text| &text.outcome).collect();
    assert_eq!(
        outcomes,
        [
            &ContrastOutcome::NotChecked(ContrastUnknown::PaintedOver),
            &ContrastOutcome::NotChecked(ContrastUnknown::UnresolvedBackdrop),
        ],
        "{two:#?}"
    );

    // A translucent fill is the last solid behind the text; the opaque one under it shows through.
    let texts = judged(leaf(|ctx| {
        fill(ctx, Color::WHITE);
        fill(ctx, Color::BLACK.with_alpha(128));
        write_black(ctx);
    }));
    let (meets, reading) = reading_of("a translucent fill over an opaque one", &texts);
    assert!(meets, "{reading:?}");
    assert_eq!(
        reading.background,
        Color::BLACK.with_alpha(128).blend_over(Color::WHITE)
    );
}

/// What a paint whose geometry is not finite does: a text painted before it is painted over by
/// it, wherever the text is. A text whose own box is not finite is left out of the answer.
#[test]
fn a_paint_whose_extent_is_not_finite_touches_every_text_before_it() {
    // Each scale has a finite determinant, so the transform paints; three of them multiply past `f64`.
    let huge = Matrix4::scaling(1e103, 1e103, 1.0);
    let under_huge = |child: TreeNode| {
        box_node(RenderTransform::new(huge)).child(
            box_node(RenderTransform::new(huge))
                .child(box_node(RenderTransform::new(huge)).child(child)),
        )
    };
    let rows: Vec<(&'static str, TreeNode)> = vec![
        ("a text", under_huge(leaf(write_black))),
        (
            // Its corners at 0 times infinity are not numbers at all.
            "a circle",
            under_huge(leaf(|ctx| {
                ctx.canvas()
                    .draw_circle(Point::new(0.5, 0.5), 0.5, &Paint::fill(Color::BLACK));
            })),
        ),
    ];
    for (name, child) in rows {
        let texts = judged(around(
            |ctx| {
                fill(ctx, Color::WHITE);
                write_black(ctx);
            },
            child,
            nothing,
        ));
        let outcomes: Vec<&ContrastOutcome> = texts.iter().map(|text| &text.outcome).collect();
        assert_eq!(
            outcomes,
            [&ContrastOutcome::NotChecked(ContrastUnknown::PaintedOver)],
            "{name}: {texts:#?}"
        );
    }
}

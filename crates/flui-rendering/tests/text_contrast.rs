//! The contrast rule over a painted frame, through the public API: a text's colour against
//! the fill painted behind it, read from the frame's scene, at WCAG's large-text thresholds with
//! the ratio unrounded; a transparent run not judged; a text whose colours the commands do not
//! determine reported as not
//! checked, with the reason its layers, clips, save layers, transforms and later paints give.

use std::fmt;
use std::rc::Rc;

use flui_foundation::TextDirection;
use flui_foundation::geometry::{DevicePixelRatio, Matrix4, Offset, Point, Rect, Size};
use flui_objects::{
    RenderBackdropFilter, RenderClipPath, RenderClipRect, RenderCustomPaint, RenderOpacity,
    RenderTransform,
};
use flui_painting::TextPainter;
use flui_painting::paint::{BlendMode, ImageFilter, Paint, Path, Shader};
use flui_painting::styling::{Color, linear_to_srgb};
use flui_painting::typography::{FontWeight, TextSpan, TextStyle};
use flui_rendering::audit::{ContrastOutcome, ContrastUnknown, TextContrast};
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

//! The contrast rule over a painted frame, through the public API: a text's colour against
//! the fill painted behind it, read from the frame's scene, at WCAG's large-text thresholds and
//! its 0.01 tolerance, and a text whose background the commands do not determine reported as
//! not checked.

use flui_foundation::TextDirection;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_objects::RenderCustomPaint;
use flui_painting::TextPainter;
use flui_painting::paint::{Paint, Shader};
use flui_painting::styling::{Color, linear_to_srgb};
use flui_painting::typography::{FontWeight, TextSpan, TextStyle};
use flui_rendering::audit::{ContrastOutcome, ContrastUnknown, TextContrast};
use flui_rendering::{
    PipelineOwner, TextContextHandle,
    constraints::BoxConstraints,
    delegates::{CustomPaintContext, CustomPainter},
    testing::{SceneHost, box_node, tree},
};

/// What is painted behind the text.
#[derive(Debug, Clone, Copy)]
enum Behind {
    /// One opaque fill over the whole box.
    Fill(Color),
    /// An opaque fill over the box's left 30 px, part of the text only.
    PartFill(Color),
    /// A gradient over the whole box.
    Gradient,
    /// Nothing.
    Nothing,
}

/// Paints `behind`, then `TEXT` in `color` at `size`, `weight`.
#[derive(Debug)]
struct Sample {
    behind: Behind,
    color: Color,
    size: f64,
    weight: FontWeight,
}

const TEXT: &str = "Contrast";

impl CustomPainter for Sample {
    fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
        let area = Rect::from_origin_size(Point::ZERO, ctx.size());
        // The text starts at x = 4 and runs past x = 30.
        let part = Rect::from_ltrb(0.0, 0.0, 30.0, area.height());
        match self.behind {
            Behind::Fill(color) => ctx.canvas().draw_rect(area, &Paint::fill(color)),
            Behind::PartFill(color) => ctx.canvas().draw_rect(part, &Paint::fill(color)),
            Behind::Gradient => {
                let mut paint = Paint::fill(Color::WHITE);
                paint.shader = Some(Shader::simple_linear(
                    Offset::ZERO,
                    Offset::new(area.width(), 0.0),
                    vec![Color::WHITE, Color::BLACK],
                ));
                ctx.canvas().draw_rect(area, &paint);
            }
            Behind::Nothing => {}
        }
        let style = TextStyle::default()
            .with_color(self.color)
            .with_font_size(self.size)
            .with_font_weight(self.weight);
        let mut label = TextPainter::new()
            .with_text(TextSpan::styled(TEXT, style))
            .with_text_direction(TextDirection::Ltr);
        ctx.text()
            .expect("the paint pass lends the text context")
            .measurement()
            .layout(&mut label, 0.0, area.width())
            .expect("the label lays out");
        label.paint(ctx.canvas(), Offset::new(4.0, 4.0));
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        false
    }
}

/// The texts `sample`'s one frame paints, judged.
fn audit(sample: Sample) -> Vec<TextContrast> {
    let text = TextContextHandle::standalone();
    let mut owner = PipelineOwner::new(text);
    let painter =
        RenderCustomPaint::new(Some(std::rc::Rc::new(sample)), None, Size::new(320.0, 80.0));
    let (root, _) = tree::mount(&mut owner, box_node(painter));
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::loose(Size::new(400.0, 400.0))));
    let mut host = SceneHost::new();
    host.run_frame(&mut owner)
        .expect("the frame paints")
        .expect("the first frame produces a layer tree");
    owner
        .audit_text_contrast(host.version())
        .expect("the version holds the owner's frame")
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

/// What the rule should find for one text.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Expect {
    Meets(f64),
    Below(f64),
    NotChecked(ContrastUnknown),
}

/// One row per input: the pair, the size, the weight and what is behind decide the outcome.
#[test]
fn contrast_is_judged_against_the_threshold_its_size_and_weight_need() {
    let regular = FontWeight::NORMAL;
    let bold = FontWeight::BOLD;
    let white = Behind::Fill(Color::WHITE);
    type Row = (&'static str, Behind, Color, f64, FontWeight, Expect);
    let table: [Row; 13] = [
        (
            "a failing pair",
            white,
            grey_against_white(2.0),
            16.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "a passing pair",
            white,
            Color::BLACK,
            16.0,
            regular,
            Expect::Meets(4.5),
        ),
        (
            "24 px is large text",
            white,
            grey_against_white(3.5),
            24.0,
            regular,
            Expect::Meets(3.0),
        ),
        (
            "23 px regular is not",
            white,
            grey_against_white(3.5),
            23.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "19 px bold is large text",
            white,
            grey_against_white(3.5),
            19.0,
            bold,
            Expect::Meets(3.0),
        ),
        (
            "18 px bold is not",
            white,
            grey_against_white(3.5),
            18.0,
            bold,
            Expect::Below(4.5),
        ),
        (
            "19 px regular is not",
            white,
            grey_against_white(3.5),
            19.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "within the 0.01 tolerance",
            white,
            grey_against_white(4.495),
            16.0,
            regular,
            Expect::Meets(4.5),
        ),
        (
            "past the 0.01 tolerance",
            white,
            grey_against_white(4.485),
            16.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "a half-transparent black is the grey it shows over white, under 4.5",
            white,
            Color::BLACK.with_alpha(128),
            16.0,
            regular,
            Expect::Below(4.5),
        ),
        (
            "a fill over part of the text",
            Behind::PartFill(Color::WHITE),
            Color::BLACK,
            16.0,
            regular,
            Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop),
        ),
        (
            "a gradient behind the text",
            Behind::Gradient,
            Color::BLACK,
            16.0,
            regular,
            Expect::NotChecked(ContrastUnknown::UnresolvedBackdrop),
        ),
        (
            "nothing behind the text",
            Behind::Nothing,
            Color::BLACK,
            16.0,
            regular,
            Expect::NotChecked(ContrastUnknown::NothingOpaqueBehind),
        ),
    ];
    for (name, behind, color, size, weight, expect) in table {
        let texts = audit(Sample {
            behind,
            color,
            size,
            weight,
        });
        let [text] = texts.as_slice() else {
            panic!("{name}: one text is painted: {texts:#?}");
        };
        assert_eq!(text.text, TEXT, "{name}");
        let found = match text.outcome {
            ContrastOutcome::Meets(reading) => Expect::Meets(reading.required),
            ContrastOutcome::Below(reading) => Expect::Below(reading.required),
            ContrastOutcome::NotChecked(why) => Expect::NotChecked(why),
            _ => panic!("{name}: an outcome this test does not know: {text:#?}"),
        };
        assert_eq!(found, expect, "{name}: {text:#?}");
    }
}

/// The reading names the ratio, the composited colours and the size it judged.
#[test]
fn a_reading_names_the_ratio_and_the_colours_it_compared() {
    let texts = audit(Sample {
        behind: Behind::Fill(Color::WHITE),
        color: grey_against_white(3.0),
        size: 16.0,
        weight: FontWeight::NORMAL,
    });
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

//! Shaping a paragraph on a realm's [`TextContext`] (ADR-0092 §1).
//!
//! One ranged builder per paragraph: the defaults first, then each span's
//! style over its byte range. The result answers the paragraph metrics
//! [`TextLayout::metrics`](crate::TextLayout::metrics) answers on the
//! cosmic-text path; no parity with those numbers is claimed.

use std::borrow::Cow;
use std::fmt;

use crate::styling::Color;
use crate::typography::{FontStyle, TextDirection, TextStyle};
use parley::style::{
    FontFamily, FontFamilyName, FontStyle as ParleyFontStyle, FontWeight, GenericFamily,
    LineHeight, StyleProperty,
};
use parley::{Alignment, AlignmentOptions, Layout};

use crate::text_layout::{TextContext, TextLayoutResult, paint_color};

/// What one paragraph is shaped from.
#[derive(Clone, Copy, Debug)]
pub struct ParagraphSpec<'a> {
    /// The styled spans, in order; their texts concatenate to the paragraph.
    /// The same shape `TextLayout::from_spans` takes.
    pub spans: &'a [(String, Option<TextStyle>)],
    /// The style applied where a span sets nothing of its own.
    pub default_style: Option<&'a TextStyle>,
    /// The paragraph's font size, in logical pixels.
    pub font_size: f32,
    /// The width lines break at; `None` breaks only at hard breaks.
    pub max_width: Option<f32>,
    /// The line height in logical pixels; `None` is `1.2 × font_size`.
    pub line_height: Option<f32>,
    /// The edge lines align to: `Ltr` aligns left, `Rtl` aligns right.
    ///
    /// It does not set the bidi base direction. Parley 0.11 takes that from
    /// the paragraph's first strong character and has no way to override it,
    /// so Latin-first text under `Rtl` is still ordered as an LTR paragraph
    /// (flui-painting `ARCHITECTURE.md`, mapping decision 12).
    pub direction: TextDirection,
}

/// A shaped, line-broken paragraph.
pub struct ParagraphLayout {
    layout: Layout<SpanBrush>,
    text: String,
    line_height: f32,
}

impl ParagraphLayout {
    /// The paragraph's metrics, read from its laid-out lines.
    ///
    /// Empty text has no line to read, so it reports one line box of the
    /// paragraph's line height with the baseline at `0.8 ×` that height, as
    /// the cosmic-text path does.
    #[must_use]
    pub fn metrics(&self) -> TextLayoutResult {
        let first = if self.text.is_empty() {
            None
        } else {
            self.layout.get(0)
        };
        let Some(first) = first else {
            return TextLayoutResult {
                width: 0.0,
                height: f64::from(self.line_height),
                line_count: 1,
                max_line_width: 0.0,
                alphabetic_baseline: f64::from(self.line_height * 0.8),
                ideographic_baseline: f64::from(self.line_height),
                truncated: false,
            };
        };
        let line = first.metrics();
        let width = self.layout.width();
        TextLayoutResult {
            width: f64::from(width),
            height: f64::from(self.layout.height()),
            line_count: self.layout.len().max(1),
            max_line_width: f64::from(width),
            alphabetic_baseline: f64::from(line.baseline),
            ideographic_baseline: f64::from(line.block_max_coord),
            truncated: false,
        }
    }
}

impl fmt::Debug for ParagraphLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParagraphLayout")
            .field("text", &self.text)
            .field("lines", &self.layout.len())
            .finish_non_exhaustive()
    }
}

/// The colour a span paints with, carried through shaping per glyph run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SpanBrush(Option<Color>);

impl TextContext {
    /// Shapes and line-breaks `paragraph` on this context's fonts.
    pub fn shape(&mut self, paragraph: &ParagraphSpec<'_>) -> ParagraphLayout {
        debug_assert!(
            paragraph.font_size > 0.0 && paragraph.font_size.is_finite(),
            "ParagraphSpec font_size must be positive and finite, got {}",
            paragraph.font_size
        );
        let text: String = paragraph
            .spans
            .iter()
            .map(|(text, _)| text.as_str())
            .collect();
        let line_height = paragraph.line_height.unwrap_or(paragraph.font_size * 1.2);

        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, &text, 1.0, true);
        builder.push_default(family(None));
        builder.push_default(StyleProperty::FontSize(paragraph.font_size));
        builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(line_height)));
        if let Some(style) = paragraph.default_style {
            for property in properties(style) {
                builder.push_default(property);
            }
        }
        let mut start = 0;
        for (span, style) in paragraph.spans {
            let range = start..start + span.len();
            start = range.end;
            if let Some(style) = style {
                for property in properties(style) {
                    builder.push(property, range.clone());
                }
            }
        }
        let mut layout = builder.build(&text);
        layout.break_all_lines(paragraph.max_width);
        let alignment = match paragraph.direction {
            TextDirection::Ltr => Alignment::Left,
            TextDirection::Rtl => Alignment::Right,
        };
        layout.align(alignment, AlignmentOptions::default());
        ParagraphLayout {
            layout,
            text,
            line_height,
        }
    }
}

/// The family list a style asks for: its family, its fallbacks, then
/// sans-serif, so a family the collection lacks shapes in the default face.
fn family(style: Option<&TextStyle>) -> StyleProperty<'static, SpanBrush> {
    let named = style
        .into_iter()
        .flat_map(|style| style.font_family.iter().chain(&style.font_family_fallback))
        .map(|name| FontFamilyName::Named(Cow::Owned(name.clone())));
    let families: Vec<_> = named
        .chain([FontFamilyName::Generic(GenericFamily::SansSerif)])
        .collect();
    StyleProperty::FontFamily(FontFamily::List(Cow::Owned(families)))
}

/// The Parley properties `style` sets; a field left unset adds nothing.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64 style values narrow to Parley's f32 layout space"
)]
fn properties(style: &TextStyle) -> Vec<StyleProperty<'static, SpanBrush>> {
    let mut properties = Vec::new();
    if style.font_family.is_some() {
        properties.push(family(Some(style)));
    }
    if let Some(weight) = style.font_weight {
        properties.push(StyleProperty::FontWeight(FontWeight::new(f32::from(
            weight.value(),
        ))));
    }
    if let Some(font_style) = style.font_style {
        properties.push(StyleProperty::FontStyle(match font_style {
            FontStyle::Normal => ParleyFontStyle::Normal,
            FontStyle::Italic => ParleyFontStyle::Italic,
        }));
    }
    if let Some(size) = style.font_size {
        properties.push(StyleProperty::FontSize(size as f32));
    }
    if let Some(spacing) = style.letter_spacing {
        properties.push(StyleProperty::LetterSpacing(spacing as f32));
    }
    if let Some(height) = style.height {
        properties.push(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            height as f32,
        )));
    }
    if let Some(color) = paint_color(style) {
        properties.push(StyleProperty::Brush(SpanBrush(Some(color))));
    }
    properties
}

#[cfg(test)]
mod tests {
    use crate::typography::{TextDirection, TextStyle};

    use super::{ParagraphLayout, ParagraphSpec};
    use crate::text_layout::{FontCollection, TextContext};

    const WIDTH: f32 = 400.0;

    fn shaped(text: &str, direction: TextDirection) -> ParagraphLayout {
        let spans: Vec<(String, Option<TextStyle>)> = vec![(text.to_owned(), None)];
        TextContext::new(&FontCollection::new()).shape(&ParagraphSpec {
            spans: &spans,
            default_style: None,
            font_size: 16.0,
            max_width: Some(WIDTH),
            line_height: None,
            direction,
        })
    }

    fn first_line_offset(paragraph: &ParagraphLayout) -> f32 {
        paragraph
            .layout
            .get(0)
            .expect("BUG: non-empty text lays out at least one line")
            .metrics()
            .offset
    }

    /// `Rtl` pushes a line to the right edge and nothing more: Parley 0.11
    /// resolves the base direction from the first strong character, so a
    /// Latin-first paragraph stays LTR under `Rtl` and a Hebrew-first one is
    /// RTL under `Ltr`. SkParagraph would order the first as an RTL
    /// paragraph; this pins the divergence until the base direction can be
    /// set (ADR-0092 §10 step 5).
    #[test]
    fn rtl_aligns_lines_right_without_setting_the_base_direction() {
        let ltr = shaped("abc 123 !", TextDirection::Ltr);
        let rtl = shaped("abc 123 !", TextDirection::Rtl);

        assert!(
            first_line_offset(&ltr).abs() < f32::EPSILON,
            "an Ltr line starts at the left edge, got {}",
            first_line_offset(&ltr)
        );
        assert!(
            first_line_offset(&rtl) > 0.0,
            "an Rtl line is pushed toward the right edge, got {}",
            first_line_offset(&rtl)
        );
        assert!(
            !rtl.layout.is_rtl(),
            "Latin-first text keeps an LTR base direction under Rtl"
        );
        assert!(
            shaped("\u{05E9}\u{05DC}\u{05D5}\u{05DD} abc", TextDirection::Ltr)
                .layout
                .is_rtl(),
            "Hebrew-first text takes an RTL base direction under Ltr"
        );
    }
}

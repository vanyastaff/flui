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
use cosmic_text::fontdb::Family;
use parley::fontique::Collection;
use parley::style::{
    FontFamily, FontFamilyName, FontStyle as ParleyFontStyle, FontWeight, GenericFamily,
    LineHeight, OverflowWrap, StyleProperty,
};
use parley::{Alignment, AlignmentOptions, Layout};

use crate::text_layout::font_resolve::resolve_family_name;
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
    /// The line height in logical pixels; `None` is `1.2 ×` each run's own
    /// font size, so a larger span grows its line box.
    pub line_height: Option<f32>,
    /// The edge lines align to: `Ltr` aligns left, `Rtl` aligns right.
    ///
    /// It does not set the bidi base direction. Parley 0.11 takes that from
    /// the paragraph's first strong character and has no way to override it,
    /// so Latin-first text under `Rtl` is still ordered as an LTR paragraph
    /// (flui-painting `ARCHITECTURE.md`, mapping decision 12).
    pub direction: TextDirection,
    /// The lines the paragraph keeps; `None` or `Some(0)` keeps every line,
    /// as `TextLayout::from_spans` does. Lines past
    /// it are still shaped, but the metrics stop at it and report
    /// `truncated`. No ellipsis is shaped into the last kept line
    /// (flui-painting `ARCHITECTURE.md`, mapping decision 15).
    pub max_lines: Option<usize>,
}

/// A shaped, line-broken paragraph.
pub struct ParagraphLayout {
    layout: Layout<SpanBrush>,
    text: String,
    line_height: f32,
    max_lines: Option<usize>,
}

impl ParagraphLayout {
    /// The paragraph's metrics, read from its laid-out lines.
    ///
    /// Empty text lays out one empty line in the paragraph's font, so it
    /// reports the line box and baseline a line of text in that font has
    /// (flui-painting `ARCHITECTURE.md`, mapping decision 15). A layout with
    /// no line at all reports one line box of the paragraph's line height
    /// with the baseline at `0.8 ×` that height.
    #[must_use]
    pub fn metrics(&self) -> TextLayoutResult {
        let Some(first) = self.layout.get(0) else {
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
        let kept = self
            .max_lines
            .map_or(self.layout.len(), |max| max.min(self.layout.len()));
        let (width, height) = if kept < self.layout.len() {
            // The layout's own width and height cover every line; a
            // truncated paragraph measures only the lines it keeps, with
            // Parley's rule (trailing whitespace excluded from the width).
            self.layout
                .lines()
                .take(kept)
                .fold((0.0_f32, 0.0_f32), |(width, height), line| {
                    let metrics = line.metrics();
                    let extent =
                        metrics.inline_min_coord + metrics.advance - metrics.trailing_whitespace;
                    (width.max(extent), height + metrics.line_height)
                })
        } else {
            (self.layout.width(), self.layout.height())
        };
        TextLayoutResult {
            width: f64::from(width),
            height: f64::from(height),
            line_count: kept.max(1),
            max_line_width: f64::from(width),
            alphabetic_baseline: f64::from(line.baseline),
            ideographic_baseline: f64::from(line.block_max_coord),
            truncated: kept < self.layout.len(),
        }
    }

    /// The paragraph's narrowest and widest widths: `(min, max)`, where min
    /// takes every soft break (the widest unbreakable run) and max takes
    /// none (the single-line width). Independent of the width it was broken
    /// at and of `max_lines`.
    #[must_use]
    pub fn content_widths(&self) -> (f64, f64) {
        if self.text.is_empty() {
            return (0.0, 0.0);
        }
        let widths = self.layout.calculate_content_widths();
        (f64::from(widths.min), f64::from(widths.max))
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

        // Families are resolved against the collection before the builder
        // borrows it: the same rule the process font system resolves with.
        let collection = &mut self.font_cx.collection;
        let default_family = family(collection, None);
        let default_properties = paragraph
            .default_style
            .map(|style| properties(collection, style))
            .unwrap_or_default();
        let mut start = 0;
        let span_properties: Vec<_> = paragraph
            .spans
            .iter()
            .map(|(span, style)| {
                let range = start..start + span.len();
                start = range.end;
                let properties = style
                    .as_ref()
                    .map(|style| properties(collection, style))
                    .unwrap_or_default();
                (range, properties)
            })
            .collect();

        // Unquantized: the layout is in logical pixels, and Parley's
        // quantization would round ascent, descent and the leading halves to
        // whole logical pixels, which at a device scale other than 1 is not
        // the device grid. Metrics stay exact, as on the cosmic-text path, and
        // a baseline reaches the device grid once, when it is painted
        // (`(line_y * scale).round()`); `tests/parley_metrics_oracle.rs` pins
        // the agreement.
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, &text, 1.0, false);
        builder.push_default(default_family);
        builder.push_default(StyleProperty::FontSize(paragraph.font_size));
        // A word wider than the line breaks between its glyphs, as the
        // painted layout's `Wrap::WordOrGlyph` does, instead of overflowing
        // the line. `BreakWord` rather than `Anywhere`: the min-content width
        // stays the widest word (flui-painting `ARCHITECTURE.md`, mapping
        // decision 15).
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::BreakWord));
        // No explicit height is 1.2 em of each run's own size, so a larger
        // span grows its line box, as on the cosmic-text path; an explicit
        // height is one absolute line box for the paragraph.
        builder.push_default(StyleProperty::LineHeight(match paragraph.line_height {
            Some(height) => LineHeight::Absolute(height),
            None => LineHeight::FontSizeRelative(1.2),
        }));
        for property in default_properties {
            builder.push_default(property);
        }
        for (range, properties) in span_properties {
            for property in properties {
                builder.push(property, range.clone());
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
            max_lines: paragraph.max_lines.filter(|&lines| lines > 0),
        }
    }
}

/// The one family a style is shaped with: FLUI's family rule
/// (`resolve_family_name`) over the families `collection` holds, the rule
/// the process font system resolves with over its own database. Nothing
/// follows it in the list: past that family, Parley walks the collection's
/// fallback families, which mirror the process font system's fallback order
/// in a collection fed from the host (`FontCollection::with_host_faces`).
fn family(
    collection: &mut Collection,
    style: Option<&TextStyle>,
) -> StyleProperty<'static, SpanBrush> {
    let family = resolve_family_name(style, |name| holds_exactly(collection, name));
    let name = match family {
        Family::Name(name) => FontFamilyName::Named(Cow::Owned(name.to_owned())),
        Family::Serif => FontFamilyName::Generic(GenericFamily::Serif),
        Family::SansSerif => FontFamilyName::Generic(GenericFamily::SansSerif),
        Family::Cursive => FontFamilyName::Generic(GenericFamily::Cursive),
        Family::Fantasy => FontFamilyName::Generic(GenericFamily::Fantasy),
        Family::Monospace => FontFamilyName::Generic(GenericFamily::Monospace),
    };
    StyleProperty::FontFamily(FontFamily::Single(name))
}

/// Whether `collection` holds a family spelled exactly `name`.
///
/// fontique looks family names up without regard to case, fontdb (and so the
/// paint side's `InstalledFamilies`) exactly; the rule is asked the paint
/// side's question, so a style naming `"segoe ui"` degrades to the
/// sans-serif generic on both sides instead of measuring in Segoe UI and
/// painting in the generic's family.
pub(crate) fn holds_exactly(collection: &mut Collection, name: &str) -> bool {
    collection
        .family_id(name)
        .and_then(|id| collection.family_name(id))
        .is_some_and(|held| held == name)
}

/// The Parley properties `style` sets; a field left unset adds nothing.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64 style values narrow to Parley's f32 layout space"
)]
fn properties(
    collection: &mut Collection,
    style: &TextStyle,
) -> Vec<StyleProperty<'static, SpanBrush>> {
    let mut properties = Vec::new();
    if style.font_family.is_some() {
        properties.push(family(collection, Some(style)));
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
            max_lines: None,
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

    /// On a collection holding only the bundled faces, a glyph the style's
    /// family lacks measures in Roboto, the face cosmic-text's last resort
    /// paints it with, not as `.notdef`: Cyrillic in Material Icons measures
    /// exactly as Cyrillic in Roboto.
    #[test]
    fn a_glyph_the_named_family_lacks_measures_in_roboto_on_the_bundled_collection() {
        let fonts = FontCollection::new();
        let width = |family: &str| {
            let style = TextStyle {
                font_family: Some(family.to_owned()),
                ..TextStyle::default()
            };
            let spans: Vec<(String, Option<TextStyle>)> = vec![("Привет".to_owned(), Some(style))];
            TextContext::new(&fonts)
                .shape(&ParagraphSpec {
                    spans: &spans,
                    default_style: None,
                    font_size: 16.0,
                    max_width: None,
                    line_height: None,
                    direction: TextDirection::Ltr,
                    max_lines: None,
                })
                .metrics()
                .width
        };
        let roboto = width("Roboto");
        let icons = width("Material Icons");
        assert!(
            (icons - roboto).abs() < 1e-3,
            "Cyrillic in Material Icons measures {icons}, in Roboto {roboto}"
        );
    }

    fn wrapped(max_lines: Option<usize>) -> ParagraphLayout {
        let spans: Vec<(String, Option<TextStyle>)> = vec![(
            "one two three four five six seven eight nine ten".to_owned(),
            None,
        )];
        TextContext::new(&FontCollection::new()).shape(&ParagraphSpec {
            spans: &spans,
            default_style: None,
            font_size: 16.0,
            max_width: Some(60.0),
            line_height: Some(20.0),
            direction: TextDirection::Ltr,
            max_lines,
        })
    }

    /// `max_lines` stops the metrics at the kept lines: the height covers
    /// only them and `truncated` says lines were dropped. Without it the
    /// same paragraph reports every line.
    #[test]
    fn max_lines_truncates_parley_metrics() {
        let full = wrapped(None).metrics();
        assert!(
            full.line_count > 2,
            "the probe must wrap past two lines, got {}",
            full.line_count
        );
        assert!(!full.truncated);

        let kept = wrapped(Some(2)).metrics();
        assert_eq!(kept.line_count, 2);
        assert!(kept.truncated, "dropping lines reports truncation");
        assert!(
            (kept.height - 40.0).abs() < 1e-3,
            "two 20 px lines, got {}",
            kept.height
        );
        assert!(kept.height < full.height);
        assert!(kept.width <= 60.0 + 1e-3);

        let exact = wrapped(Some(full.line_count)).metrics();
        assert!(!exact.truncated, "keeping every line is no truncation");
        assert!((exact.height - full.height).abs() < 1e-3);
    }

    /// The content widths are the widest word and the single-line width,
    /// whatever width the paragraph was broken at.
    #[test]
    fn content_widths_bound_every_break_width() {
        let paragraph = wrapped(None);
        let (min, max) = paragraph.content_widths();
        assert!(min > 0.0 && min < max, "min {min}, max {max}");
        let spans: Vec<(String, Option<TextStyle>)> = vec![("seven".to_owned(), None)];
        let seven = TextContext::new(&FontCollection::new())
            .shape(&ParagraphSpec {
                spans: &spans,
                default_style: None,
                font_size: 16.0,
                max_width: None,
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
            })
            .metrics()
            .width;
        assert!(
            min >= seven - 1e-3,
            "min {min} covers the word 'seven' ({seven})"
        );
    }
}

//! Text measurement helpers: `measure_text`, `measure_inline_span`,
//! `style_to_attrs`.
//!
//! Mythos chain U6 extracted these from the 1,243-LOC
//! `text_layout.rs` god module. Measurement is shape-then-compute on
//! a transient cosmic-text `Buffer`; reuses the global `FONT_SYSTEM`
//! singleton from [`super::layout`].

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};
use flui_types::typography::{FontStyle, FontWeight, TextStyle};

use super::TextLayoutResult;
use super::layout::font_system;

/// Converts FLUI `TextStyle` to cosmic-text `Attrs`.
pub(super) fn style_to_attrs(style: Option<&TextStyle>) -> Attrs<'static> {
    let mut attrs = Attrs::new();

    if let Some(style) = style {
        if let Some(ref family) = style.font_family {
            attrs = attrs.family(match family.as_str() {
                "serif" | "Serif" => Family::Serif,
                "sans-serif" | "SansSerif" | "sans" => Family::SansSerif,
                "monospace" | "Monospace" | "mono" => Family::Monospace,
                "cursive" | "Cursive" => Family::Cursive,
                "fantasy" | "Fantasy" => Family::Fantasy,
                _ => Family::SansSerif,
            });
        }

        if let Some(weight) = style.font_weight {
            let cosmic_weight = match weight {
                FontWeight::W100 => Weight::THIN,
                FontWeight::W200 => Weight::EXTRA_LIGHT,
                FontWeight::W300 => Weight::LIGHT,
                FontWeight::W400 => Weight::NORMAL,
                FontWeight::W500 => Weight::MEDIUM,
                FontWeight::W600 => Weight::SEMIBOLD,
                FontWeight::W700 => Weight::BOLD,
                FontWeight::W800 => Weight::EXTRA_BOLD,
                FontWeight::W900 => Weight::BLACK,
            };
            attrs = attrs.weight(cosmic_weight);
        }

        if let Some(font_style) = style.font_style {
            let cosmic_style = match font_style {
                FontStyle::Normal => Style::Normal,
                FontStyle::Italic => Style::Italic,
            };
            attrs = attrs.style(cosmic_style);
        }
    }

    attrs
}

/// Measures text and returns layout metrics.
pub fn measure_text(
    text: &str,
    style: Option<&TextStyle>,
    font_size: f32,
    max_width: Option<f32>,
    line_height: Option<f32>,
) -> TextLayoutResult {
    debug_assert!(
        font_size > 0.0 && font_size.is_finite(),
        "measure_text font_size must be positive and finite, got {font_size}"
    );

    let mut font_system = font_system().lock();

    let line_height = line_height.unwrap_or(font_size * 1.2);
    let metrics = Metrics::new(font_size, line_height);

    let mut buffer = Buffer::new(&mut font_system, metrics);
    buffer.set_size(&mut font_system, max_width, None);

    let attrs = style_to_attrs(style);
    buffer.set_text(&mut font_system, text, attrs, Shaping::Advanced);
    buffer.shape_until_scroll(&mut font_system, false);

    let mut total_height = 0.0f32;
    let mut max_line_width = 0.0f32;
    let mut line_count = 0usize;
    let mut first_baseline = 0.0f32;

    for run in buffer.layout_runs() {
        line_count += 1;
        max_line_width = max_line_width.max(run.line_w);
        total_height = total_height.max(run.line_top + run.line_height);

        if line_count == 1 {
            first_baseline = run.line_top + run.line_height * 0.8;
        }
    }

    if line_count == 0 {
        line_count = 1;
        total_height = line_height;
        first_baseline = line_height * 0.8;
    }

    TextLayoutResult {
        width: max_line_width,
        height: total_height,
        line_count,
        max_line_width,
        alphabetic_baseline: first_baseline,
    }
}

/// Measures text with rich spans (`InlineSpan`).
///
/// Extracts plain text and measures it; per-span styling support is
/// future work.
pub fn measure_inline_span(
    span: &flui_types::typography::InlineSpan,
    font_size: f32,
    max_width: Option<f32>,
    scale_factor: f32,
) -> TextLayoutResult {
    let plain_text = span.to_plain_text();
    let style = span.style();
    let scaled_font_size = font_size * scale_factor;

    measure_text(&plain_text, style, scaled_font_size, max_width, None)
}

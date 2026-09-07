//! Text measurement helpers: `measure_text`, `measure_inline_span`,
//! `style_to_attrs`.
//!
//! Extracted from the 1,243-LOC
//! `text_layout.rs` god module. Measurement is shape-then-compute on
//! a transient cosmic-text `Buffer`; reuses the global `FONT_SYSTEM`
//! singleton from [`super::layout`].

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};
use flui_types::typography::{FontStyle, FontWeight, TextStyle};

use super::TextLayoutResult;
use super::layout::font_system;

/// Converts FLUI `TextStyle` to cosmic-text `Attrs`.
///
/// `family` comes from the crate's font resolution rather than from
/// `style.font_family` directly: a family the host does not
/// carry must not reach the shaper, or the run falls into cosmic-text's
/// unfiltered, emoji-first fallback tail. The caller resolves it because
/// resolution needs the font database, and the caller is what holds the lock.
///
/// The returned `Attrs` borrows the style's family string for
/// `Family::Name`, hence the shared lifetime.
pub(super) fn style_to_attrs<'a>(style: Option<&'a TextStyle>, family: Family<'a>) -> Attrs<'a> {
    let mut attrs = Attrs::new().family(family);

    if let Some(style) = style {
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

    let line_height = line_height.unwrap_or(font_size * 1.2);

    // cosmic-text 0.19: `Buffer::new_empty` skips the empty-string shape pass
    // `Buffer::new` performs, and `set_size` is lazy, so the expensive part of
    // describing the buffer stays outside the global `FONT_SYSTEM` lock. The
    // lock brackets family resolution (which reads the font database), the
    // lazy `set_text`, and the shape pass — resolution has to be inside it
    // because the two must agree on one database.
    let mut buffer = Buffer::new_empty(Metrics::new(font_size, line_height));
    buffer.set_size(max_width, None);

    {
        let mut state = font_system().lock();
        let family = state.resolve_family(style);
        let attrs = style_to_attrs(style, family);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut state.system, false);
    }

    super::layout::metrics_from_shaped_buffer(&buffer, line_height, false)
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

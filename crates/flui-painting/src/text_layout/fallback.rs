//! Fallback text layout when the `text` feature is disabled.
//!
//! Mythos chain U6 extracted these from the 1,243-LOC
//! `text_layout.rs` god module. Provides estimated metrics based on
//! character count rather than real shaping; lets dependent crates
//! compile without cosmic-text.

use flui_types::{
    geometry::{Offset, Pixels, Rect},
    typography::{
        LineMetrics, TextAffinity, TextBox, TextDirection, TextPosition, TextRange, TextStyle,
    },
};

use super::TextLayoutResult;

/// Stub text layout for use by `TextPainter` when the `text` feature
/// is disabled.
///
/// Provides estimated metrics based on character count rather than
/// real shaping.
#[derive(Debug)]
pub struct TextLayout {
    /// Font size used for layout.
    font_size: f32,
    /// Line height used for layout.
    line_height: f32,
    /// Text direction.
    direction: TextDirection,
    /// The plain text stored for hit testing / metrics.
    text: String,
    /// Estimated width.
    width: f32,
    /// Number of estimated lines.
    line_count: usize,
}

impl TextLayout {
    /// Creates a new stub text layout with estimated metrics.
    pub fn new(
        text: &str,
        _style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
    ) -> Self {
        debug_assert!(
            font_size > 0.0 && font_size.is_finite(),
            "TextLayout::new font_size must be positive and finite, got {font_size}"
        );

        let line_height = line_height.unwrap_or(font_size * 1.2);
        let char_count = text.chars().count();
        let estimated_width = char_count as f32 * font_size * 0.5;

        let (width, line_count) = if let Some(max_w) = max_width {
            if estimated_width > max_w {
                let lines = (estimated_width / max_w).ceil() as usize;
                (max_w.min(estimated_width), lines.max(1))
            } else {
                (estimated_width, 1)
            }
        } else {
            (estimated_width, 1)
        };

        Self {
            font_size,
            line_height,
            direction,
            text: text.to_string(),
            width,
            line_count,
        }
    }

    /// Returns estimated metrics.
    pub fn metrics(&self) -> TextLayoutResult {
        TextLayoutResult {
            width: self.width,
            height: self.line_count as f32 * self.line_height,
            line_count: self.line_count,
            max_line_width: self.width,
            alphabetic_baseline: self.line_height * 0.8,
        }
    }

    /// Returns the screen offset for a caret at the given text
    /// position (estimated).
    pub fn get_offset_for_caret(&mut self, position: TextPosition) -> Offset<Pixels> {
        let avg_char_width = self.font_size * 0.5;
        let x = position.offset as f32 * avg_char_width;
        Offset::new(Pixels(x), Pixels(0.0))
    }

    /// Returns the text position for a screen offset (estimated).
    pub fn get_position_for_offset(&self, offset: Offset<Pixels>) -> TextPosition {
        let avg_char_width = self.font_size * 0.5;
        let char_index = if avg_char_width > 0.0 {
            (offset.dx.0 / avg_char_width).round() as usize
        } else {
            0
        };
        let max_offset = self.text.len();
        TextPosition::new(char_index.min(max_offset), TextAffinity::Downstream)
    }

    /// Returns line metrics for all lines in the layout (estimated).
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        let mut metrics = Vec::with_capacity(self.line_count.max(1));
        let ascent = self.font_size * 0.8;
        let descent = self.font_size * 0.2;

        for i in 0..self.line_count.max(1) {
            metrics.push(LineMetrics::new(
                true,
                ascent as f64,
                descent as f64,
                ascent as f64,
                self.line_height as f64,
                self.width as f64,
                0.0,
                (i as f32 * self.line_height + ascent) as f64,
                i,
                0,
                0,
                0,
                0,
            ));
        }

        if metrics.is_empty() {
            metrics.push(LineMetrics::new(
                true,
                ascent as f64,
                descent as f64,
                ascent as f64,
                self.line_height as f64,
                0.0,
                0.0,
                ascent as f64,
                0,
                0,
                0,
                0,
                0,
            ));
        }

        metrics
    }

    /// Returns bounding boxes for the given text range (estimated).
    #[allow(clippy::needless_pass_by_value)]
    pub fn get_boxes_for_range(&self, range: TextRange) -> Vec<TextBox> {
        let avg_char_width = self.font_size * 0.5;
        let start_x = range.start as f32 * avg_char_width;
        let end_x = range.end as f32 * avg_char_width;

        let rect = Rect::from_ltrb(
            Pixels(start_x),
            Pixels(0.0),
            Pixels(end_x),
            Pixels(self.line_height),
        );

        vec![TextBox::new(rect, self.direction)]
    }

    /// Returns the word boundary at the given text position
    /// (estimated).
    pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
        let bytes = self.text.as_bytes();
        let offset = position.offset.min(bytes.len());

        let mut start = offset;
        while start > 0 && bytes.get(start - 1).is_some_and(|&b| b != b' ') {
            start -= 1;
        }

        let mut end = offset;
        while end < bytes.len() && bytes.get(end).is_some_and(|&b| b != b' ') {
            end += 1;
        }

        TextRange::new(start, end)
    }
}

/// Stub direction detection (always returns LTR).
pub fn detect_text_direction(_text: &str) -> Option<TextDirection> {
    Some(TextDirection::Ltr)
}

/// Stub measurement (estimates based on character count).
pub fn measure_text(
    text: &str,
    _style: Option<&TextStyle>,
    font_size: f32,
    max_width: Option<f32>,
    _line_height: Option<f32>,
) -> TextLayoutResult {
    debug_assert!(
        font_size > 0.0 && font_size.is_finite(),
        "measure_text font_size must be positive and finite, got {font_size}"
    );

    let char_count = text.chars().count();
    let estimated_width = char_count as f32 * font_size * 0.5;
    let line_height = font_size * 1.2;

    let (width, line_count) = if let Some(max_w) = max_width {
        if estimated_width > max_w {
            let lines = (estimated_width / max_w).ceil() as usize;
            (max_w.min(estimated_width), lines.max(1))
        } else {
            (estimated_width, 1)
        }
    } else {
        (estimated_width, 1)
    };

    TextLayoutResult {
        width,
        height: line_count as f32 * line_height,
        line_count,
        max_line_width: width,
        alphabetic_baseline: line_height * 0.8,
    }
}

/// Stub measurement for `InlineSpan`.
pub fn measure_inline_span(
    span: &flui_types::typography::InlineSpan,
    font_size: f32,
    max_width: Option<f32>,
    scale_factor: f32,
) -> TextLayoutResult {
    let plain_text = span.to_plain_text();
    let scaled_font_size = font_size * scale_factor;
    measure_text(&plain_text, span.style(), scaled_font_size, max_width, None)
}

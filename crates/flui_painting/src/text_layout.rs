//! Text layout engine using cosmic-text.
//!
//! This module provides text shaping, measurement, and layout capabilities
//! using the cosmic-text library.

#[cfg(feature = "text")]
mod inner {
    use cosmic_text::{Attrs, Buffer, Cursor, Family, FontSystem, Metrics, Shaping, Style, Weight};
    use flui_types::geometry::{Offset, Rect, Size};
    use flui_types::typography::{
        FontStyle, FontWeight, LineMetrics, TextAffinity, TextBox, TextDirection, TextPosition,
        TextRange, TextStyle,
    };
    use parking_lot::Mutex;
    use std::sync::OnceLock;

    /// Global font system instance.
    ///
    /// cosmic-text requires a FontSystem for font discovery and shaping.
    /// We use a global instance with interior mutability for convenience.
    static FONT_SYSTEM: OnceLock<Mutex<FontSystem>> = OnceLock::new();

    /// Gets or initializes the global font system.
    fn font_system() -> &'static Mutex<FontSystem> {
        FONT_SYSTEM.get_or_init(|| {
            tracing::debug!("Initializing global FontSystem");
            Mutex::new(FontSystem::new())
        })
    }

    /// Text layout result containing computed metrics.
    #[derive(Debug, Clone)]
    pub struct TextLayoutResult {
        /// Total width of the laid out text.
        pub width: f32,
        /// Total height of the laid out text.
        pub height: f32,
        /// Number of lines after layout.
        pub line_count: usize,
        /// Width of the longest line.
        pub max_line_width: f32,
        /// Distance to alphabetic baseline from top.
        pub alphabetic_baseline: f32,
    }

    impl TextLayoutResult {
        /// Returns the size as a Size struct.
        #[inline]
        #[must_use]
        pub fn size(&self) -> Size {
            Size::new(self.width, self.height)
        }
    }

    /// A laid out text buffer with cursor and hit testing support.
    ///
    /// This wraps a cosmic-text Buffer and provides methods for:
    /// - Cursor positioning (offset → screen position)
    /// - Hit testing (screen position → text offset)
    /// - Line metrics
    /// - Selection boxes
    pub struct TextLayout {
        /// The underlying cosmic-text buffer.
        buffer: Buffer,
        /// Font size used for layout.
        font_size: f32,
        /// Line height used for layout.
        line_height: f32,
        /// Text direction.
        direction: TextDirection,
    }

    impl TextLayout {
        /// Creates a new text layout.
        pub fn new(
            text: &str,
            style: Option<&TextStyle>,
            font_size: f32,
            max_width: Option<f32>,
            line_height: Option<f32>,
            direction: TextDirection,
        ) -> Self {
            let mut font_system = font_system().lock();

            let line_height = line_height.unwrap_or(font_size * 1.2);
            let metrics = Metrics::new(font_size, line_height);

            let mut buffer = Buffer::new(&mut font_system, metrics);
            buffer.set_size(&mut font_system, max_width, None);

            let attrs = style_to_attrs(style);
            buffer.set_text(&mut font_system, text, attrs, Shaping::Advanced);
            buffer.shape_until_scroll(&mut font_system, false);

            Self {
                buffer,
                font_size,
                line_height,
                direction,
            }
        }

        /// Returns the computed metrics for this layout.
        pub fn metrics(&self) -> TextLayoutResult {
            let mut total_height = 0.0f32;
            let mut max_line_width = 0.0f32;
            let mut line_count = 0usize;
            let mut first_baseline = 0.0f32;

            for run in self.buffer.layout_runs() {
                line_count += 1;
                max_line_width = max_line_width.max(run.line_w);
                total_height = total_height.max(run.line_top + run.line_height);

                if line_count == 1 {
                    first_baseline = run.line_top + run.line_height * 0.8;
                }
            }

            if line_count == 0 {
                line_count = 1;
                total_height = self.line_height;
                first_baseline = self.line_height * 0.8;
            }

            TextLayoutResult {
                width: max_line_width,
                height: total_height,
                line_count,
                max_line_width,
                alphabetic_baseline: first_baseline,
            }
        }

        /// Returns the screen offset for a caret at the given text position.
        ///
        /// This is used for drawing the text cursor.
        pub fn get_offset_for_caret(&mut self, position: TextPosition) -> Offset {
            let mut font_system = font_system().lock();

            // Create cursor at position
            let cursor = Cursor::new(0, position.offset);

            // Get cursor position from buffer
            if let Some(layout_cursor) = self.buffer.layout_cursor(&mut font_system, cursor) {
                // layout_cursor gives us glyph index, we need to find actual x position
                let mut x = 0.0f32;
                let mut y = 0.0f32;

                for run in self.buffer.layout_runs() {
                    if run.line_i == layout_cursor.line {
                        y = run.line_top;

                        // Find x position by iterating glyphs
                        for glyph in run.glyphs.iter() {
                            if glyph.start <= position.offset && position.offset <= glyph.end {
                                // Interpolate within glyph if needed
                                let glyph_progress = if glyph.end > glyph.start {
                                    (position.offset - glyph.start) as f32
                                        / (glyph.end - glyph.start) as f32
                                } else {
                                    0.0
                                };
                                x = glyph.x + glyph.w * glyph_progress;
                                break;
                            }
                            x = glyph.x + glyph.w;
                        }
                        break;
                    }
                }

                Offset::new(x, y)
            } else {
                // Fallback for empty buffer or invalid position
                Offset::ZERO
            }
        }

        /// Returns the text position for a screen offset.
        ///
        /// This is used for hit testing (e.g., mouse clicks).
        pub fn get_position_for_offset(&self, offset: Offset) -> TextPosition {
            let x = offset.dx;
            let y = offset.dy;

            // Find the line at y
            let mut target_line: Option<usize> = None;
            let mut line_top = 0.0f32;

            for run in self.buffer.layout_runs() {
                if y >= run.line_top && y < run.line_top + run.line_height {
                    target_line = Some(run.line_i);
                    line_top = run.line_top;
                    break;
                }
            }

            // If no line found, use last line or return 0
            let target_line = match target_line {
                Some(l) => l,
                None => {
                    // Check if below all lines
                    let mut last_line = 0;
                    for run in self.buffer.layout_runs() {
                        last_line = run.line_i;
                    }
                    if y >= line_top {
                        last_line
                    } else {
                        return TextPosition::upstream(0);
                    }
                }
            };

            // Find glyph at x in target line
            for run in self.buffer.layout_runs() {
                if run.line_i == target_line {
                    let mut last_offset = run.glyphs.first().map(|g| g.start).unwrap_or(0);

                    for glyph in run.glyphs.iter() {
                        let glyph_center = glyph.x + glyph.w / 2.0;

                        if x < glyph_center {
                            // Cursor is before center of this glyph
                            return TextPosition::new(glyph.start, TextAffinity::Downstream);
                        }

                        last_offset = glyph.end;
                    }

                    // Past all glyphs
                    return TextPosition::new(last_offset, TextAffinity::Upstream);
                }
            }

            TextPosition::upstream(0)
        }

        /// Returns line metrics for all lines in the layout.
        pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
            let mut metrics = Vec::new();
            let mut line_number = 0;

            for run in self.buffer.layout_runs() {
                let ascent = self.font_size * 0.8;
                let descent = self.font_size * 0.2;

                // Calculate text indices for this line
                let start_index = run.glyphs.first().map(|g| g.start).unwrap_or(0);
                let end_index = run.glyphs.last().map(|g| g.end).unwrap_or(start_index);

                metrics.push(LineMetrics::new(
                    true, // hard_break - assume true for now
                    ascent as f64,
                    descent as f64,
                    ascent as f64,                       // unscaled_ascent
                    run.line_height as f64,              // height
                    run.line_w as f64,                   // width
                    0.0,                                 // left
                    run.line_top as f64 + ascent as f64, // baseline
                    line_number,
                    start_index,
                    end_index,
                    end_index, // end_excluding_whitespace (simplified)
                    end_index, // end_including_newline (simplified)
                ));

                line_number += 1;
            }

            // Ensure at least one line
            if metrics.is_empty() {
                metrics.push(LineMetrics::new(
                    true,
                    (self.font_size * 0.8) as f64,
                    (self.font_size * 0.2) as f64,
                    (self.font_size * 0.8) as f64,
                    self.line_height as f64,
                    0.0,
                    0.0,
                    (self.font_size * 0.8) as f64,
                    0,
                    0,
                    0,
                    0,
                    0,
                ));
            }

            metrics
        }

        /// Returns bounding boxes for the given text range.
        ///
        /// Used for rendering text selection highlights.
        pub fn get_boxes_for_range(&self, range: TextRange) -> Vec<TextBox> {
            let mut boxes = Vec::new();

            for run in self.buffer.layout_runs() {
                let mut line_start_x: Option<f32> = None;
                let mut line_end_x = 0.0f32;

                for glyph in run.glyphs.iter() {
                    // Check if glyph overlaps with range
                    if glyph.end > range.start && glyph.start < range.end {
                        if line_start_x.is_none() {
                            // Adjust start x if range starts mid-glyph
                            let start_offset = if glyph.start < range.start {
                                let progress = (range.start - glyph.start) as f32
                                    / (glyph.end - glyph.start) as f32;
                                glyph.x + glyph.w * progress
                            } else {
                                glyph.x
                            };
                            line_start_x = Some(start_offset);
                        }

                        // Adjust end x if range ends mid-glyph
                        line_end_x = if glyph.end > range.end {
                            let progress =
                                (range.end - glyph.start) as f32 / (glyph.end - glyph.start) as f32;
                            glyph.x + glyph.w * progress
                        } else {
                            glyph.x + glyph.w
                        };
                    }
                }

                if let Some(start_x) = line_start_x {
                    let rect = Rect::from_ltrb(
                        start_x,
                        run.line_top,
                        line_end_x,
                        run.line_top + run.line_height,
                    );
                    boxes.push(TextBox::new(rect, self.direction));
                }
            }

            boxes
        }

        /// Returns the word boundary at the given text position.
        ///
        /// Used for double-click word selection.
        pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
            // Get the full text by reconstructing from glyphs
            // For now, use a simple word boundary algorithm
            let mut char_positions: Vec<usize> = Vec::new();

            for run in self.buffer.layout_runs() {
                for glyph in run.glyphs.iter() {
                    // Each glyph represents a character range
                    // This is simplified - real implementation would need actual text
                    char_positions.push(glyph.start);
                }
            }

            // Simple word boundary: find whitespace boundaries
            let offset = position.offset;

            // Find start of word (scan backwards for whitespace or start)
            let mut word_start = offset;
            for i in (0..offset).rev() {
                if char_positions.contains(&i) {
                    // Check if this is a word boundary
                    // Simplified: assume no whitespace info available
                    word_start = i;
                } else {
                    break;
                }
            }

            // Find end of word (scan forwards for whitespace or end)
            let mut word_end = offset;
            let max_pos = char_positions.last().copied().unwrap_or(0);
            for i in offset..=max_pos {
                if char_positions.contains(&i) {
                    word_end = i + 1;
                } else {
                    break;
                }
            }

            TextRange::new(word_start, word_end)
        }
    }

    impl std::fmt::Debug for TextLayout {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TextLayout")
                .field("font_size", &self.font_size)
                .field("line_height", &self.line_height)
                .field("direction", &self.direction)
                .finish_non_exhaustive()
        }
    }

    /// Converts FLUI TextStyle to cosmic-text Attrs.
    fn style_to_attrs(style: Option<&TextStyle>) -> Attrs<'static> {
        let mut attrs = Attrs::new();

        if let Some(style) = style {
            // Font family
            if let Some(ref family) = style.font_family {
                // For now, use generic families based on name
                attrs = attrs.family(match family.as_str() {
                    "serif" | "Serif" => Family::Serif,
                    "sans-serif" | "SansSerif" | "sans" => Family::SansSerif,
                    "monospace" | "Monospace" | "mono" => Family::Monospace,
                    "cursive" | "Cursive" => Family::Cursive,
                    "fantasy" | "Fantasy" => Family::Fantasy,
                    _ => Family::SansSerif, // Default
                });
            }

            // Font weight
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

            // Font style (italic)
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
    ///
    /// # Arguments
    ///
    /// * `text` - The text to measure
    /// * `style` - Optional text style
    /// * `font_size` - Font size in pixels
    /// * `max_width` - Maximum width constraint (None for unlimited)
    /// * `line_height` - Line height in pixels (if None, uses font_size * 1.2)
    ///
    /// # Returns
    ///
    /// Layout result with computed metrics.
    pub fn measure_text(
        text: &str,
        style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
    ) -> TextLayoutResult {
        let mut font_system = font_system().lock();

        // Create metrics
        let line_height = line_height.unwrap_or(font_size * 1.2);
        let metrics = Metrics::new(font_size, line_height);

        // Create buffer
        let mut buffer = Buffer::new(&mut font_system, metrics);

        // Set size constraint
        buffer.set_size(&mut font_system, max_width, None);

        // Set text with attributes
        let attrs = style_to_attrs(style);
        buffer.set_text(&mut font_system, text, attrs, Shaping::Advanced);

        // Shape the text
        buffer.shape_until_scroll(&mut font_system, false);

        // Compute metrics from layout runs
        let mut total_height = 0.0f32;
        let mut max_line_width = 0.0f32;
        let mut line_count = 0usize;
        let mut first_baseline = 0.0f32;

        for run in buffer.layout_runs() {
            line_count += 1;
            max_line_width = max_line_width.max(run.line_w);
            total_height = total_height.max(run.line_top + run.line_height);

            // First line baseline
            if line_count == 1 {
                // Approximate alphabetic baseline as ~80% of line height from top
                first_baseline = run.line_top + run.line_height * 0.8;
            }
        }

        // Handle empty text
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

    /// Measures text with rich spans (InlineSpan).
    ///
    /// For now, this extracts plain text and measures it.
    /// In the future, we can support per-span styling.
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_measure_simple_text() {
            let result = measure_text("Hello, World!", None, 14.0, None, None);

            assert!(result.width > 0.0);
            assert!(result.height > 0.0);
            assert_eq!(result.line_count, 1);
        }

        #[test]
        fn test_measure_multiline_text() {
            let result = measure_text("Line 1\nLine 2\nLine 3", None, 14.0, None, None);

            assert!(result.height > 0.0);
            assert_eq!(result.line_count, 3);
        }

        #[test]
        fn test_measure_with_width_constraint() {
            // Long text that should wrap
            let text = "This is a very long line of text that should wrap when constrained";

            // Without constraint
            let unconstrained = measure_text(text, None, 14.0, None, None);

            // With narrow constraint
            let constrained = measure_text(text, None, 14.0, Some(100.0), None);

            // Constrained should have more lines
            assert!(constrained.line_count >= unconstrained.line_count);
            // Constrained width should be less
            assert!(constrained.max_line_width <= 100.0 + 1.0); // Allow small overflow
        }

        #[test]
        fn test_measure_empty_text() {
            let result = measure_text("", None, 14.0, None, None);

            assert_eq!(result.line_count, 1);
            assert!(result.height > 0.0); // Should still have height
        }

        #[test]
        fn test_text_layout_creation() {
            let layout =
                TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

            let metrics = layout.metrics();
            assert!(metrics.width > 0.0);
            assert!(metrics.height > 0.0);
            assert_eq!(metrics.line_count, 1);
        }

        #[test]
        fn test_text_layout_caret_position() {
            let mut layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

            // Position at start
            let start_offset = layout.get_offset_for_caret(TextPosition::upstream(0));
            assert!(start_offset.dx >= 0.0);

            // Position in middle
            let mid_offset = layout.get_offset_for_caret(TextPosition::upstream(2));
            assert!(mid_offset.dx > start_offset.dx);

            // Position at end
            let end_offset = layout.get_offset_for_caret(TextPosition::upstream(5));
            assert!(end_offset.dx >= mid_offset.dx);
        }

        #[test]
        fn test_text_layout_hit_test() {
            let layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

            // Hit test at start
            let pos = layout.get_position_for_offset(Offset::new(0.0, 5.0));
            assert_eq!(pos.offset, 0);

            // Hit test past end should give last position
            let pos = layout.get_position_for_offset(Offset::new(1000.0, 5.0));
            assert!(pos.offset <= 5);
        }

        #[test]
        fn test_text_layout_line_metrics() {
            let layout =
                TextLayout::new("Line 1\nLine 2", None, 14.0, None, None, TextDirection::Ltr);

            let metrics = layout.get_line_metrics();
            assert_eq!(metrics.len(), 2);

            // First line
            assert_eq!(metrics[0].line_number, 0);
            assert!(metrics[0].width > 0.0);

            // Second line
            assert_eq!(metrics[1].line_number, 1);
        }

        #[test]
        fn test_text_layout_selection_boxes() {
            let layout =
                TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

            // Get boxes for "ello"
            let boxes = layout.get_boxes_for_range(TextRange::new(1, 5));
            assert!(!boxes.is_empty());

            // Box should have positive dimensions
            let first_box = &boxes[0];
            assert!(first_box.rect.width() > 0.0);
            assert!(first_box.rect.height() > 0.0);
        }

        #[test]
        fn test_text_layout_word_boundary() {
            let layout = TextLayout::new("Hello World", None, 14.0, None, None, TextDirection::Ltr);

            // Word boundary at position 2 (in "Hello")
            let boundary = layout.get_word_boundary(TextPosition::upstream(2));
            // Should return some range containing position 2
            assert!(boundary.start <= 2);
            assert!(boundary.end >= 2);
        }
    }
}

#[cfg(feature = "text")]
pub use inner::*;

/// Fallback implementation when text feature is disabled.
#[cfg(not(feature = "text"))]
mod fallback {
    use flui_types::geometry::Size;

    /// Text layout result (stub).
    #[derive(Debug, Clone)]
    pub struct TextLayoutResult {
        pub width: f32,
        pub height: f32,
        pub line_count: usize,
        pub max_line_width: f32,
        pub alphabetic_baseline: f32,
    }

    impl TextLayoutResult {
        #[inline]
        #[must_use]
        pub fn size(&self) -> Size {
            Size::new(self.width, self.height)
        }
    }

    /// Stub measurement (estimates based on character count).
    pub fn measure_text(
        text: &str,
        _style: Option<&flui_types::typography::TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        _line_height: Option<f32>,
    ) -> TextLayoutResult {
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

    /// Stub measurement for InlineSpan.
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
}

#[cfg(not(feature = "text"))]
pub use fallback::*;

//! Text layout engine using cosmic-text.
//!
//! This module provides text shaping, measurement, and layout capabilities
//! using the cosmic-text library.

#[cfg(feature = "text")]
mod inner {
    use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight};
    use flui_types::geometry::Size;
    use flui_types::typography::{FontStyle, FontWeight, TextStyle};
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

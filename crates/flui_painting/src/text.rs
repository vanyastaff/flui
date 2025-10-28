//! Text painting implementation
//!
//! Provides painting functionality for text rendering with alignment,
//! direction, overflow handling, and multi-line support.

use flui_types::{
    Rect, Point,
    styling::Color,
    typography::{TextAlign, TextDirection, TextOverflow, TextSpan, TextStyle},
};
use flui_engine::{Painter, Paint};

/// Text painter - handles text rendering with layout and styling
pub struct TextPainter;

impl TextPainter {
    /// Paint text with full styling support
    ///
    /// # Arguments
    ///
    /// * `painter` - The backend-agnostic painter
    /// * `rect` - The bounding rectangle for the text
    /// * `text` - The text string to paint
    /// * `font_size` - Font size in points
    /// * `color` - Text color
    /// * `text_align` - Horizontal alignment
    /// * `text_direction` - Text direction (LTR/RTL)
    /// * `max_lines` - Maximum number of lines (None = unlimited)
    /// * `overflow` - How to handle overflow
    /// * `soft_wrap` - Whether to wrap at word boundaries
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        text: &str,
        font_size: f32,
        color: Color,
        text_align: TextAlign,
        _text_direction: TextDirection, // TODO: implement RTL support
        _max_lines: Option<usize>,      // TODO: implement max lines
        _overflow: TextOverflow,         // TODO: implement overflow
        _soft_wrap: bool,                // TODO: implement soft wrap
    ) {
        // Convert color
        let paint_color = [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ];

        let paint = Paint {
            color: paint_color,
            stroke_width: 0.0,
            anti_alias: true,
        };

        // Calculate text position based on alignment
        // TODO: This is a simplified implementation - proper text layout
        // should measure text width and position accordingly
        let position = match text_align {
            TextAlign::Left | TextAlign::Start => {
                Point::new(rect.left(), rect.top())
            }
            TextAlign::Center => {
                // Approximate center (would need actual text measurement)
                Point::new(rect.left() + rect.width() / 2.0, rect.top())
            }
            TextAlign::Right | TextAlign::End => {
                Point::new(rect.right(), rect.top())
            }
            TextAlign::Justify => {
                Point::new(rect.left(), rect.top())
            }
        };

        // Use the painter's text method
        // NOTE: This is a simplified implementation. Complex text layout features
        // (wrapping, overflow ellipsis, multi-line, proper alignment) should be
        // implemented by the backend's text() method
        painter.text(text, position, font_size, &paint);
    }

    /// Paint text with shadow
    ///
    /// Simplified version that uses the painter's text_with_shadow method
    pub fn paint_with_shadow(
        painter: &mut dyn Painter,
        rect: Rect,
        text: &str,
        font_size: f32,
        color: Color,
        shadow_color: Color,
        shadow_offset: flui_types::Offset,
    ) {
        // Convert colors
        let text_color = [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ];

        let shadow_rgba = [
            shadow_color.red() as f32 / 255.0,
            shadow_color.green() as f32 / 255.0,
            shadow_color.blue() as f32 / 255.0,
            shadow_color.alpha() as f32 / 255.0,
        ];

        let paint = Paint {
            color: text_color,
            stroke_width: 0.0,
            anti_alias: true,
        };

        let position = Point::new(rect.left(), rect.top());

        // Use painter's built-in shadow support
        painter.text_with_shadow(text, position, font_size, &paint, shadow_offset, shadow_rgba);
    }

    /// Paint a TextSpan with rich text support (recursive)
    ///
    /// This traverses the TextSpan tree and paints each span with its own style.
    ///
    /// # Arguments
    ///
    /// * `painter` - The backend-agnostic painter
    /// * `rect` - The bounding rectangle for the text
    /// * `span` - The root TextSpan to paint
    /// * `base_style` - Default style for unstyled spans
    /// * `text_align` - Horizontal alignment
    ///
    /// # Returns
    ///
    /// Returns the final position after rendering all text
    pub fn paint_span(
        painter: &mut dyn Painter,
        rect: Rect,
        span: &TextSpan,
        base_style: &TextStyle,
        text_align: TextAlign,
    ) -> Point {
        let mut current_pos = match text_align {
            TextAlign::Left | TextAlign::Start => {
                Point::new(rect.left(), rect.top())
            }
            TextAlign::Center => {
                Point::new(rect.left() + rect.width() / 2.0, rect.top())
            }
            TextAlign::Right | TextAlign::End => {
                Point::new(rect.right(), rect.top())
            }
            TextAlign::Justify => {
                Point::new(rect.left(), rect.top())
            }
        };

        Self::paint_span_recursive(painter, span, base_style, &mut current_pos);
        current_pos
    }

    /// Recursively paint a TextSpan and its children
    fn paint_span_recursive(
        painter: &mut dyn Painter,
        span: &TextSpan,
        base_style: &TextStyle,
        current_pos: &mut Point,
    ) {
        // Determine the effective style for this span
        let effective_style = span.style.as_ref().unwrap_or(base_style);

        // Paint this span's text if it has any
        if let Some(text) = &span.text {
            if !text.is_empty() {
                Self::paint_span_text(painter, text, effective_style, current_pos);
            }
        }

        // Recursively paint children
        for child in &span.children {
            Self::paint_span_recursive(painter, child, effective_style, current_pos);
        }
    }

    /// Paint a single text segment with the given style
    fn paint_span_text(
        painter: &mut dyn Painter,
        text: &str,
        style: &TextStyle,
        current_pos: &mut Point,
    ) {
        // Extract color and font size from style
        let color = style.color.unwrap_or(Color::BLACK);
        let font_size = style.font_size.unwrap_or(14.0) as f32;

        let paint_color = [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ];

        let paint = Paint {
            color: paint_color,
            stroke_width: 0.0,
            anti_alias: true,
        };

        // Check if we have shadows
        if !style.shadows.is_empty() {
            // Paint with the first shadow (simplified)
            let shadow = &style.shadows[0];
            let shadow_color = [
                shadow.color.red() as f32 / 255.0,
                shadow.color.green() as f32 / 255.0,
                shadow.color.blue() as f32 / 255.0,
                shadow.color.alpha() as f32 / 255.0,
            ];

            let shadow_offset = flui_types::Offset {
                dx: shadow.offset_x as f32,
                dy: shadow.offset_y as f32,
            };

            painter.text_with_shadow(
                text,
                *current_pos,
                font_size,
                &paint,
                shadow_offset,
                shadow_color,
            );
        } else {
            painter.text(text, *current_pos, font_size, &paint);
        }

        // Advance position (simplified - actual implementation would measure text width)
        // For now, we just move right by an estimate
        // TODO: Proper text measurement
        current_pos.x += text.len() as f32 * font_size * 0.5;
    }

    /// Convert a TextSpan tree to plain text
    ///
    /// Useful for accessibility and text measurement.
    pub fn span_to_plain_text(span: &TextSpan) -> String {
        let mut result = String::new();

        if let Some(text) = &span.text {
            result.push_str(text);
        }

        for child in &span.children {
            result.push_str(&Self::span_to_plain_text(child));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_painter_exists() {
        // Basic smoke test to ensure TextPainter compiles
        let _painter = TextPainter;
    }

    #[test]
    fn test_text_align_variants() {
        // Test that all TextAlign variants are handled
        let aligns = vec![
            TextAlign::Left,
            TextAlign::Right,
            TextAlign::Center,
            TextAlign::Justify,
            TextAlign::Start,
            TextAlign::End,
        ];

        for _align in aligns {
            // Just checking that all variants compile
        }
    }

    #[test]
    fn test_text_overflow_variants() {
        // Test that TextOverflow variants exist
        let _clip = TextOverflow::Clip;
        let _ellipsis = TextOverflow::Ellipsis;
        let _fade = TextOverflow::Fade;
    }

    #[test]
    fn test_text_direction_variants() {
        // Test that TextDirection variants exist
        let _ltr = TextDirection::Ltr;
        let _rtl = TextDirection::Rtl;
    }

    #[test]
    fn test_color_conversion() {
        // Test color conversion logic
        let color = Color::rgba(255, 128, 64, 200);

        let converted = [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ];

        assert!((converted[0] - 1.0).abs() < 0.01);
        assert!((converted[1] - 0.502).abs() < 0.01);
        assert!((converted[2] - 0.251).abs() < 0.01);
        assert!((converted[3] - 0.784).abs() < 0.01);
    }

    #[test]
    fn test_span_to_plain_text_simple() {
        let span = TextSpan::new("Hello");
        let text = TextPainter::span_to_plain_text(&span);
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_span_to_plain_text_with_children() {
        let child1 = TextSpan::new("Hello");
        let child2 = TextSpan::new(" World");
        let parent = TextSpan::with_children(vec![child1, child2]);

        let text = TextPainter::span_to_plain_text(&parent);
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_span_to_plain_text_nested() {
        let inner = TextSpan::new("inner");
        let middle = TextSpan::with_children(vec![TextSpan::new("middle "), inner]);
        let outer = TextSpan::with_children(vec![TextSpan::new("outer "), middle]);

        let text = TextPainter::span_to_plain_text(&outer);
        assert_eq!(text, "outer middle inner");
    }

    #[test]
    fn test_textspan_with_style() {
        let style = TextStyle::default()
            .with_color(Color::RED)
            .with_font_size(20.0);
        let span = TextSpan::styled("Styled", style);

        assert_eq!(span.text, Some("Styled".to_string()));
        assert!(span.style.is_some());
        assert_eq!(span.style.as_ref().unwrap().color, Some(Color::RED));
    }
}

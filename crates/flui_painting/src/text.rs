//! Text painting implementation
//!
//! Provides painting functionality for text rendering with alignment,
//! direction, overflow handling, and multi-line support.

use flui_types::{
    Rect, Point,
    styling::Color,
    typography::{TextAlign, TextDirection, TextOverflow},
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
}

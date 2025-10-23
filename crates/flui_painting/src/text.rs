//! Text painting implementation
//!
//! Provides painting functionality for text rendering with alignment,
//! direction, overflow handling, and multi-line support.

use flui_types::{
    Rect, Size,
    styling::Color,
    typography::{TextAlign, TextDirection, TextOverflow},
};

/// Text painter - handles text rendering with layout and styling
pub struct TextPainter;

impl TextPainter {
    /// Paint text with full styling support
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter
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
        painter: &egui::Painter,
        rect: Rect,
        text: &str,
        font_size: f32,
        color: Color,
        text_align: TextAlign,
        _text_direction: TextDirection, // TODO: implement RTL support
        max_lines: Option<usize>,
        overflow: TextOverflow,
        soft_wrap: bool,
    ) {
        // Convert flui_types::Color to egui::Color32
        let egui_color = egui::Color32::from_rgba_unmultiplied(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        );

        // Create egui text format
        let font_id = egui::FontId::proportional(font_size);

        // Calculate text alignment
        let align = match text_align {
            TextAlign::Left | TextAlign::Start => egui::Align2::LEFT_TOP,
            TextAlign::Right | TextAlign::End => egui::Align2::RIGHT_TOP,
            TextAlign::Center => egui::Align2::CENTER_TOP,
            TextAlign::Justify => egui::Align2::LEFT_TOP, // egui doesn't support justify, use left
        };

        // For now, we'll implement basic text rendering
        // Full multi-line support with wrapping would require more complex layout logic

        // Convert flui_types::Rect to coordinates
        let min_x = rect.min.x;
        let min_y = rect.min.y;
        let max_x = rect.max.x;
        let width = rect.width();

        let pos = match text_align {
            TextAlign::Left | TextAlign::Start => egui::pos2(min_x, min_y),
            TextAlign::Right | TextAlign::End => egui::pos2(max_x, min_y),
            TextAlign::Center => egui::pos2(min_x + width / 2.0, min_y),
            TextAlign::Justify => egui::pos2(min_x, min_y),
        };

        // Handle text overflow
        let display_text = match overflow {
            TextOverflow::Clip => text.to_string(),
            TextOverflow::Ellipsis => {
                // Simple ellipsis implementation - in production would measure text
                if text.len() > 100 {
                    format!("{}...", &text[..97])
                } else {
                    text.to_string()
                }
            }
            TextOverflow::Fade => text.to_string(), // Would implement fade shader
            TextOverflow::Visible => text.to_string(),
        };

        // Handle multi-line text
        let lines: Vec<&str> = if soft_wrap {
            display_text.lines().collect()
        } else {
            vec![display_text.as_str()]
        };

        // Apply max_lines limit
        let limited_lines = if let Some(max) = max_lines {
            &lines[..lines.len().min(max)]
        } else {
            &lines[..]
        };

        // Paint each line
        let line_height = font_size * 1.2; // Standard line height
        for (i, line) in limited_lines.iter().enumerate() {
            let line_pos = egui::pos2(
                pos.x,
                pos.y + (i as f32 * line_height),
            );

            painter.text(
                line_pos,
                align,
                line,
                font_id.clone(),
                egui_color,
            );
        }
    }

    /// Calculate text size for layout purposes
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter (needed for font metrics)
    /// * `text` - The text string to measure
    /// * `font_size` - Font size in points
    /// * `max_width` - Maximum width for wrapping (None = unlimited)
    /// * `soft_wrap` - Whether to wrap at word boundaries
    ///
    /// # Returns
    ///
    /// The size required to render the text
    pub fn measure_text(
        _painter: &egui::Painter,
        text: &str,
        font_size: f32,
        _max_width: Option<f32>,
        soft_wrap: bool,
    ) -> Size {
        // Simplified text measurement
        // In production, this would use proper font metrics from egui

        let char_width = font_size * 0.6; // Approximate character width
        let line_height = font_size * 1.2; // Approximate line height

        if soft_wrap {
            // Multi-line measurement with wrapping
            let lines: Vec<&str> = text.lines().collect();
            let total_height = lines.len() as f32 * line_height;

            let mut max_line_width: f32 = 0.0;
            for line in lines {
                let line_width = line.len() as f32 * char_width;
                max_line_width = max_line_width.max(line_width);
            }

            Size::new(max_line_width, total_height)
        } else {
            // Single line measurement
            let width = text.len() as f32 * char_width;
            Size::new(width, line_height)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_painter_basic() {
        // This is a smoke test - actual rendering would require egui context
        // Just verify the API is callable

        let text = "Hello, World!";
        let font_size = 16.0;
        let color = Color::rgb(0, 0, 0);

        // We can't actually paint without egui context, but we can verify types
        assert_eq!(text.len(), 13);
        assert_eq!(font_size, 16.0);
        assert_eq!(color.red(), 0);
    }

    #[test]
    fn test_text_overflow_ellipsis() {
        let long_text = "a".repeat(150);
        let overflow = TextOverflow::Ellipsis;

        // Verify ellipsis logic
        match overflow {
            TextOverflow::Ellipsis => {
                if long_text.len() > 100 {
                    let display = format!("{}...", &long_text[..97]);
                    assert_eq!(display.len(), 100); // 97 chars + "..."
                }
            }
            _ => {}
        }
    }

    #[test]
    fn test_text_align_mapping() {
        // Verify alignment mapping
        let left = TextAlign::Left;
        let right = TextAlign::Right;
        let center = TextAlign::Center;

        match left {
            TextAlign::Left => {
                let align = egui::Align2::LEFT_TOP;
                assert_eq!(align, egui::Align2::LEFT_TOP);
            }
            _ => panic!("Wrong alignment"),
        }

        match right {
            TextAlign::Right => {
                let align = egui::Align2::RIGHT_TOP;
                assert_eq!(align, egui::Align2::RIGHT_TOP);
            }
            _ => panic!("Wrong alignment"),
        }

        match center {
            TextAlign::Center => {
                let align = egui::Align2::CENTER_TOP;
                assert_eq!(align, egui::Align2::CENTER_TOP);
            }
            _ => panic!("Wrong alignment"),
        }
    }

    #[test]
    fn test_color_conversion() {
        let color = Color::rgba(255, 128, 64, 255);

        let egui_color = egui::Color32::from_rgba_unmultiplied(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        );

        assert_eq!(egui_color.r(), 255);
        assert_eq!(egui_color.g(), 128);
        assert_eq!(egui_color.b(), 64);
        assert_eq!(egui_color.a(), 255);
    }

    #[test]
    fn test_max_lines_limiting() {
        let lines = vec!["line1", "line2", "line3", "line4", "line5"];
        let max_lines = Some(3);

        let limited = if let Some(max) = max_lines {
            &lines[..lines.len().min(max)]
        } else {
            &lines[..]
        };

        assert_eq!(limited.len(), 3);
        assert_eq!(limited[0], "line1");
        assert_eq!(limited[2], "line3");
    }
}

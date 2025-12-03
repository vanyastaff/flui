//! Minimal text rendering module
//!
//! Provides RenderParagraph for text display.

use crate::core::{BoxLayoutContext, BoxPaintContext, Leaf, RenderBox, RenderObject, RenderResult};
use flui_painting::Paint;
use flui_types::{
    styling::Color,
    typography::{TextAlign, TextDirection, TextOverflow, TextStyle},
    Size,
};
use std::any::Any;

/// Data for RenderParagraph
#[derive(Debug, Clone)]
pub struct ParagraphData {
    /// The text to display
    pub text: String,
    /// Text style (size, color, font, etc.)
    pub font_size: f32,
    /// Text color
    pub color: Color,
    /// Text alignment
    pub text_align: TextAlign,
    /// Text direction
    pub text_direction: TextDirection,
    /// Maximum number of lines (None = unlimited)
    pub max_lines: Option<usize>,
    /// Text overflow behavior
    pub overflow: TextOverflow,
    /// Whether to wrap text at word boundaries
    pub soft_wrap: bool,
}

impl ParagraphData {
    /// Create new paragraph data
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_size: 14.0,
            color: Color::BLACK,
            text_align: TextAlign::default(),
            text_direction: TextDirection::default(),
            max_lines: None,
            overflow: TextOverflow::default(),
            soft_wrap: true,
        }
    }

    /// Set font size
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Set color
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Set text alignment
    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }

    /// Set max lines
    pub fn with_max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    /// Set overflow behavior
    pub fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }
}

/// RenderParagraph - Multi-line text rendering
///
/// This is a Leaf RenderObject (no children) that renders text.
#[derive(Debug)]
pub struct RenderParagraph {
    /// The paragraph data
    data: ParagraphData,
    /// Cached layout size
    size: Size,
}

impl RenderParagraph {
    /// Create new RenderParagraph
    pub fn new(data: ParagraphData) -> Self {
        Self {
            data,
            size: Size::ZERO,
        }
    }

    /// Get reference to data
    pub fn data(&self) -> &ParagraphData {
        &self.data
    }

    /// Get mutable reference to data
    pub fn data_mut(&mut self) -> &mut ParagraphData {
        &mut self.data
    }

    /// Get the text
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Set the text
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.data.text = text.into();
    }

    /// Get font size
    pub fn font_size(&self) -> f32 {
        self.data.font_size
    }

    /// Set font size
    pub fn set_font_size(&mut self, size: f32) {
        self.data.font_size = size;
    }

    /// Get text color
    pub fn color(&self) -> Color {
        self.data.color
    }

    /// Set text color
    pub fn set_color(&mut self, color: Color) {
        self.data.color = color;
    }
}

impl RenderObject for RenderParagraph {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        "RenderParagraph"
    }
}

impl RenderBox<Leaf> for RenderParagraph {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Simple text size estimation
        let char_width = self.data.font_size * 0.6;
        let line_height = self.data.font_size * 1.2;

        let text_len = self.data.text.len() as f32;
        let max_width = constraints.max_width;

        // Simple text wrapping simulation
        let chars_per_line = if self.data.soft_wrap && max_width.is_finite() {
            (max_width / char_width).max(1.0) as usize
        } else {
            self.data.text.len().max(1)
        };

        let num_lines = if chars_per_line > 0 {
            ((text_len / chars_per_line as f32).ceil() as usize).max(1)
        } else {
            1
        };

        // Apply max_lines constraint
        let actual_lines = if let Some(max_lines) = self.data.max_lines {
            num_lines.min(max_lines)
        } else {
            num_lines
        };

        // Calculate actual text width
        let actual_text_width = (text_len * char_width).min(max_width);

        let width = if self.data.soft_wrap && max_width.is_finite() && actual_text_width > max_width
        {
            max_width
        } else {
            actual_text_width
        };

        let height = actual_lines as f32 * line_height;
        let size = constraints.constrain(Size::new(width, height));

        self.size = size;
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
        let paint = Paint {
            color: self.data.color,
            ..Default::default()
        };

        let offset = ctx.offset;

        // Calculate text position based on alignment
        let char_width = self.data.font_size * 0.6;
        let text_width = (self.data.text.len() as f32) * char_width;

        let x_local = match self.data.text_align {
            TextAlign::Left | TextAlign::Start => 0.0,
            TextAlign::Center => ((self.size.width - text_width) / 2.0).max(0.0),
            TextAlign::Right | TextAlign::End => (self.size.width - text_width).max(0.0),
            TextAlign::Justify => 0.0,
        };

        let position = flui_types::Offset::new(offset.dx + x_local, offset.dy);

        let text_style = TextStyle::default()
            .with_font_size(self.data.font_size as f64)
            .with_color(self.data.color);

        ctx.canvas_mut()
            .draw_text(&self.data.text, position, &text_style, &paint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paragraph_data_new() {
        let data = ParagraphData::new("Hello");
        assert_eq!(data.text, "Hello");
        assert_eq!(data.font_size, 14.0);
    }

    #[test]
    fn test_render_paragraph_new() {
        let data = ParagraphData::new("Hello, World!");
        let paragraph = RenderParagraph::new(data);
        assert_eq!(paragraph.text(), "Hello, World!");
    }
}

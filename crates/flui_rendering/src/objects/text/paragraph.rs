//! RenderParagraph - Multi-line text rendering
//!
//! This is a Leaf RenderObject that renders multi-line text with styling,
//! line breaks, and text wrapping.

use flui_core::DynRenderObject;
use flui_types::{
    Offset, Rect, Size,
    constraints::BoxConstraints,
    styling::Color,
    typography::{TextAlign, TextDirection, TextOverflow},
};
use flui_painting::TextPainter;

use crate::core::{RenderBoxMixin, LeafRenderBox};
use crate::delegate_to_mixin;

// ===== Data Structure =====
// Note: TextAlign, TextDirection, TextOverflow are imported from flui_types

/// Data for RenderParagraph
#[derive(Debug, Clone)]
pub struct ParagraphData {
    /// The text to display
    pub text: String,
    /// Text style (size, color, font, etc.)
    pub font_size: f32,
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

// ===== Type Alias =====

/// RenderParagraph - Multi-line text rendering
///
/// This is a Leaf RenderObject (no children) that renders text.
/// Supports:
/// - Multi-line text with wrapping
/// - Text alignment (left, right, center, justify)
/// - Text direction (LTR, RTL)
/// - Max lines and overflow handling
/// - Text styling (size, color)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderParagraph, ParagraphData};
///
/// let data = ParagraphData::new("Hello, World!")
///     .with_font_size(16.0)
///     .with_align(TextAlign::Center);
/// let mut paragraph = RenderParagraph::new(data);
/// ```
pub type RenderParagraph = LeafRenderBox<ParagraphData>;

// ===== Methods =====

impl RenderParagraph {
    /// Get the text
    pub fn text(&self) -> &str {
        &self.data().text
    }

    /// Set the text
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.data_mut().text = text.into();
        self.mark_needs_layout(); // Text change requires re-layout
    }

    /// Get font size
    pub fn font_size(&self) -> f32 {
        self.data().font_size
    }

    /// Set font size
    pub fn set_font_size(&mut self, size: f32) {
        if self.data().font_size != size {
            self.data_mut().font_size = size;
            self.mark_needs_layout(); // Size change requires re-layout
        }
    }

    /// Get text color
    pub fn color(&self) -> Color {
        self.data().color
    }

    /// Set text color
    pub fn set_color(&mut self, color: Color) {
        if self.data().color != color {
            self.data_mut().color = color;
            self.mark_needs_paint(); // Color change only needs repaint
        }
    }

    /// Get text alignment
    pub fn text_align(&self) -> TextAlign {
        self.data().text_align
    }

    /// Set text alignment
    pub fn set_text_align(&mut self, align: TextAlign) {
        if self.data().text_align != align {
            self.data_mut().text_align = align;
            self.mark_needs_paint(); // Alignment change only needs repaint
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderParagraph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.state_mut().constraints = Some(constraints);

        // Calculate text size
        // In production, this would use a proper text layout engine
        // For now, use simple estimation based on character count and font size

        let data = self.data();
        let char_width = data.font_size * 0.6; // Approximate character width
        let line_height = data.font_size * 1.2; // Approximate line height

        let text_len = data.text.len() as f32;
        let max_width = constraints.max_width.min(constraints.max_width);

        // Simple text wrapping simulation
        let chars_per_line = if data.soft_wrap && max_width.is_finite() {
            (max_width / char_width).max(1.0) as usize
        } else {
            data.text.len()
        };

        let num_lines = if chars_per_line > 0 {
            ((text_len / chars_per_line as f32).ceil() as usize).max(1)
        } else {
            1
        };

        // Apply max_lines constraint
        let actual_lines = if let Some(max_lines) = data.max_lines {
            num_lines.min(max_lines)
        } else {
            num_lines
        };

        let width = if data.soft_wrap && max_width.is_finite() {
            max_width
        } else {
            (chars_per_line as f32 * char_width).min(max_width)
        };

        let height = actual_lines as f32 * line_height;

        let size = constraints.constrain(Size::new(width, height));
        self.state_mut().size = Some(size);
        self.clear_needs_layout();
        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(size) = self.state().size {
            let data = self.data();

            // Create rect for text rendering
            let rect = Rect::from_xywh(offset.dx, offset.dy, size.width, size.height);

            // Use TextPainter from flui_painting
            TextPainter::paint(
                painter,
                rect,
                &data.text,
                data.font_size,
                data.color,
                data.text_align,
                data.text_direction,
                data.max_lines,
                data.overflow,
                data.soft_wrap,
            );
        }
    }

    // Delegate all other methods to the mixin
    delegate_to_mixin!();
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_align_default() {
        assert_eq!(TextAlign::default(), TextAlign::Left);
    }

    #[test]
    fn test_text_direction_default() {
        assert_eq!(TextDirection::default(), TextDirection::Ltr);
    }

    #[test]
    fn test_text_overflow_default() {
        assert_eq!(TextOverflow::default(), TextOverflow::Clip);
    }

    #[test]
    fn test_paragraph_data_new() {
        let data = ParagraphData::new("Hello");
        assert_eq!(data.text, "Hello");
        assert_eq!(data.font_size, 14.0);
        assert_eq!(data.color, Color::BLACK);
        assert_eq!(data.text_align, TextAlign::Left);
        assert!(data.soft_wrap);
    }

    #[test]
    fn test_paragraph_data_builder() {
        let data = ParagraphData::new("Test")
            .with_font_size(20.0)
            .with_color(Color::RED)
            .with_align(TextAlign::Center)
            .with_max_lines(3)
            .with_overflow(TextOverflow::Ellipsis);

        assert_eq!(data.font_size, 20.0);
        assert_eq!(data.color, Color::RED);
        assert_eq!(data.text_align, TextAlign::Center);
        assert_eq!(data.max_lines, Some(3));
        assert_eq!(data.overflow, TextOverflow::Ellipsis);
    }

    #[test]
    fn test_render_paragraph_new() {
        let data = ParagraphData::new("Hello, World!");
        let paragraph = LeafRenderBox::new(data);
        assert_eq!(paragraph.text(), "Hello, World!");
        assert_eq!(paragraph.font_size(), 14.0);
    }

    #[test]
    fn test_render_paragraph_set_text() {
        let data = ParagraphData::new("Initial");
        let mut paragraph = LeafRenderBox::new(data);

        paragraph.set_text("Updated");
        assert_eq!(paragraph.text(), "Updated");
        assert!(paragraph.needs_layout());
    }

    #[test]
    fn test_render_paragraph_set_font_size() {
        let data = ParagraphData::new("Text");
        let mut paragraph = LeafRenderBox::new(data);

        // Layout first to clear initial needs_layout
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        paragraph.layout(constraints);

        paragraph.set_font_size(20.0);
        assert_eq!(paragraph.font_size(), 20.0);
        assert!(paragraph.needs_layout());
    }

    #[test]
    fn test_render_paragraph_set_color() {
        let data = ParagraphData::new("Text");
        let mut paragraph = LeafRenderBox::new(data);

        // Layout first to clear initial needs_paint
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        paragraph.layout(constraints);

        paragraph.set_color(Color::BLUE);
        assert_eq!(paragraph.color(), Color::BLUE);
        assert!(paragraph.needs_paint());
        assert!(!paragraph.needs_layout()); // Color change doesn't need layout
    }

    #[test]
    fn test_render_paragraph_set_text_align() {
        let data = ParagraphData::new("Text");
        let mut paragraph = LeafRenderBox::new(data);

        // Layout first
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        paragraph.layout(constraints);

        paragraph.set_text_align(TextAlign::Center);
        assert_eq!(paragraph.text_align(), TextAlign::Center);
        assert!(paragraph.needs_paint());
        assert!(!paragraph.needs_layout()); // Alignment change doesn't need layout
    }

    #[test]
    fn test_render_paragraph_layout() {
        let data = ParagraphData::new("Hello, World!");
        let mut paragraph = LeafRenderBox::new(data);

        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
        let size = paragraph.layout(constraints);

        // Should have some size based on text
        assert!(size.width > 0.0);
        assert!(size.height > 0.0);
        assert!(size.width <= 200.0);
        assert!(size.height <= 100.0);
    }

    #[test]
    fn test_render_paragraph_max_lines() {
        let long_text = "This is a very long text that will definitely need multiple lines to display properly";
        let data = ParagraphData::new(long_text).with_max_lines(2);
        let mut paragraph = LeafRenderBox::new(data);

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 1000.0);
        let size = paragraph.layout(constraints);

        // Height should be limited by max_lines
        let line_height = 14.0 * 1.2; // font_size * line_height_factor
        assert!(size.height <= 2.0 * line_height + 1.0); // +1 for rounding
    }
}

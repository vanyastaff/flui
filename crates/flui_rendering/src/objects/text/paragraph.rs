//! RenderParagraph - Multi-line text rendering
//!
//! This is a Leaf RenderObject that renders multi-line text with styling,
//! line breaks, and text wrapping.

use flui_core::render::LeafRender;
use flui_engine::{BoxedLayer, layer::pool};
use flui_types::{
    Offset, Point, Size,
    constraints::BoxConstraints,
    styling::Color,
    typography::{TextAlign, TextDirection, TextOverflow, TextStyle},
};

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

// ===== RenderObject =====

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
#[derive(Debug)]
pub struct RenderParagraph {
    /// The paragraph data
    data: ParagraphData,
    /// Cached layout size (set during layout, used during paint)
    size: Option<Size>,
}

impl RenderParagraph {
    /// Create new RenderParagraph
    pub fn new(data: ParagraphData) -> Self {
        Self { data, size: None }
    }

    /// Get reference to data
    pub fn data(&self) -> &ParagraphData {
        &self.data
    }

    /// Get mutable reference to data
    pub fn data_mut(&mut self) -> &mut ParagraphData {
        &mut self.data
    }
}

// ===== Helper Methods =====

impl RenderParagraph {
    /// Get the text
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Set the text (caller must trigger re-layout in the framework)
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.data.text = text.into();
    }

    /// Get font size
    pub fn font_size(&self) -> f32 {
        self.data.font_size
    }

    /// Set font size (caller must trigger re-layout in the framework)
    pub fn set_font_size(&mut self, size: f32) {
        self.data.font_size = size;
    }

    /// Get text color
    pub fn color(&self) -> Color {
        self.data.color
    }

    /// Set text color (caller must trigger repaint in the framework)
    pub fn set_color(&mut self, color: Color) {
        self.data.color = color;
    }

    /// Get text alignment
    pub fn text_align(&self) -> TextAlign {
        self.data.text_align
    }

    /// Set text alignment (caller must trigger repaint in the framework)
    pub fn set_text_align(&mut self, align: TextAlign) {
        self.data.text_align = align;
    }
}

// ===== RenderObject Implementation =====

impl LeafRender for RenderParagraph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Calculate text size
        // In production, this would use a proper text layout engine
        // For now, use simple estimation based on character count and font size

        let char_width = self.data.font_size * 0.6; // Approximate character width
        let line_height = self.data.font_size * 1.2; // Approximate line height

        let text_len = self.data.text.len() as f32;
        let max_width = constraints.max_width;

        // Simple text wrapping simulation
        let chars_per_line = if self.data.soft_wrap && max_width.is_finite() {
            (max_width / char_width).max(1.0) as usize
        } else {
            self.data.text.len()
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

        // Calculate actual text width (intrinsic size)
        // Text should only take as much width as needed, not expand to fill
        let actual_text_width = (text_len * char_width).min(max_width);

        let width = if self.data.soft_wrap && max_width.is_finite() && actual_text_width > max_width
        {
            // Only expand to max_width if text is too long and needs wrapping
            max_width
        } else {
            // Otherwise, use intrinsic width (text's natural size)
            actual_text_width
        };

        let height = actual_lines as f32 * line_height;

        let size = constraints.constrain(Size::new(width, height));

        // Cache the size for painting
        self.size = Some(size);

        size
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        let mut pooled = flui_engine::PooledPictureLayer::new(pool::acquire_picture());

        if let Some(size) = self.size {
            // Create text style from paragraph data
            let style = TextStyle {
                font_size: Some(self.data.font_size as f64),
                color: Some(self.data.color),
                ..Default::default()
            };

            // Paint in LOCAL coordinates (0, 0) - transform will be applied by parent
            let position = Point::new(0.0, 0.0);

            // Draw text to picture layer
            pooled.as_mut().draw_text(&self.data.text, position, style);
        }

        // Wrap in TransformLayer to apply offset
        let picture_layer: BoxedLayer = Box::new(pooled);
        if offset != Offset::ZERO {
            Box::new(flui_engine::TransformLayer::translate(picture_layer, offset))
        } else {
            picture_layer
        }
    }
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
        let paragraph = RenderParagraph::new(data);
        assert_eq!(paragraph.text(), "Hello, World!");
        assert_eq!(paragraph.font_size(), 14.0);
    }

    #[test]
    fn test_render_paragraph_set_text() {
        let data = ParagraphData::new("Initial");
        let mut paragraph = RenderParagraph::new(data);

        paragraph.set_text("Updated");
        assert_eq!(paragraph.text(), "Updated");
    }

    #[test]
    fn test_render_paragraph_set_font_size() {
        let data = ParagraphData::new("Text");
        let mut paragraph = RenderParagraph::new(data);

        paragraph.set_font_size(20.0);
        assert_eq!(paragraph.font_size(), 20.0);
    }

    #[test]
    fn test_render_paragraph_set_color() {
        let data = ParagraphData::new("Text");
        let mut paragraph = RenderParagraph::new(data);

        paragraph.set_color(Color::BLUE);
        assert_eq!(paragraph.color(), Color::BLUE);
    }

    #[test]
    fn test_render_paragraph_set_text_align() {
        let data = ParagraphData::new("Text");
        let mut paragraph = RenderParagraph::new(data);

        paragraph.set_text_align(TextAlign::Center);
        assert_eq!(paragraph.text_align(), TextAlign::Center);
    }

    // Note: Layout tests require ElementTree setup which is more complex in new architecture
    // These tests would need to be integration tests with proper tree setup
}

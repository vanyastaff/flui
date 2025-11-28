//! RenderEditableLine - Single-line editable text

use crate::core::{BoxProtocol, LayoutContext, PaintContext};
use crate::core::{Leaf, RenderBox};
use flui_painting::{Canvas, Paint};
use flui_types::prelude::{Color, TextAlign, TextStyle};
use flui_types::{Rect, Size};

/// Text selection range
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    /// Start position (base of selection)
    pub base: usize,
    /// End position (extent of selection)
    pub extent: usize,
}

impl TextSelection {
    /// Create new selection
    pub fn new(base: usize, extent: usize) -> Self {
        Self { base, extent }
    }

    /// Create collapsed selection (cursor at position)
    pub fn collapsed(position: usize) -> Self {
        Self {
            base: position,
            extent: position,
        }
    }

    /// Check if selection is collapsed (cursor)
    pub fn is_collapsed(&self) -> bool {
        self.base == self.extent
    }

    /// Get selection start (min of base/extent)
    pub fn start(&self) -> usize {
        self.base.min(self.extent)
    }

    /// Get selection end (max of base/extent)
    pub fn end(&self) -> usize {
        self.base.max(self.extent)
    }

    /// Get selection length
    pub fn length(&self) -> usize {
        self.end() - self.start()
    }
}

impl Default for TextSelection {
    fn default() -> Self {
        Self::collapsed(0)
    }
}

/// RenderObject for single-line editable text input
///
/// Displays text with cursor and selection support. This is a simplified
/// implementation for basic text input needs.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderEditableLine;
/// use flui_painting::{TextStyle, Color};
///
/// let style = TextStyle::default().with_color(Color::BLACK);
/// let mut editable = RenderEditableLine::new("Hello".to_string(), style);
/// editable.set_cursor_position(5);
/// ```
#[derive(Debug)]
pub struct RenderEditableLine {
    /// Current text content
    pub text: String,
    /// Text style
    pub style: TextStyle,
    /// Text alignment
    pub text_align: TextAlign,
    /// Current selection/cursor
    pub selection: TextSelection,
    /// Whether to show cursor
    pub show_cursor: bool,
    /// Cursor color
    pub cursor_color: Color,
    /// Cursor width
    pub cursor_width: f32,
    /// Selection highlight color
    pub selection_color: Color,
    /// Whether text is read-only
    pub read_only: bool,
    /// Whether to obscure text (password field)
    pub obscure_text: bool,

    // Cache for layout
    size: Size,
}

impl RenderEditableLine {
    /// Create new editable line
    pub fn new(text: String, style: TextStyle) -> Self {
        Self {
            text,
            style,
            text_align: TextAlign::Left,
            selection: TextSelection::default(),
            show_cursor: true,
            cursor_color: Color::BLACK,
            cursor_width: 2.0,
            selection_color: Color::rgba(76, 153, 255, 76), // Light blue with 30% alpha
            read_only: false,
            obscure_text: false,
            size: Size::ZERO,
        }
    }

    /// Create empty editable line
    pub fn empty(style: TextStyle) -> Self {
        Self::new(String::new(), style)
    }

    /// Set text content
    pub fn set_text(&mut self, text: String) {
        self.text = text;
        // Clamp selection to new text length
        let len = self.text.len();
        self.selection.base = self.selection.base.min(len);
        self.selection.extent = self.selection.extent.min(len);
    }

    /// Set cursor position (collapsed selection)
    pub fn set_cursor_position(&mut self, position: usize) {
        let pos = position.min(self.text.len());
        self.selection = TextSelection::collapsed(pos);
    }

    /// Set selection range
    pub fn set_selection(&mut self, start: usize, end: usize) {
        let len = self.text.len();
        self.selection = TextSelection::new(start.min(len), end.min(len));
    }

    /// Insert text at cursor
    pub fn insert_text(&mut self, text: &str) {
        if self.read_only {
            return;
        }

        let cursor = self.selection.base;
        self.text.insert_str(cursor, text);
        self.set_cursor_position(cursor + text.len());
    }

    /// Delete character before cursor (backspace)
    pub fn delete_before_cursor(&mut self) {
        if self.read_only {
            return;
        }

        if self.selection.is_collapsed() {
            let cursor = self.selection.base;
            if cursor > 0 {
                self.text.remove(cursor - 1);
                self.set_cursor_position(cursor - 1);
            }
        } else {
            // Delete selection
            let start = self.selection.start();
            let end = self.selection.end();
            self.text.drain(start..end);
            self.set_cursor_position(start);
        }
    }

    /// Delete character after cursor (delete key)
    pub fn delete_after_cursor(&mut self) {
        if self.read_only {
            return;
        }

        if self.selection.is_collapsed() {
            let cursor = self.selection.base;
            if cursor < self.text.len() {
                self.text.remove(cursor);
            }
        } else {
            // Delete selection
            let start = self.selection.start();
            let end = self.selection.end();
            self.text.drain(start..end);
            self.set_cursor_position(start);
        }
    }

    /// Get display text (obscured if password)
    fn display_text(&self) -> String {
        if self.obscure_text && !self.text.is_empty() {
            "•".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        }
    }

    /// Calculate text width (simplified - assumes monospace)
    fn calculate_text_width(&self, text: &str) -> f32 {
        // Simplified calculation based on font size
        // In real implementation, this would use proper text measurement
        let font_size = self.style.font_size.unwrap_or(16.0) as f32;
        let char_width = font_size * 0.6; // Approximation
        text.chars().count() as f32 * char_width
    }

    /// Get cursor X position
    fn get_cursor_x(&self) -> f32 {
        let display = self.display_text();
        let before_cursor = display
            .chars()
            .take(self.selection.base)
            .collect::<String>();
        self.calculate_text_width(&before_cursor)
    }
}

impl<T: FullRenderTree> RenderBox<T, Leaf> for RenderEditableLine {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;

        // Calculate text size
        let display = self.display_text();
        let text_width = self.calculate_text_width(&display);
        let font_size = self.style.font_size.unwrap_or(16.0) as f32;
        let text_height = font_size * 1.2; // Line height

        // Size based on constraints
        let width = text_width
            .max(100.0)
            .clamp(constraints.min_width, constraints.max_width);
        let height = text_height.clamp(constraints.min_height, constraints.max_height);

        let size = Size::new(width, height);
        self.size = size;
        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: crate::core::PaintTree,
    {
        // Draw selection highlight if not collapsed
        if !self.selection.is_collapsed() {
            let selection_rect = Rect::from_xywh(
                self.selection.start() as f32 * 8.0, // Approximate char width
                0.0,
                self.selection.length() as f32 * 8.0,
                self.size.height,
            );
            let selection_paint = Paint::fill(self.selection_color);
            ctx.canvas().rect(selection_rect, &selection_paint);
        }

        // Draw text
        let text_paint = Paint::fill(self.style.color.unwrap_or(Color::BLACK));
        let display_text = if self.obscure_text {
            "•".repeat(self.text.len())
        } else {
            self.text.clone()
        };

        ctx.canvas().text(
            &display_text,
            flui_types::Offset::ZERO,
            &self.style,
            &text_paint,
        );

        // Draw cursor if showing using conditional drawing
        if self.show_cursor {
            let cursor_x = self.selection.base as f32 * 8.0; // Approximate
            let cursor_rect = Rect::from_xywh(cursor_x, 0.0, self.cursor_width, self.size.height);
            let cursor_paint = Paint::fill(self.cursor_color);
            ctx.canvas().rect(cursor_rect, &cursor_paint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_selection_new() {
        let selection = TextSelection::new(0, 5);
        assert_eq!(selection.base, 0);
        assert_eq!(selection.extent, 5);
    }

    #[test]
    fn test_text_selection_collapsed() {
        let selection = TextSelection::collapsed(3);
        assert_eq!(selection.base, 3);
        assert_eq!(selection.extent, 3);
        assert!(selection.is_collapsed());
    }

    #[test]
    fn test_text_selection_range() {
        let selection = TextSelection::new(2, 7);
        assert_eq!(selection.start(), 2);
        assert_eq!(selection.end(), 7);
        assert_eq!(selection.length(), 5);
        assert!(!selection.is_collapsed());
    }

    #[test]
    fn test_text_selection_reversed() {
        let selection = TextSelection::new(7, 2);
        assert_eq!(selection.start(), 2);
        assert_eq!(selection.end(), 7);
        assert_eq!(selection.length(), 5);
    }

    #[test]
    fn test_render_editable_line_new() {
        let style = TextStyle::default();
        let editable = RenderEditableLine::new("Hello".to_string(), style);

        assert_eq!(editable.text, "Hello");
        assert_eq!(editable.selection, TextSelection::collapsed(0));
        assert!(editable.show_cursor);
        assert!(!editable.read_only);
    }

    #[test]
    fn test_render_editable_line_empty() {
        let style = TextStyle::default();
        let editable = RenderEditableLine::empty(style);

        assert_eq!(editable.text, "");
        assert_eq!(editable.selection, TextSelection::collapsed(0));
    }

    #[test]
    fn test_set_text() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);
        editable.set_cursor_position(5);

        editable.set_text("Hi".to_string());

        assert_eq!(editable.text, "Hi");
        assert_eq!(editable.selection.base, 2); // Clamped to text length
    }

    #[test]
    fn test_set_cursor_position() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);

        editable.set_cursor_position(3);
        assert_eq!(editable.selection, TextSelection::collapsed(3));

        editable.set_cursor_position(100);
        assert_eq!(editable.selection, TextSelection::collapsed(5)); // Clamped
    }

    #[test]
    fn test_set_selection() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);

        editable.set_selection(1, 4);
        assert_eq!(editable.selection.start(), 1);
        assert_eq!(editable.selection.end(), 4);
    }

    #[test]
    fn test_insert_text() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);
        editable.set_cursor_position(5);

        editable.insert_text(" World");

        assert_eq!(editable.text, "Hello World");
        assert_eq!(editable.selection.base, 11);
    }

    #[test]
    fn test_insert_text_read_only() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);
        editable.read_only = true;
        editable.set_cursor_position(5);

        editable.insert_text(" World");

        assert_eq!(editable.text, "Hello"); // No change
    }

    #[test]
    fn test_delete_before_cursor() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);
        editable.set_cursor_position(5);

        editable.delete_before_cursor();

        assert_eq!(editable.text, "Hell");
        assert_eq!(editable.selection.base, 4);
    }

    #[test]
    fn test_delete_before_cursor_at_start() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);
        editable.set_cursor_position(0);

        editable.delete_before_cursor();

        assert_eq!(editable.text, "Hello"); // No change
    }

    #[test]
    fn test_delete_selection() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello World".to_string(), style);
        editable.set_selection(6, 11);

        editable.delete_before_cursor();

        assert_eq!(editable.text, "Hello ");
        assert_eq!(editable.selection.base, 6);
    }

    #[test]
    fn test_delete_after_cursor() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("Hello".to_string(), style);
        editable.set_cursor_position(0);

        editable.delete_after_cursor();

        assert_eq!(editable.text, "ello");
        assert_eq!(editable.selection.base, 0);
    }

    #[test]
    fn test_obscure_text() {
        let style = TextStyle::default();
        let mut editable = RenderEditableLine::new("password".to_string(), style);
        editable.obscure_text = true;

        let display = editable.display_text();
        assert_eq!(display, "••••••••");
    }

    #[test]
    fn test_arity_is_leaf() {
        let style = TextStyle::default();
        let editable = RenderEditableLine::new("Hello".to_string(), style);

        assert_eq!(editable.arity(), RuntimeArity::Exact(0));
    }
}

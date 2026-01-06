//! ColoredBox widget - paints a colored rectangle
//!
//! A simple widget that paints a solid color rectangle.
//! Similar to Flutter's ColoredBox widget.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Create a red box
//! ColoredBox::new([1.0, 0.0, 0.0, 1.0], 100.0, 50.0)
//!
//! // Using convenience constructors
//! ColoredBox::red(100.0, 50.0)
//! ColoredBox::green(100.0, 50.0)
//! ColoredBox::blue(100.0, 50.0)
//! ```

use flui_rendering::objects::RenderColoredBox;
use flui_rendering::wrapper::BoxWrapper;
use flui_types::Size;
use flui_view::{impl_render_view, RenderView};

/// A widget that paints a solid color rectangle.
///
/// This is the simplest visual widget - it just fills its area with a color.
///
/// ## Layout Behavior
///
/// ColoredBox has a preferred size but will be constrained by its parent.
/// If the parent provides tight constraints, ColoredBox will use those.
///
/// ## Examples
///
/// ```rust,ignore
/// // Red square
/// ColoredBox::red(100.0, 100.0)
///
/// // Custom color (RGBA, 0.0-1.0)
/// ColoredBox::new([0.5, 0.5, 0.5, 1.0], 200.0, 100.0)
/// ```
#[derive(Debug, Clone)]
pub struct ColoredBox {
    /// The color as RGBA (0.0-1.0 per channel).
    pub color: [f32; 4],
    /// The preferred size.
    pub size: Size,
}

impl ColoredBox {
    /// Creates a new ColoredBox with the given color and size.
    pub fn new(color: [f32; 4], width: f32, height: f32) -> Self {
        Self {
            color,
            size: Size::new(width, height),
        }
    }

    /// Creates a red ColoredBox.
    pub fn red(width: f32, height: f32) -> Self {
        Self::new([1.0, 0.0, 0.0, 1.0], width, height)
    }

    /// Creates a green ColoredBox.
    pub fn green(width: f32, height: f32) -> Self {
        Self::new([0.0, 1.0, 0.0, 1.0], width, height)
    }

    /// Creates a blue ColoredBox.
    pub fn blue(width: f32, height: f32) -> Self {
        Self::new([0.0, 0.0, 1.0, 1.0], width, height)
    }

    /// Creates a white ColoredBox.
    pub fn white(width: f32, height: f32) -> Self {
        Self::new([1.0, 1.0, 1.0, 1.0], width, height)
    }

    /// Creates a black ColoredBox.
    pub fn black(width: f32, height: f32) -> Self {
        Self::new([0.0, 0.0, 0.0, 1.0], width, height)
    }

    /// Creates a transparent ColoredBox.
    pub fn transparent(width: f32, height: f32) -> Self {
        Self::new([0.0, 0.0, 0.0, 0.0], width, height)
    }

    /// Creates a ColoredBox from a hex color (0xRRGGBB).
    pub fn from_hex(hex: u32, width: f32, height: f32) -> Self {
        let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
        let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
        let b = (hex & 0xFF) as f32 / 255.0;
        Self::new([r, g, b, 1.0], width, height)
    }

    /// Creates a ColoredBox from a hex color with alpha (0xRRGGBBAA).
    pub fn from_hex_with_alpha(hex: u32, width: f32, height: f32) -> Self {
        let r = ((hex >> 24) & 0xFF) as f32 / 255.0;
        let g = ((hex >> 16) & 0xFF) as f32 / 255.0;
        let b = ((hex >> 8) & 0xFF) as f32 / 255.0;
        let a = (hex & 0xFF) as f32 / 255.0;
        Self::new([r, g, b, a], width, height)
    }
}

// Implement View trait via macro
impl_render_view!(ColoredBox);

impl RenderView for ColoredBox {
    type RenderObject = BoxWrapper<RenderColoredBox>;

    fn create_render_object(&self) -> Self::RenderObject {
        BoxWrapper::new(RenderColoredBox::new(self.color, self.size))
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        let inner = render_object.inner();
        if inner.color() != self.color || inner.preferred_size() != self.size {
            // RenderColoredBox is immutable after creation, recreate
            *render_object = BoxWrapper::new(RenderColoredBox::new(self.color, self.size));
        }
    }

    fn has_children(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colored_box_new() {
        let box_widget = ColoredBox::new([0.5, 0.5, 0.5, 1.0], 100.0, 50.0);
        assert_eq!(box_widget.color, [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(box_widget.size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_colored_box_red() {
        let box_widget = ColoredBox::red(100.0, 50.0);
        assert_eq!(box_widget.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_colored_box_green() {
        let box_widget = ColoredBox::green(100.0, 50.0);
        assert_eq!(box_widget.color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn test_colored_box_blue() {
        let box_widget = ColoredBox::blue(100.0, 50.0);
        assert_eq!(box_widget.color, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn test_colored_box_from_hex() {
        // Red
        let red = ColoredBox::from_hex(0xFF0000, 10.0, 10.0);
        assert_eq!(red.color, [1.0, 0.0, 0.0, 1.0]);

        // Green
        let green = ColoredBox::from_hex(0x00FF00, 10.0, 10.0);
        assert_eq!(green.color, [0.0, 1.0, 0.0, 1.0]);

        // Green with alpha
        let green_alpha = ColoredBox::from_hex_with_alpha(0x00FF00FF, 10.0, 10.0);
        assert_eq!(green_alpha.color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn test_render_view_create() {
        let box_widget = ColoredBox::red(100.0, 50.0);
        let render = box_widget.create_render_object();
        assert_eq!(render.inner().color(), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(render.inner().preferred_size(), Size::new(100.0, 50.0));
    }
}

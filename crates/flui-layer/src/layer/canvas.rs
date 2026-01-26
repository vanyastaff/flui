//! Canvas layer - leaf layer with actual drawing commands
//!
//! CanvasLayer is the most common layer type, containing a Canvas
//! with drawing commands that will be rendered to the screen.

use flui_painting::{Canvas, DisplayList, DisplayListCore};
use flui_types::geometry::{Pixels, Rect};

/// Canvas layer - a leaf layer that contains drawing commands
///
/// # Architecture
///
/// ```text
/// Canvas → DisplayList → CanvasLayer → CommandRenderer → GPU
/// ```
///
/// CanvasLayer wraps a Canvas and provides access to its DisplayList
/// for rendering. The actual rendering is done by CommandRenderer
/// implementations in flui_engine.
///
/// # Example
///
/// ```rust
/// use flui_layer::CanvasLayer;
/// use flui_painting::Canvas;
///
/// // Create from existing canvas
/// let canvas = Canvas::new();
/// let layer = CanvasLayer::from_canvas(canvas);
///
/// // Or create empty
/// let empty_layer = CanvasLayer::new();
/// ```
#[derive(Default)]
pub struct CanvasLayer {
    canvas: Canvas,
}

// Manual Debug implementation (Canvas may not derive Debug)
impl std::fmt::Debug for CanvasLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasLayer")
            .field("bounds", &self.bounds())
            .finish()
    }
}

impl CanvasLayer {
    /// Creates a new empty canvas layer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a canvas layer from an existing canvas.
    pub fn from_canvas(canvas: Canvas) -> Self {
        Self { canvas }
    }

    /// Clears the canvas, removing all drawing commands.
    pub fn clear(&mut self) {
        self.canvas = Canvas::new();
    }

    /// Returns a reference to the underlying canvas.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// Returns a mutable reference to the underlying canvas.
    pub fn canvas_mut(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    /// Returns the display list containing all drawing commands.
    pub fn display_list(&self) -> &DisplayList {
        self.canvas.display_list()
    }

    /// Returns the bounds of all drawing commands in this layer.
    pub fn bounds(&self) -> Rect<Pixels> {
        self.canvas.display_list().bounds()
    }

    /// Returns true if the canvas has no drawing commands.
    pub fn is_empty(&self) -> bool {
        self.canvas.display_list().commands().count() == 0
    }

    /// Returns the number of drawing commands in this layer.
    pub fn command_count(&self) -> usize {
        self.canvas.display_list().commands().count()
    }
}

// Thread safety
unsafe impl Send for CanvasLayer {}
unsafe impl Sync for CanvasLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_layer_new() {
        let layer = CanvasLayer::new();
        assert!(layer.is_empty());
        assert_eq!(layer.command_count(), 0);
    }

    #[test]
    fn test_canvas_layer_from_canvas() {
        let canvas = Canvas::new();
        let layer = CanvasLayer::from_canvas(canvas);
        assert!(layer.is_empty());
    }

    #[test]
    fn test_canvas_layer_clear() {
        let mut layer = CanvasLayer::new();
        layer.clear();
        assert!(layer.is_empty());
    }

    #[test]
    fn test_canvas_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<CanvasLayer>();
        assert_sync::<CanvasLayer>();
    }

    #[test]
    fn test_canvas_layer_debug() {
        let layer = CanvasLayer::new();
        let debug = format!("{:?}", layer);
        assert!(debug.contains("CanvasLayer"));
    }
}

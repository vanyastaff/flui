//! `CanvasLayer` — a live recorder inside the tree; no production producer, kept for
//! hand-authored scenes (`SceneBuilder::add_canvas`) and fixtures.

use flui_painting::{Canvas, DisplayList};
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
#[derive(Default, Clone)]
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
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps a recorder that already holds commands.
    pub fn from_canvas(canvas: Canvas) -> Self {
        Self { canvas }
    }

    /// Drops every recorded command.
    pub fn clear(&mut self) {
        self.canvas = Canvas::new();
    }

    /// The recorder.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// The recorder, for drawing into.
    pub fn canvas_mut(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    /// The commands recorded so far.
    pub fn display_list(&self) -> &DisplayList {
        self.canvas.display_list()
    }

    /// The union of the recorded commands that contribute bounds, or `None`
    /// when none does yet.
    pub fn bounds(&self) -> Option<Rect<Pixels>> {
        self.canvas.display_list().bounds()
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.canvas.display_list().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_layer_new() {
        let layer = CanvasLayer::new();
        assert!(layer.is_empty());
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
        let rect = Rect::from_xywh(
            flui_types::geometry::px(0.0),
            flui_types::geometry::px(0.0),
            flui_types::geometry::px(10.0),
            flui_types::geometry::px(10.0),
        );
        layer
            .canvas_mut()
            .draw_rect(rect, &flui_painting::Paint::default());
        assert!(!layer.is_empty());
        layer.clear();
        assert!(layer.is_empty());
        assert_eq!(layer.bounds(), None);
    }

    #[test]
    fn test_canvas_layer_debug() {
        let layer = CanvasLayer::new();
        let debug = format!("{layer:?}");
        assert!(debug.contains("CanvasLayer"));
    }
}

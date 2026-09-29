//! `CanvasLayer` — a live recorder inside the tree; no production producer, kept for
//! hand-authored scenes (`SceneBuilder::add_canvas`) and fixtures.

use flui_foundation::geometry::Rect;
use flui_painting::{Canvas, DisplayList};

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
    pub fn bounds(&self) -> Option<Rect<f64>> {
        self.canvas.display_list().bounds()
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.canvas.display_list().is_empty()
    }
}

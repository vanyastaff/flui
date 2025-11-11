//! Canvas layer - leaf layer with actual drawing commands
//!
//! REFACTORED: Now uses Clean Architecture with CommandRenderer (Visitor Pattern)

use crate::renderer::CommandRenderer;
use flui_painting::{Canvas, DisplayList};
use flui_types::Offset;

/// Canvas layer - a leaf layer that contains drawing commands
///
/// # Architecture (NEW - Clean Architecture)
///
/// ```text
/// Canvas → DisplayList → CanvasLayer.render() → CommandRenderer → GPU
///                             ↓ (Visitor Pattern)
///                         Commands execute polymorphically
/// ```
///
/// This refactored design eliminates the 250-line match statement and follows
/// the Visitor pattern for clean separation of concerns.
#[derive(Default)]
pub struct CanvasLayer {
    canvas: Canvas,
}

impl CanvasLayer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_canvas(canvas: Canvas) -> Self {
        Self { canvas }
    }

    pub fn clear(&mut self) {
        self.canvas = Canvas::new();
    }

    pub fn display_list(&self) -> &DisplayList {
        self.canvas.display_list()
    }

    /// Render using a command renderer (NEW - Visitor Pattern)
    ///
    /// This is the new clean architecture rendering path. All commands
    /// are dispatched polymorphically via the CommandRenderer trait using
    /// the **Visitor Pattern**.
    ///
    /// # Visitor Pattern
    ///
    /// The dispatcher uses the visitor pattern for polymorphic command execution:
    /// - `DrawCommand` is the visitable element (data)
    /// - `CommandRenderer` is the visitor interface
    /// - `dispatch_commands()` performs the double-dispatch
    ///
    /// This design allows adding new renderers without modifying DrawCommand.
    ///
    /// # Example
    /// ```rust,ignore
    /// let mut renderer = WgpuRenderer::new(painter);
    /// canvas_layer.render(&mut renderer);
    /// ```
    pub fn render(&self, renderer: &mut dyn CommandRenderer) {
        use crate::renderer::dispatch_commands;

        // Use visitor pattern dispatcher for clean separation of concerns
        dispatch_commands(self.canvas.display_list().commands(), renderer);
    }
}

impl CanvasLayer {
    #[deprecated(note = "Use WgpuRenderer::with_transform instead. This is legacy compatibility.")]
    #[allow(dead_code)]
    fn legacy_with_transform<F>(
        painter: &mut dyn crate::painter::Painter,
        transform: &flui_types::geometry::Matrix4,
        draw_fn: F,
    ) where
        F: FnOnce(&mut dyn crate::painter::Painter),
    {
        if transform.is_identity() {
            draw_fn(painter);
            return;
        }

        painter.save();
        let (tx, ty, _) = transform.translation_component();
        if tx != 0.0 || ty != 0.0 {
            painter.translate(Offset::new(tx, ty));
        }
        draw_fn(painter);
        painter.restore();
    }
}

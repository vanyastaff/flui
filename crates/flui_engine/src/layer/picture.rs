//! Canvas layer - leaf layer with actual drawing commands
//!
//! REFACTORED: Now uses Clean Architecture with CommandRenderer (Visitor Pattern)

use crate::renderer::CommandRenderer;
use flui_interaction::{HitTestResult, HitTestable};
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

/// Hit testing implementation for CanvasLayer
///
/// Checks if a point hits any drawing commands in the canvas by testing
/// against the display list bounds. This is a basic implementation that
/// checks the overall bounds - precise hit testing (e.g., path containment)
/// can be added later.
///
/// # Hit Testing Strategy
///
/// 1. Get bounds from DisplayList (union of all command bounds)
/// 2. Check if position is within bounds
/// 3. If hit, add entry to HitTestResult (no handler by default)
///
/// # Future Enhancements
///
/// - Region-based event handlers attached to specific drawing commands
/// - Path-based hit detection for precise containment checks
/// - Layer-specific event routing (e.g., button clicks, hover)
impl HitTestable for CanvasLayer {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        use flui_interaction::HitTestEntry;
        use flui_types::geometry::Point;

        // Get bounds from DisplayList
        let bounds = self.display_list().bounds();

        // Convert Offset to Point for bounds check
        let point = Point::new(position.dx, position.dy);

        // Check if position is within bounds
        if bounds.contains(point) {
            // Add entry to result (no handler - just marks the hit)
            // In the future, this could include event handlers attached to specific regions
            // Using 0 as temporary element_id (will be replaced with actual element ID system)
            let entry = HitTestEntry::new(0, position, bounds);
            result.add(entry);

            tracing::trace!(
                position = ?position,
                bounds = ?bounds,
                "CanvasLayer hit"
            );

            true
        } else {
            false
        }
    }
}

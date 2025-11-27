//! Canvas layer - leaf layer with actual drawing commands
//!
//! REFACTORED: Now uses Clean Architecture with CommandRenderer (Visitor Pattern)

use crate::renderer::CommandRenderer;
use flui_interaction::{ElementId, HitTestResult, HitTestable};
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

// Manual Debug implementation (Canvas doesn't derive Debug)
impl std::fmt::Debug for CanvasLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasLayer")
            .field("canvas", &"<Canvas>")
            .finish()
    }
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
/// Checks if a point hits any registered hit regions in the display list.
/// Hit regions are added by RenderPointerListener during paint phase.
///
/// # Hit Testing Strategy
///
/// 1. Check hit regions in reverse order (last added = topmost)
/// 2. For each hit, call the registered handler
/// 3. Fall back to bounds check if no regions registered
///
/// This connects GestureDetector callbacks to actual pointer events.
impl HitTestable for CanvasLayer {
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        use flui_interaction::HitTestEntry;
        use flui_types::geometry::Point;

        let point = Point::new(position.dx, position.dy);
        let display_list = self.display_list();
        let hit_regions = display_list.hit_regions();

        // Check hit regions in reverse order (topmost first)
        let mut any_hit = false;
        for region in hit_regions.iter().rev() {
            if region.contains(point) {
                // Create handler that wraps the region's handler
                let region_handler = region.handler.clone();
                let handler: flui_interaction::hit_test::PointerEventHandler =
                    std::sync::Arc::new(move |event| {
                        region_handler(event);
                        flui_interaction::EventPropagation::Stop
                    });

                // Add entry with handler
                // Use placeholder ElementId(1) for canvas-level hit regions
                let entry =
                    HitTestEntry::with_handler(ElementId::new(1), position, region.bounds, handler);
                result.add(entry);

                tracing::trace!(
                    position = ?position,
                    bounds = ?region.bounds,
                    "HitRegion hit - handler registered"
                );

                any_hit = true;
                // Don't break - add all overlapping regions for proper event bubbling
            }
        }

        // Fall back to overall bounds check if no regions
        if !any_hit {
            let bounds = display_list.bounds();
            if bounds.contains(point) {
                // Use placeholder ElementId(1) for canvas-level bounds hit
                let entry = HitTestEntry::new(ElementId::new(1), position, bounds);
                result.add(entry);

                tracing::trace!(
                    position = ?position,
                    bounds = ?bounds,
                    "CanvasLayer bounds hit (no handler)"
                );

                any_hit = true;
            }
        }

        any_hit
    }
}

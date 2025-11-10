//! RenderScrollView - Render object for scrollable widgets
//!
//! Handles layout of scrollable content with scroll offset state.
//! Supports keyboard controls for scrolling.

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_types::layout::Axis;
use flui_types::{BoxConstraints, Offset, Rect, Size};
use parking_lot::Mutex;
use std::sync::Arc;

/// RenderScrollView - handles scrolling of a single child
///
/// Lays out child with infinite constraints in scroll direction,
/// applies scroll offset during paint, and handles scroll events.
#[derive(Debug)]
pub struct RenderScrollView {
    /// Scroll direction (Vertical or Horizontal)
    direction: Axis,

    /// Whether to reverse the scroll direction
    _reverse: bool,

    /// Viewport size (our constrained size)
    viewport_size: Size,

    /// Content size (child's actual size)
    content_size: Size,

    /// Current scroll offset (shared with controller if provided)
    scroll_offset: Arc<Mutex<f32>>,

    /// Max scroll offset (shared with controller if provided)
    max_scroll_offset: Arc<Mutex<f32>>,

    /// Whether to show scroll bars
    show_scrollbar: bool,

    /// Scroll bar thickness in pixels
    scrollbar_thickness: f32,
}

impl RenderScrollView {
    /// Create a new RenderScrollView with internal state
    pub fn new(direction: Axis, reverse: bool) -> Self {
        Self::with_arcs(
            direction,
            reverse,
            Arc::new(Mutex::new(0.0)),
            Arc::new(Mutex::new(0.0)),
        )
    }

    /// Create with external controller arcs (called from SingleChildScrollView)
    pub fn with_controller_arcs(
        direction: Axis,
        reverse: bool,
        offset: Arc<Mutex<f32>>,
        max_offset: Arc<Mutex<f32>>,
    ) -> Self {
        Self::with_arcs(direction, reverse, offset, max_offset)
    }

    /// Internal constructor to avoid duplication
    fn with_arcs(
        direction: Axis,
        reverse: bool,
        offset: Arc<Mutex<f32>>,
        max_offset: Arc<Mutex<f32>>,
    ) -> Self {
        Self {
            direction,
            _reverse: reverse,
            viewport_size: Size::zero(),
            content_size: Size::zero(),
            scroll_offset: offset,
            max_scroll_offset: max_offset,
            show_scrollbar: true,
            scrollbar_thickness: 8.0,
        }
    }

    /// Enable or disable scroll bar visibility
    pub fn set_show_scrollbar(&mut self, show: bool) {
        self.show_scrollbar = show;
    }

    /// Set scroll bar thickness in pixels
    pub fn set_scrollbar_thickness(&mut self, thickness: f32) {
        self.scrollbar_thickness = thickness.max(1.0);
    }

    /// Get current scroll offset
    pub fn get_scroll_offset(&self) -> f32 {
        *self.scroll_offset.lock()
    }

    /// Calculate maximum scroll offset based on content and viewport sizes
    fn calculate_max_scroll(&self) -> f32 {
        match self.direction {
            Axis::Vertical => (self.content_size.height - self.viewport_size.height).max(0.0),
            Axis::Horizontal => (self.content_size.width - self.viewport_size.width).max(0.0),
        }
    }

    /// Calculate child offset with scroll applied
    fn calculate_child_offset(&self, base_offset: Offset) -> Offset {
        let scroll = self.get_scroll_offset();
        match self.direction {
            Axis::Vertical => Offset::new(base_offset.dx, base_offset.dy - scroll),
            Axis::Horizontal => Offset::new(base_offset.dx - scroll, base_offset.dy),
        }
    }

    /// Update max scroll offset after layout
    fn update_max_scroll(&self) {
        let max = self.calculate_max_scroll();
        *self.max_scroll_offset.lock() = max;

        // Clamp current offset if it exceeds new max
        let mut offset = self.scroll_offset.lock();
        if *offset > max {
            *offset = max.max(0.0);
        }
    }

    /// Create scroll event handler callback
    fn create_scroll_handler(&self) -> Arc<dyn Fn(f32, f32) + Send + Sync> {
        let offset = Arc::clone(&self.scroll_offset);
        let max_offset = Arc::clone(&self.max_scroll_offset);
        let direction = self.direction;

        Arc::new(move |dx: f32, dy: f32| {
            // Select delta based on scroll direction
            let delta = match direction {
                Axis::Vertical => -dy,   // Negative: scroll down = positive delta
                Axis::Horizontal => -dx, // Negative: scroll right = positive delta
            };

            // Update offset with bounds checking
            let mut current = offset.lock();
            let max = *max_offset.lock();
            *current = (*current + delta).clamp(0.0, max);

            #[cfg(debug_assertions)]
            tracing::debug!("Scroll event: delta={:.1}, offset={:.1}", delta, *current);
        })
    }

    // TODO: Implement paint_scrollbar() using Canvas API when needed
    // The scrollbar should be drawn directly to canvas using draw_rect()
}

impl Render for RenderScrollView {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Create constraints for child - infinite in scroll direction
        let child_constraints = match self.direction {
            Axis::Vertical => BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                0.0,
                f32::INFINITY, // Infinite height for vertical scroll
            ),
            Axis::Horizontal => BoxConstraints::new(
                0.0,
                f32::INFINITY, // Infinite width for horizontal scroll
                constraints.min_height,
                constraints.max_height,
            ),
        };

        // Layout child with infinite constraint
        let child_size = tree.layout_child(child_id, child_constraints);

        // Store content size for scroll calculations
        self.content_size = child_size;

        // Our size is constrained by viewport
        self.viewport_size = constraints.constrain(child_size);

        // Update max scroll in controller
        self.update_max_scroll();

        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderScrollView::layout: direction={:?}, content_size={:?}, viewport_size={:?}, max_scroll={:.1}",
            self.direction,
            self.content_size,
            self.viewport_size,
            self.calculate_max_scroll()
        );

        self.viewport_size
    }

    fn paint(&self, ctx: &PaintContext) -> flui_painting::Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Calculate child offset with scroll applied
        let child_offset = self.calculate_child_offset(offset);

        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderScrollView::paint: scroll_offset={:.1}, offset={:?}, child_offset={:?}, viewport={:?}, content={:?}",
            self.get_scroll_offset(),
            offset,
            child_offset,
            self.viewport_size,
            self.content_size
        );

        // Create canvas with clipping
        let mut canvas = flui_painting::Canvas::new();

        // Apply viewport clipping
        let clip_rect = Rect::from_min_size(offset, self.viewport_size);
        canvas.save();
        canvas.clip_rect(clip_rect);

        // Paint child with scroll offset applied
        let child_canvas = tree.paint_child(child_id, child_offset);
        canvas.append_canvas(child_canvas);

        canvas.restore();

        // TODO: Integrate scrollbar painting via Canvas API
        // TODO: Integrate ScrollableLayer event handling (needs event system refactor)
        // For now, scrolling events will need to be handled differently
        // The old ScrollableLayer approach doesn't work with Canvas-only architecture

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_scroll_view_new() {
        let render = RenderScrollView::new(Axis::Vertical, false);
        assert_eq!(render.direction, Axis::Vertical);
    }
}

//! RenderViewport - Render object for viewport widgets
//!
//! A viewport shows a subset of large content through a fixed-size window.
//! It applies an offset to its child to show different portions of the content.

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_types::layout::Axis;
use flui_types::{BoxConstraints, Offset, Rect, Size};

/// RenderViewport - displays a slice of content through a fixed viewport
///
/// The viewport has a fixed size and shows a portion of its child based on
/// the scroll offset. The child can be larger than the viewport.
#[derive(Debug)]
pub struct RenderViewport {
    /// Scroll direction (Vertical or Horizontal)
    axis: Axis,

    /// Current viewport offset (how far scrolled)
    offset: Offset,

    /// Viewport size (our constrained size)
    viewport_size: Size,

    /// Whether to clip content outside viewport
    clip: bool,
}

impl RenderViewport {
    /// Create a new viewport
    pub fn new(axis: Axis, offset: Offset) -> Self {
        Self {
            axis,
            offset,
            viewport_size: Size::zero(),
            clip: true,
        }
    }

    /// Update the viewport offset
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Get the current viewport offset
    pub fn get_offset(&self) -> Offset {
        self.offset
    }

    /// Set whether to clip content outside viewport
    pub fn set_clip(&mut self, clip: bool) {
        self.clip = clip;
    }

    /// Calculate child constraints based on viewport axis
    ///
    /// Child is given infinite constraints in scroll direction,
    /// but constrained in cross-axis direction.
    fn calculate_child_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        match self.axis {
            Axis::Vertical => BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                0.0,
                f32::INFINITY, // Infinite height
            ),
            Axis::Horizontal => BoxConstraints::new(
                0.0,
                f32::INFINITY, // Infinite width
                constraints.min_height,
                constraints.max_height,
            ),
        }
    }

    /// Calculate the offset to apply to child during painting
    fn calculate_paint_offset(&self) -> Offset {
        match self.axis {
            Axis::Vertical => Offset::new(0.0, -self.offset.dy),
            Axis::Horizontal => Offset::new(-self.offset.dx, 0.0),
        }
    }
}

impl Render for RenderViewport {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Calculate child constraints (infinite in scroll direction)
        let child_constraints = self.calculate_child_constraints(constraints);

        // Layout child with infinite constraint
        let _child_size = tree.layout_child(child_id, child_constraints);

        // Our size is the viewport size (constrained by parent)
        self.viewport_size = constraints.biggest();

        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderViewport::layout: axis={:?}, viewport_size={:?}, child_size={:?}, offset={:?}",
            self.axis,
            self.viewport_size,
            _child_size,
            self.offset
        );

        self.viewport_size
    }

    fn paint(&self, ctx: &PaintContext) -> flui_painting::Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Calculate paint offset for child
        let paint_offset = self.calculate_paint_offset();
        let child_offset = offset + paint_offset;

        #[cfg(debug_assertions)]
        tracing::trace!(
            "RenderViewport::paint: offset={:?}, paint_offset={:?}, child_offset={:?}",
            offset,
            paint_offset,
            child_offset
        );

        // Create canvas and apply clipping if needed
        let mut canvas = flui_painting::Canvas::new();

        if self.clip {
            // Apply clipping to viewport bounds
            let clip_rect = Rect::from_min_size(offset, self.viewport_size);
            canvas.save();
            canvas.clip_rect(clip_rect);
        }

        // Paint child
        let child_canvas = tree.paint_child(child_id, child_offset);
        canvas.append_canvas(child_canvas);

        if self.clip {
            canvas.restore();
        }

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
    fn test_render_viewport_new() {
        let viewport = RenderViewport::new(Axis::Vertical, Offset::ZERO);
        assert_eq!(viewport.axis, Axis::Vertical);
        assert_eq!(viewport.get_offset(), Offset::ZERO);
        assert!(viewport.clip);
    }

    #[test]
    fn test_set_offset() {
        let mut viewport = RenderViewport::new(Axis::Vertical, Offset::ZERO);
        viewport.set_offset(Offset::new(0.0, 100.0));
        assert_eq!(viewport.get_offset(), Offset::new(0.0, 100.0));
    }

    #[test]
    fn test_calculate_child_constraints_vertical() {
        let viewport = RenderViewport::new(Axis::Vertical, Offset::ZERO);
        let constraints = BoxConstraints::new(100.0, 200.0, 50.0, 150.0);
        let child_constraints = viewport.calculate_child_constraints(constraints);

        assert_eq!(child_constraints.min_width, 100.0);
        assert_eq!(child_constraints.max_width, 200.0);
        assert_eq!(child_constraints.min_height, 0.0);
        assert_eq!(child_constraints.max_height, f32::INFINITY);
    }

    #[test]
    fn test_calculate_child_constraints_horizontal() {
        let viewport = RenderViewport::new(Axis::Horizontal, Offset::ZERO);
        let constraints = BoxConstraints::new(100.0, 200.0, 50.0, 150.0);
        let child_constraints = viewport.calculate_child_constraints(constraints);

        assert_eq!(child_constraints.min_width, 0.0);
        assert_eq!(child_constraints.max_width, f32::INFINITY);
        assert_eq!(child_constraints.min_height, 50.0);
        assert_eq!(child_constraints.max_height, 150.0);
    }

    #[test]
    fn test_calculate_paint_offset_vertical() {
        let viewport = RenderViewport::new(Axis::Vertical, Offset::new(0.0, 50.0));
        let paint_offset = viewport.calculate_paint_offset();
        assert_eq!(paint_offset, Offset::new(0.0, -50.0));
    }

    #[test]
    fn test_calculate_paint_offset_horizontal() {
        let viewport = RenderViewport::new(Axis::Horizontal, Offset::new(75.0, 0.0));
        let paint_offset = viewport.calculate_paint_offset();
        assert_eq!(paint_offset, Offset::new(-75.0, 0.0));
    }
}

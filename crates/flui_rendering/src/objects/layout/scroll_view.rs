//! RenderScrollView - Box-based scrollable render object
//!
//! This render object provides simple box-based scrolling for a single child.
//! Used by SingleChildScrollView for straightforward scroll scenarios without slivers.
//!
//! Flutter reference: <https://api.flutter.dev/flutter/widgets/SingleChildScrollView-class.html>

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::layout::Axis;
use flui_types::painting::Paint;
use flui_types::prelude::*;
use parking_lot::Mutex;
use std::sync::Arc;

/// RenderObject that scrolls a single box child
///
/// Provides simple offset-based scrolling without the complexity of slivers.
/// Suitable for SingleChildScrollView where the entire child is laid out
/// and then scrolled as a whole.
///
/// # Architecture
///
/// ```text
/// Parent Box Constraints
///        ↓
///   RenderScrollView
///        ↓
/// Child (laid out with relaxed constraints)
///        ↓
/// Paint with offset based on scroll position
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderScrollView;
/// use flui_types::layout::Axis;
///
/// // Vertical scrolling
/// let scroll_view = RenderScrollView::new(Axis::Vertical, false);
/// ```
#[derive(Debug)]
pub struct RenderScrollView {
    /// Scroll direction
    pub axis: Axis,

    /// Whether to reverse the scroll direction
    pub reverse: bool,

    /// Current scroll offset (shared with ScrollController if present)
    scroll_offset: Arc<Mutex<f32>>,

    /// Maximum scroll offset (shared with ScrollController if present)
    max_scroll_offset: Arc<Mutex<f32>>,

    /// Whether to show scrollbar
    show_scrollbar: bool,

    /// Scrollbar thickness
    scrollbar_thickness: f32,

    // Layout cache
    child_size: Size,
    viewport_size: Size,
}

impl RenderScrollView {
    /// Create new scroll view
    pub fn new(axis: Axis, reverse: bool) -> Self {
        Self {
            axis,
            reverse,
            scroll_offset: Arc::new(Mutex::new(0.0)),
            max_scroll_offset: Arc::new(Mutex::new(0.0)),
            show_scrollbar: true,
            scrollbar_thickness: 8.0,
            child_size: Size::ZERO,
            viewport_size: Size::ZERO,
        }
    }

    /// Create scroll view with shared Arc references (for ScrollController)
    pub fn with_controller_arcs(
        axis: Axis,
        reverse: bool,
        scroll_offset: Arc<Mutex<f32>>,
        max_scroll_offset: Arc<Mutex<f32>>,
    ) -> Self {
        Self {
            axis,
            reverse,
            scroll_offset,
            max_scroll_offset,
            show_scrollbar: true,
            scrollbar_thickness: 8.0,
            child_size: Size::ZERO,
            viewport_size: Size::ZERO,
        }
    }

    /// Set scroll offset
    pub fn set_scroll_offset(&mut self, offset: f32) {
        *self.scroll_offset.lock() = offset.max(0.0).min(*self.max_scroll_offset.lock());
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> f32 {
        *self.scroll_offset.lock()
    }

    /// Get maximum scroll offset
    pub fn max_scroll_offset(&self) -> f32 {
        *self.max_scroll_offset.lock()
    }

    /// Set whether to show scrollbar
    pub fn set_show_scrollbar(&mut self, show: bool) {
        self.show_scrollbar = show;
    }

    /// Set scrollbar thickness
    pub fn set_scrollbar_thickness(&mut self, thickness: f32) {
        self.scrollbar_thickness = thickness;
    }

    /// Calculate child constraints based on viewport constraints
    fn child_constraints(&self, viewport_constraints: &BoxConstraints) -> BoxConstraints {
        match self.axis {
            Axis::Vertical => {
                // Child can be as tall as needed (relax height)
                BoxConstraints {
                    min_width: viewport_constraints.min_width,
                    max_width: viewport_constraints.max_width,
                    min_height: 0.0,
                    max_height: f32::INFINITY, // Relax height for vertical scroll
                }
            }
            Axis::Horizontal => {
                // Child can be as wide as needed (relax width)
                BoxConstraints {
                    min_width: 0.0,
                    max_width: f32::INFINITY, // Relax width for horizontal scroll
                    min_height: viewport_constraints.min_height,
                    max_height: viewport_constraints.max_height,
                }
            }
        }
    }

    /// Calculate scroll offset based on child and viewport sizes
    fn calculate_max_scroll_offset(&self) -> f32 {
        match self.axis {
            Axis::Vertical => (self.child_size.height - self.viewport_size.height).max(0.0),
            Axis::Horizontal => (self.child_size.width - self.viewport_size.width).max(0.0),
        }
    }

    /// Calculate paint offset based on scroll offset and axis
    fn paint_offset(&self) -> Offset {
        let offset = *self.scroll_offset.lock();

        let offset = if self.reverse { -offset } else { offset };

        match self.axis {
            Axis::Vertical => Offset::new(0.0, -offset),
            Axis::Horizontal => Offset::new(-offset, 0.0),
        }
    }

    /// Paint scrollbar indicator on canvas
    fn paint_scrollbar_on_canvas(&self, canvas: &mut flui_painting::Canvas) {
        let max_offset = self.max_scroll_offset();
        if max_offset <= 0.0 {
            return;
        }

        let current_offset = *self.scroll_offset.lock();
        let track_paint = Paint::fill(Color::rgba(0, 0, 0, 25)); // ~0.1 alpha
        let handle_paint = Paint::fill(Color::rgba(0, 0, 0, 128)); // ~0.5 alpha

        match self.axis {
            Axis::Vertical => {
                // Vertical scrollbar on right edge
                let viewport_height = self.viewport_size.height;
                let content_height = self.child_size.height;

                // Scrollbar handle size proportional to visible ratio
                let handle_height = (viewport_height / content_height * viewport_height).max(20.0);

                // Scrollbar position based on scroll offset
                let scroll_ratio = current_offset / max_offset;
                let handle_offset = scroll_ratio * (viewport_height - handle_height);

                // Draw scrollbar track and handle using pill shape (rounded ends)
                let track_rect = Rect::from_xywh(
                    self.viewport_size.width - self.scrollbar_thickness,
                    0.0,
                    self.scrollbar_thickness,
                    viewport_height,
                );
                let handle_rect = Rect::from_xywh(
                    self.viewport_size.width - self.scrollbar_thickness,
                    handle_offset,
                    self.scrollbar_thickness,
                    handle_height,
                );

                canvas
                    .rect(track_rect, &track_paint)
                    .draw_pill(handle_rect, &handle_paint);
            }
            Axis::Horizontal => {
                // Horizontal scrollbar on bottom edge
                let viewport_width = self.viewport_size.width;
                let content_width = self.child_size.width;

                // Scrollbar handle size proportional to visible ratio
                let handle_width = (viewport_width / content_width * viewport_width).max(20.0);

                // Scrollbar position based on scroll offset
                let scroll_ratio = current_offset / max_offset;
                let handle_offset = scroll_ratio * (viewport_width - handle_width);

                // Draw scrollbar track and handle using pill shape (rounded ends)
                let track_rect = Rect::from_xywh(
                    0.0,
                    self.viewport_size.height - self.scrollbar_thickness,
                    viewport_width,
                    self.scrollbar_thickness,
                );
                let handle_rect = Rect::from_xywh(
                    handle_offset,
                    self.viewport_size.height - self.scrollbar_thickness,
                    handle_width,
                    self.scrollbar_thickness,
                );

                canvas
                    .rect(track_rect, &track_paint)
                    .draw_pill(handle_rect, &handle_paint);
            }
        }
    }
}

impl Default for RenderScrollView {
    fn default() -> Self {
        Self::new(Axis::Vertical, false)
    }
}

impl RenderBox<Single> for RenderScrollView {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let viewport_constraints = &ctx.constraints;

        // Store viewport size
        self.viewport_size = Size::new(
            viewport_constraints.max_width,
            viewport_constraints.max_height,
        );

        // Layout child with relaxed constraints
        let child_id = ctx.children.single();
        let child_constraints = self.child_constraints(viewport_constraints);
        self.child_size = ctx.layout_child(child_id, child_constraints);

        // Calculate and update max scroll offset
        let max_offset = self.calculate_max_scroll_offset();
        *self.max_scroll_offset.lock() = max_offset;

        // Clamp current scroll offset to valid range
        let current_offset = *self.scroll_offset.lock();
        *self.scroll_offset.lock() = current_offset.max(0.0).min(max_offset);

        // Return viewport size (not child size!)
        self.viewport_size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        let paint_offset = ctx.offset + self.paint_offset();

        // Apply clipping to viewport bounds with chaining API
        ctx.canvas()
            .saved()
            .clipped_rect(Rect::from_min_size(Point::ZERO, self.viewport_size));

        // Paint child at scrolled position
        ctx.paint_child(child_id, paint_offset);

        ctx.canvas().restored();

        // Paint scrollbar if enabled
        if self.show_scrollbar && self.max_scroll_offset() > 0.0 {
            self.paint_scrollbar_on_canvas(ctx.canvas());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_scroll_view_new() {
        let scroll_view = RenderScrollView::new(Axis::Vertical, false);

        assert_eq!(scroll_view.axis, Axis::Vertical);
        assert!(!scroll_view.reverse);
        assert_eq!(scroll_view.scroll_offset(), 0.0);
    }

    #[test]
    fn test_render_scroll_view_horizontal() {
        let scroll_view = RenderScrollView::new(Axis::Horizontal, false);

        assert_eq!(scroll_view.axis, Axis::Horizontal);
    }

    #[test]
    fn test_set_scroll_offset() {
        let mut scroll_view = RenderScrollView::new(Axis::Vertical, false);

        // Set max offset first
        *scroll_view.max_scroll_offset.lock() = 100.0;

        scroll_view.set_scroll_offset(50.0);
        assert_eq!(scroll_view.scroll_offset(), 50.0);

        // Should clamp to max
        scroll_view.set_scroll_offset(150.0);
        assert_eq!(scroll_view.scroll_offset(), 100.0);

        // Should clamp to min
        scroll_view.set_scroll_offset(-10.0);
        assert_eq!(scroll_view.scroll_offset(), 0.0);
    }

    #[test]
    fn test_child_constraints_vertical() {
        let scroll_view = RenderScrollView::new(Axis::Vertical, false);

        let viewport_constraints = BoxConstraints::new(100.0, 400.0, 50.0, 600.0);
        let child_constraints = scroll_view.child_constraints(&viewport_constraints);

        // Width preserved
        assert_eq!(child_constraints.min_width, 100.0);
        assert_eq!(child_constraints.max_width, 400.0);

        // Height relaxed for scrolling
        assert_eq!(child_constraints.min_height, 0.0);
        assert!(child_constraints.max_height.is_infinite());
    }

    #[test]
    fn test_child_constraints_horizontal() {
        let scroll_view = RenderScrollView::new(Axis::Horizontal, false);

        let viewport_constraints = BoxConstraints::new(100.0, 400.0, 50.0, 600.0);
        let child_constraints = scroll_view.child_constraints(&viewport_constraints);

        // Width relaxed for scrolling
        assert_eq!(child_constraints.min_width, 0.0);
        assert!(child_constraints.max_width.is_infinite());

        // Height preserved
        assert_eq!(child_constraints.min_height, 50.0);
        assert_eq!(child_constraints.max_height, 600.0);
    }

    #[test]
    fn test_calculate_max_scroll_offset_vertical() {
        let mut scroll_view = RenderScrollView::new(Axis::Vertical, false);

        scroll_view.viewport_size = Size::new(400.0, 600.0);
        scroll_view.child_size = Size::new(400.0, 1000.0);

        let max_offset = scroll_view.calculate_max_scroll_offset();
        assert_eq!(max_offset, 400.0); // 1000 - 600
    }

    #[test]
    fn test_calculate_max_scroll_offset_horizontal() {
        let mut scroll_view = RenderScrollView::new(Axis::Horizontal, false);

        scroll_view.viewport_size = Size::new(400.0, 600.0);
        scroll_view.child_size = Size::new(800.0, 600.0);

        let max_offset = scroll_view.calculate_max_scroll_offset();
        assert_eq!(max_offset, 400.0); // 800 - 400
    }

    #[test]
    fn test_calculate_max_scroll_offset_no_scroll_needed() {
        let mut scroll_view = RenderScrollView::new(Axis::Vertical, false);

        scroll_view.viewport_size = Size::new(400.0, 600.0);
        scroll_view.child_size = Size::new(400.0, 500.0); // Smaller than viewport

        let max_offset = scroll_view.calculate_max_scroll_offset();
        assert_eq!(max_offset, 0.0); // No scrolling needed
    }

    #[test]
    fn test_paint_offset_vertical() {
        let scroll_view = RenderScrollView::new(Axis::Vertical, false);

        *scroll_view.scroll_offset.lock() = 100.0;

        let offset = scroll_view.paint_offset();
        assert_eq!(offset, Offset::new(0.0, -100.0)); // Negative to scroll down
    }

    #[test]
    fn test_paint_offset_horizontal() {
        let scroll_view = RenderScrollView::new(Axis::Horizontal, false);

        *scroll_view.scroll_offset.lock() = 50.0;

        let offset = scroll_view.paint_offset();
        assert_eq!(offset, Offset::new(-50.0, 0.0)); // Negative to scroll right
    }

    #[test]
    fn test_paint_offset_reversed() {
        let scroll_view = RenderScrollView::new(Axis::Vertical, true);

        *scroll_view.scroll_offset.lock() = 100.0;

        let offset = scroll_view.paint_offset();
        assert_eq!(offset, Offset::new(0.0, 100.0)); // Positive when reversed
    }
}

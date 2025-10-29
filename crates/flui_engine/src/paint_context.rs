//! Paint context for rendering
//!
//! This module provides the PaintContext that is passed to RenderObjects during painting.
//! It provides access to the painter and additional rendering state.

use crate::painter::Painter;
use flui_types::{Offset, Rect};

/// Context provided to RenderObjects during paint phase
///
/// The PaintContext gives RenderObjects access to the painter and provides
/// helper methods for common painting operations.
///
/// # Example
///
/// ```rust,ignore
/// impl RenderObject for MyRenderObject {
///     fn paint(&self, context: &mut PaintContext, offset: Offset) {
///         let rect = Rect::from_ltwh(
///             offset.dx,
///             offset.dy,
///             self.size.width,
///             self.size.height,
///         );
///
///         let paint = Paint {
///             color: [1.0, 0.0, 0.0, 1.0], // Red
///             ..Default::default()
///         };
///
///         context.painter().rect(rect, &paint);
///     }
/// }
/// ```
pub struct PaintContext<'a> {
    /// The painter to draw with
    painter: &'a mut dyn Painter,

    /// Current canvas bounds (for culling)
    canvas_bounds: Rect,

    /// Whether to paint debug information
    debug_paint: bool,
}

impl<'a> PaintContext<'a> {
    /// Create a new paint context
    ///
    /// # Arguments
    /// * `painter` - The painter to use for drawing
    /// * `canvas_bounds` - The visible canvas bounds (for culling)
    pub fn new(painter: &'a mut dyn Painter, canvas_bounds: Rect) -> Self {
        Self {
            painter,
            canvas_bounds,
            debug_paint: false,
        }
    }

    /// Get access to the painter
    ///
    /// This is the primary way RenderObjects draw. The painter provides
    /// backend-agnostic drawing primitives.
    pub fn painter(&mut self) -> &mut dyn Painter {
        self.painter
    }

    /// Get the canvas bounds
    ///
    /// RenderObjects can use this for culling - avoiding painting content
    /// that is completely outside the visible area.
    pub fn canvas_bounds(&self) -> Rect {
        self.canvas_bounds
    }

    /// Check if a rect is visible (intersects with canvas bounds)
    ///
    /// Use this to avoid painting content that is completely off-screen.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn paint(&self, context: &mut PaintContext, offset: Offset) {
    ///     let rect = self.get_bounds(offset);
    ///
    ///     if !context.is_visible(rect) {
    ///         return; // Skip painting - not visible
    ///     }
    ///
    ///     // Paint the content...
    /// }
    /// ```
    pub fn is_visible(&self, rect: Rect) -> bool {
        self.canvas_bounds.intersects(&rect)
    }

    /// Enable debug painting mode
    ///
    /// When enabled, RenderObjects should draw additional debug information
    /// like bounds, baselines, etc.
    pub fn enable_debug_paint(&mut self) {
        self.debug_paint = true;
    }

    /// Disable debug painting mode
    pub fn disable_debug_paint(&mut self) {
        self.debug_paint = false;
    }

    /// Check if debug painting is enabled
    pub fn debug_paint_enabled(&self) -> bool {
        self.debug_paint
    }

    /// Paint with a translation offset
    ///
    /// Saves the transform, translates, calls the closure, then restores.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// context.paint_with_offset(Offset::new(10.0, 20.0), |ctx| {
    ///     // Paint at (10, 20) offset from current origin
    ///     ctx.painter().rect(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0), &paint);
    /// });
    /// ```
    pub fn paint_with_offset<F>(&mut self, offset: Offset, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.painter.save();
        self.painter.translate(offset);
        f(self);
        self.painter.restore();
    }

    /// Paint with a rotation
    ///
    /// Saves the transform, rotates, calls the closure, then restores.
    ///
    /// # Arguments
    /// * `angle` - Rotation angle in radians
    /// * `f` - Closure to call with rotated coordinate system
    pub fn paint_with_rotation<F>(&mut self, angle: f32, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.painter.save();
        self.painter.rotate(angle);
        f(self);
        self.painter.restore();
    }

    /// Paint with a scale
    ///
    /// Saves the transform, scales, calls the closure, then restores.
    ///
    /// # Arguments
    /// * `sx` - Horizontal scale factor
    /// * `sy` - Vertical scale factor
    /// * `f` - Closure to call with scaled coordinate system
    pub fn paint_with_scale<F>(&mut self, sx: f32, sy: f32, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.painter.save();
        self.painter.scale(sx, sy);
        f(self);
        self.painter.restore();
    }

    /// Paint with opacity
    ///
    /// Saves opacity, sets new opacity, calls closure, then restores.
    ///
    /// # Arguments
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque)
    /// * `f` - Closure to call with modified opacity
    pub fn paint_with_opacity<F>(&mut self, opacity: f32, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.painter.save();
        self.painter.set_opacity(opacity);
        f(self);
        self.painter.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::painter::{Paint, Painter};
    use flui_types::Point;

    struct MockPainter {
        save_count: usize,
        restore_count: usize,
    }

    impl MockPainter {
        fn new() -> Self {
            Self {
                save_count: 0,
                restore_count: 0,
            }
        }
    }

    impl Painter for MockPainter {
        fn rect(&mut self, _rect: Rect, _paint: &Paint) {}
        fn rrect(&mut self, _rrect: crate::painter::RRect, _paint: &Paint) {}
        fn circle(&mut self, _center: Point, _radius: f32, _paint: &Paint) {}
        fn line(&mut self, _p1: Point, _p2: Point, _paint: &Paint) {}

        fn save(&mut self) {
            self.save_count += 1;
        }

        fn restore(&mut self) {
            self.restore_count += 1;
        }

        fn translate(&mut self, _offset: Offset) {}
        fn rotate(&mut self, _angle: f32) {}
        fn scale(&mut self, _sx: f32, _sy: f32) {}
        fn clip_rect(&mut self, _rect: Rect) {}
        fn clip_rrect(&mut self, _rrect: crate::painter::RRect) {}
        fn set_opacity(&mut self, _opacity: f32) {}
    }

    #[test]
    fn test_paint_context_creation() {
        let mut painter = MockPainter::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let context = PaintContext::new(&mut painter, bounds);

        assert_eq!(context.canvas_bounds(), bounds);
        assert!(!context.debug_paint_enabled());
    }

    #[test]
    fn test_visibility_check() {
        let mut painter = MockPainter::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let context = PaintContext::new(&mut painter, bounds);

        // Inside canvas
        assert!(context.is_visible(Rect::from_ltwh(100.0, 100.0, 200.0, 200.0)));

        // Outside canvas
        assert!(!context.is_visible(Rect::from_ltwh(1000.0, 1000.0, 200.0, 200.0)));

        // Partially visible
        assert!(context.is_visible(Rect::from_ltwh(700.0, 500.0, 200.0, 200.0)));
    }

    #[test]
    fn test_debug_paint_toggle() {
        let mut painter = MockPainter::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let mut context = PaintContext::new(&mut painter, bounds);

        assert!(!context.debug_paint_enabled());

        context.enable_debug_paint();
        assert!(context.debug_paint_enabled());

        context.disable_debug_paint();
        assert!(!context.debug_paint_enabled());
    }

    #[test]
    fn test_paint_with_offset() {
        let mut painter = MockPainter::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let mut context = PaintContext::new(&mut painter, bounds);

        context.paint_with_offset(Offset::new(10.0, 20.0), |_ctx| {
            // Painting happens here
        });

        assert_eq!(painter.save_count, 1);
        assert_eq!(painter.restore_count, 1);
    }

    #[test]
    fn test_paint_with_rotation() {
        let mut painter = MockPainter::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let mut context = PaintContext::new(&mut painter, bounds);

        context.paint_with_rotation(1.57, |_ctx| {
            // Painting happens here
        });

        assert_eq!(painter.save_count, 1);
        assert_eq!(painter.restore_count, 1);
    }

    #[test]
    fn test_paint_with_scale() {
        let mut painter = MockPainter::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let mut context = PaintContext::new(&mut painter, bounds);

        context.paint_with_scale(2.0, 2.0, |_ctx| {
            // Painting happens here
        });

        assert_eq!(painter.save_count, 1);
        assert_eq!(painter.restore_count, 1);
    }

    #[test]
    fn test_paint_with_opacity() {
        let mut painter = MockPainter::new();
        let bounds = Rect::from_ltwh(0.0, 0.0, 800.0, 600.0);
        let mut context = PaintContext::new(&mut painter, bounds);

        context.paint_with_opacity(0.5, |_ctx| {
            // Painting happens here
        });

        assert_eq!(painter.save_count, 1);
        assert_eq!(painter.restore_count, 1);
    }
}

//! RenderSliverOpacity - Applies opacity to sliver content

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::prelude::*;

/// RenderObject that applies opacity to a sliver child
///
/// Similar to RenderOpacity but for slivers. This allows fading in/out
/// sliver content (lists, grids, etc.) without affecting their layout.
///
/// # Performance
///
/// - Opacity = 0.0: Child is not painted (optimization)
/// - Opacity = 1.0: Child painted normally (no layer)
/// - 0.0 < Opacity < 1.0: Uses compositing layer
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverOpacity;
///
/// // 50% transparent sliver
/// let opacity = RenderSliverOpacity::new(0.5);
/// ```
#[derive(Debug)]
pub struct RenderSliverOpacity {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
    /// Whether to always include the child in the tree even when invisible
    pub always_include_semantics: bool,

    // Layout cache
    child_size: Size,
}

impl RenderSliverOpacity {
    /// Create new sliver opacity
    ///
    /// # Arguments
    /// * `opacity` - Opacity value between 0.0 and 1.0
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
            child_size: Size::ZERO,
        }
    }

    /// Set opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Set whether to always include semantics
    pub fn set_always_include_semantics(&mut self, always: bool) {
        self.always_include_semantics = always;
    }

    /// Create with semantics always included
    pub fn with_always_include_semantics(mut self) -> Self {
        self.always_include_semantics = true;
        self
    }

    /// Check if the child should be painted
    pub fn should_paint(&self) -> bool {
        self.opacity > 0.0
    }

    /// Check if we need a compositing layer
    pub fn needs_compositing(&self) -> bool {
        self.opacity > 0.0 && self.opacity < 1.0
    }
}

impl Render for RenderSliverOpacity {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;

        // Opacity doesn't affect layout, pass through to child
        // In real implementation, child would be laid out here
        self.child_size = Size::new(
            constraints.max_width,
            constraints.max_height,
        );

        self.child_size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let _offset = ctx.offset;
        let canvas = Canvas::new();

        // If fully transparent, skip painting (unless semantics required)
        if !self.should_paint() && !self.always_include_semantics {
            return canvas;
        }

        // TODO: Implement actual opacity painting
        // If opacity == 1.0: paint child directly
        // If 0.0 < opacity < 1.0: use compositing layer with alpha
        // canvas.save_layer_alpha(self.opacity);
        // canvas.paint_child(child);
        // canvas.restore();

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_opacity_new() {
        let opacity = RenderSliverOpacity::new(0.5);

        assert_eq!(opacity.opacity, 0.5);
        assert!(!opacity.always_include_semantics);
    }

    #[test]
    fn test_render_sliver_opacity_clamps() {
        let opacity_low = RenderSliverOpacity::new(-0.5);
        let opacity_high = RenderSliverOpacity::new(1.5);

        assert_eq!(opacity_low.opacity, 0.0);
        assert_eq!(opacity_high.opacity, 1.0);
    }

    #[test]
    fn test_set_opacity() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        opacity.set_opacity(0.8);

        assert_eq!(opacity.opacity, 0.8);
    }

    #[test]
    fn test_set_opacity_clamps() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        opacity.set_opacity(2.0);

        assert_eq!(opacity.opacity, 1.0);

        opacity.set_opacity(-1.0);
        assert_eq!(opacity.opacity, 0.0);
    }

    #[test]
    fn test_set_always_include_semantics() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        opacity.set_always_include_semantics(true);

        assert!(opacity.always_include_semantics);
    }

    #[test]
    fn test_with_always_include_semantics() {
        let opacity = RenderSliverOpacity::new(0.5).with_always_include_semantics();

        assert!(opacity.always_include_semantics);
    }

    #[test]
    fn test_should_paint_transparent() {
        let opacity = RenderSliverOpacity::new(0.0);

        assert!(!opacity.should_paint());
    }

    #[test]
    fn test_should_paint_visible() {
        let opacity = RenderSliverOpacity::new(0.5);

        assert!(opacity.should_paint());
    }

    #[test]
    fn test_should_paint_opaque() {
        let opacity = RenderSliverOpacity::new(1.0);

        assert!(opacity.should_paint());
    }

    #[test]
    fn test_needs_compositing_transparent() {
        let opacity = RenderSliverOpacity::new(0.0);

        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_partial() {
        let opacity = RenderSliverOpacity::new(0.5);

        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_opaque() {
        let opacity = RenderSliverOpacity::new(1.0);

        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_opacity_zero() {
        let opacity = RenderSliverOpacity::new(0.0);

        assert_eq!(opacity.opacity, 0.0);
        assert!(!opacity.should_paint());
        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_opacity_half() {
        let opacity = RenderSliverOpacity::new(0.5);

        assert_eq!(opacity.opacity, 0.5);
        assert!(opacity.should_paint());
        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_opacity_one() {
        let opacity = RenderSliverOpacity::new(1.0);

        assert_eq!(opacity.opacity, 1.0);
        assert!(opacity.should_paint());
        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_arity_is_single_child() {
        let opacity = RenderSliverOpacity::new(0.5);
        assert_eq!(opacity.arity(), Arity::Exact(1));
    }
}

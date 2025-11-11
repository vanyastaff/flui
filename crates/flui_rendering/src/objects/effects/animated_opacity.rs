//! RenderAnimatedOpacity - animated opacity transitions

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;

use flui_types::Size;

/// RenderObject that applies animated opacity to its child
///
/// Similar to RenderOpacity, but designed for animated transitions.
/// The animating flag can be used to trigger repaint boundaries.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAnimatedOpacity;
///
/// // Create with 50% opacity, animating
/// let animated = RenderAnimatedOpacity::animating_to(0.5);
/// ```
#[derive(Debug)]
pub struct RenderAnimatedOpacity {
    /// Current opacity value (0.0 = transparent, 1.0 = opaque)
    pub opacity: f32,
    /// Whether the animation is currently running
    pub animating: bool,
}

impl RenderAnimatedOpacity {
    /// Create new animated opacity
    pub fn new(opacity: f32, animating: bool) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            animating,
        }
    }

    /// Create with opacity 1.0 (fully opaque)
    pub fn opaque() -> Self {
        Self::new(1.0, false)
    }

    /// Create with opacity 0.0 (fully transparent)
    pub fn transparent() -> Self {
        Self::new(0.0, false)
    }

    /// Create animating to target opacity
    pub fn animating_to(opacity: f32) -> Self {
        Self::new(opacity, true)
    }

    /// Set opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Set animating flag
    pub fn set_animating(&mut self, animating: bool) {
        self.animating = animating;
    }
}

impl Default for RenderAnimatedOpacity {
    fn default() -> Self {
        Self::opaque()
    }
}

impl Render for RenderAnimatedOpacity {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // Skip painting if fully transparent
        if self.opacity <= 0.0 {
            return Canvas::new();
        }

        // Paint child
        // TODO: Implement Canvas opacity support
        // For now, we just return the child canvas directly
        // Proper implementation would apply opacity to all drawing commands
        // This requires either:
        // 1. Adding saveLayer() support to Canvas
        // 2. Adding opacity field to DisplayList
        // 3. Converting Canvas to PictureLayer and wrapping in OpacityLayer
        tree.paint_child(child_id, offset)
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
    fn test_animated_opacity_new() {
        let opacity = RenderAnimatedOpacity::new(0.5, true);
        assert_eq!(opacity.opacity, 0.5);
        assert!(opacity.animating);
    }

    #[test]
    fn test_animated_opacity_opaque() {
        let opacity = RenderAnimatedOpacity::opaque();
        assert_eq!(opacity.opacity, 1.0);
        assert!(!opacity.animating);
    }

    #[test]
    fn test_animated_opacity_transparent() {
        let opacity = RenderAnimatedOpacity::transparent();
        assert_eq!(opacity.opacity, 0.0);
        assert!(!opacity.animating);
    }

    #[test]
    fn test_animated_opacity_animating_to() {
        let opacity = RenderAnimatedOpacity::animating_to(0.75);
        assert_eq!(opacity.opacity, 0.75);
        assert!(opacity.animating);
    }

    #[test]
    fn test_animated_opacity_clamping() {
        let opacity1 = RenderAnimatedOpacity::new(-0.5, false);
        assert_eq!(opacity1.opacity, 0.0);

        let opacity2 = RenderAnimatedOpacity::new(1.5, false);
        assert_eq!(opacity2.opacity, 1.0);
    }

    #[test]
    fn test_animated_opacity_set_opacity() {
        let mut opacity = RenderAnimatedOpacity::opaque();
        opacity.set_opacity(0.3);
        assert_eq!(opacity.opacity, 0.3);
    }

    #[test]
    fn test_animated_opacity_set_animating() {
        let mut opacity = RenderAnimatedOpacity::new(0.5, false);
        opacity.set_animating(true);
        assert!(opacity.animating);
    }
}

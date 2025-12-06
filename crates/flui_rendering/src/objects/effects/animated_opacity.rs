//! RenderAnimatedOpacity - animated opacity transitions

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
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

impl RenderObject for RenderAnimatedOpacity {}

impl RenderBox<Single> for RenderAnimatedOpacity {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        let child_id = *ctx.children.single();
        // Layout child with same constraints
        Ok(ctx.layout_child(child_id, ctx.constraints)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = *ctx.children.single();

        // Skip painting if fully transparent
        if self.opacity <= 0.0 {
            return;
        }

        // TODO: Implement proper opacity layer support in Canvas API
        // For now, just paint child directly - opacity effect is visual only
        // In future: save layer with opacity, paint child, restore layer
        ctx.paint_child(child_id, ctx.offset);
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

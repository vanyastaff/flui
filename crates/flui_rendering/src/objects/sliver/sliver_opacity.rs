//! RenderSliverOpacity - Applies opacity to sliver content

use crate::core::{PaintContext, PaintTree, RenderSliverProxy, Single};

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
        }
    }

    /// Set opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Set always include semantics
    pub fn set_always_include_semantics(&mut self, always: bool) {
        self.always_include_semantics = always;
    }

    /// Check if child should be painted
    pub fn should_paint(&self) -> bool {
        self.opacity > 0.0
    }

    /// Check if needs compositing layer
    pub fn needs_compositing(&self) -> bool {
        self.opacity > 0.0 && self.opacity < 1.0
    }
}

impl Default for RenderSliverOpacity {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl RenderSliverProxy for RenderSliverOpacity {
    // Layout: use default proxy (passes constraints through)

    // Paint: custom implementation for opacity
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // If fully transparent, skip painting (unless semantics required)
        if !self.should_paint() && !self.always_include_semantics {
            return;
        }

        // If fully opaque, skip the layer overhead
        if self.opacity >= 1.0 {
            ctx.proxy();
            return;
        }

        // Apply opacity layer for compositing
        ctx.canvas().save_layer_opacity(None, self.opacity);
        ctx.proxy();
        ctx.canvas().restored();
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
    fn test_should_paint() {
        let opacity_visible = RenderSliverOpacity::new(0.5);
        let opacity_invisible = RenderSliverOpacity::new(0.0);

        assert!(opacity_visible.should_paint());
        assert!(!opacity_invisible.should_paint());
    }

    #[test]
    fn test_needs_compositing() {
        let opacity_full = RenderSliverOpacity::new(1.0);
        let opacity_partial = RenderSliverOpacity::new(0.5);
        let opacity_zero = RenderSliverOpacity::new(0.0);

        assert!(!opacity_full.needs_compositing());
        assert!(opacity_partial.needs_compositing());
        assert!(!opacity_zero.needs_compositing());
    }

    #[test]
    fn test_default_is_opaque() {
        let opacity = RenderSliverOpacity::default();

        assert_eq!(opacity.opacity, 1.0);
    }

    #[test]
    fn test_opacity_range() {
        // Test edge cases
        let opacity_min = RenderSliverOpacity::new(0.0);
        let opacity_max = RenderSliverOpacity::new(1.0);
        let opacity_mid = RenderSliverOpacity::new(0.5);

        assert_eq!(opacity_min.opacity, 0.0);
        assert_eq!(opacity_max.opacity, 1.0);
        assert_eq!(opacity_mid.opacity, 0.5);
    }
}

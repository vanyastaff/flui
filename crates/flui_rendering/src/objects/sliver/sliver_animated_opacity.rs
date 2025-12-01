//! RenderSliverAnimatedOpacity - Animated opacity for sliver content

use flui_core::render::{RuntimeArity, LegacySliverRender, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::SliverGeometry;

/// RenderObject that applies animated opacity to a sliver child
///
/// Similar to RenderSliverOpacity but designed for animated transitions.
/// The opacity value is expected to change over time (driven by an animation),
/// and this render object handles the efficient rendering of those changes.
///
/// # Differences from RenderSliverOpacity
///
/// - **Animated**: Optimized for frequent opacity changes
/// - **Implicit**: Can be controlled by animation controllers
/// - **Performance**: May use different compositing strategies for smoother animation
///
/// # Performance
///
/// - Opacity = 0.0: Child is not painted (optimization)
/// - Opacity = 1.0: Child painted normally (no layer)
/// - 0.0 < Opacity < 1.0: Uses compositing layer
/// - Animation triggers repaint, not relayout
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverAnimatedOpacity;
///
/// // Create with initial opacity
/// let animated_opacity = RenderSliverAnimatedOpacity::new(1.0);
///
/// // Later, update opacity (typically from animation)
/// // animated_opacity.set_opacity(0.5);
/// ```
#[derive(Debug)]
pub struct RenderSliverAnimatedOpacity {
    /// Current opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
    /// Whether to always include the child in the tree even when invisible
    pub always_include_semantics: bool,
    /// Whether the animation is currently running
    pub animating: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverAnimatedOpacity {
    /// Create new animated sliver opacity
    ///
    /// # Arguments
    /// * `opacity` - Initial opacity value between 0.0 and 1.0
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
            animating: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set opacity value
    ///
    /// This method should be called when the animation value changes.
    /// It marks the render object for repaint but not relayout.
    pub fn set_opacity(&mut self, opacity: f32) {
        let new_opacity = opacity.clamp(0.0, 1.0);
        if (self.opacity - new_opacity).abs() > f32::EPSILON {
            self.opacity = new_opacity;
            // In a full implementation, this would call mark_needs_paint()
        }
    }

    /// Set always include semantics
    pub fn set_always_include_semantics(&mut self, always: bool) {
        self.always_include_semantics = always;
    }

    /// Set whether animation is currently running
    ///
    /// This can be used for optimization - the render object may handle
    /// compositing differently when actively animating.
    pub fn set_animating(&mut self, animating: bool) {
        self.animating = animating;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Check if child should be painted
    pub fn should_paint(&self) -> bool {
        self.opacity > 0.0
    }

    /// Check if needs compositing layer
    ///
    /// Returns true if opacity is between 0 and 1, or if actively animating
    /// (even at opacity 1.0, to avoid layer creation/destruction during animation).
    pub fn needs_compositing(&self) -> bool {
        (self.opacity > 0.0 && self.opacity < 1.0) || (self.animating && self.opacity > 0.0)
    }
}

impl Default for RenderSliverAnimatedOpacity {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl LegacySliverRender for RenderSliverAnimatedOpacity {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        // Pass constraints through to child
        let child_geometry = if let Some(child_id) = ctx.children.try_single() {
            ctx.tree.layout_sliver_child(child_id, ctx.constraints)
        } else {
            SliverGeometry::default()
        };

        // Cache geometry (opacity doesn't affect layout)
        self.sliver_geometry = child_geometry;
        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Don't paint if completely transparent
        if !self.should_paint() {
            return Canvas::new();
        }

        // Paint child if present
        if let Some(child_id) = ctx.children.try_single() {
            if self.sliver_geometry.visible {
                let child_canvas = ctx.tree.paint_child(child_id, ctx.offset);

                // Apply opacity to canvas
                // TODO: When opacity layer support is available, apply it here
                // For now, we just paint normally (opacity would be applied by compositor)
                if self.needs_compositing() {
                    // Mark that this needs a compositing layer with opacity
                    // In full implementation, this would wrap in an OpacityLayer
                }

                return child_canvas;
            }
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_animated_opacity_new() {
        let opacity = RenderSliverAnimatedOpacity::new(0.7);

        assert_eq!(opacity.opacity, 0.7);
        assert!(!opacity.always_include_semantics);
        assert!(!opacity.animating);
    }

    #[test]
    fn test_render_sliver_animated_opacity_default() {
        let opacity = RenderSliverAnimatedOpacity::default();

        assert_eq!(opacity.opacity, 1.0);
    }

    #[test]
    fn test_set_opacity() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);
        opacity.set_opacity(0.8);

        assert_eq!(opacity.opacity, 0.8);
    }

    #[test]
    fn test_set_opacity_clamped() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);

        opacity.set_opacity(1.5);
        assert_eq!(opacity.opacity, 1.0);

        opacity.set_opacity(-0.3);
        assert_eq!(opacity.opacity, 0.0);
    }

    #[test]
    fn test_set_always_include_semantics() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);
        opacity.set_always_include_semantics(true);

        assert!(opacity.always_include_semantics);
    }

    #[test]
    fn test_set_animating() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);
        opacity.set_animating(true);

        assert!(opacity.animating);
    }

    #[test]
    fn test_should_paint_zero_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.0);

        assert!(!opacity.should_paint());
    }

    #[test]
    fn test_should_paint_full_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(1.0);

        assert!(opacity.should_paint());
    }

    #[test]
    fn test_should_paint_partial_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.5);

        assert!(opacity.should_paint());
    }

    #[test]
    fn test_needs_compositing_partial_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.5);

        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_full_opacity_not_animating() {
        let opacity = RenderSliverAnimatedOpacity::new(1.0);

        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_full_opacity_while_animating() {
        let mut opacity = RenderSliverAnimatedOpacity::new(1.0);
        opacity.set_animating(true);

        // Should still composite while animating to avoid layer creation/destruction
        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_zero_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.0);

        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_arity_is_single_child() {
        let opacity = RenderSliverAnimatedOpacity::new(0.5);
        assert_eq!(opacity.arity(), RuntimeArity::Exact(1));
    }
}

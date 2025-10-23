//! RenderAnimatedOpacity - animated opacity transitions

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderAnimatedOpacity
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimatedOpacityData {
    /// Current opacity value (0.0 = transparent, 1.0 = opaque)
    pub opacity: f32,
    /// Whether the animation is currently running
    pub animating: bool,
}

impl AnimatedOpacityData {
    /// Create new animated opacity data
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            animating: false,
        }
    }

    /// Create with opacity 1.0 (fully opaque)
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Create with opacity 0.0 (fully transparent)
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Create animating to target opacity
    pub fn animating_to(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            animating: true,
        }
    }
}

impl Default for AnimatedOpacityData {
    fn default() -> Self {
        Self::opaque()
    }
}

/// RenderObject that applies animated opacity to its child
///
/// Similar to RenderOpacity, but designed for animated transitions.
/// The animating flag can be used to trigger repaint boundaries.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::AnimatedOpacityData};
///
/// // Create with 50% opacity, animating
/// let mut animated = SingleRenderBox::new(AnimatedOpacityData::animating_to(0.5));
/// ```
pub type RenderAnimatedOpacity = SingleRenderBox<AnimatedOpacityData>;

// ===== Public API =====

impl RenderAnimatedOpacity {
    /// Get opacity
    pub fn opacity(&self) -> f32 {
        self.data().opacity
    }

    /// Get animating flag
    pub fn is_animating(&self) -> bool {
        self.data().animating
    }

    /// Set opacity
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if self.data().opacity != clamped {
            self.data_mut().opacity = clamped;
            RenderBoxMixin::mark_needs_paint(self);
        }
    }

    /// Set animating flag
    pub fn set_animating(&mut self, animating: bool) {
        if self.data().animating != animating {
            self.data_mut().animating = animating;
            RenderBoxMixin::mark_needs_paint(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderAnimatedOpacity {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = self.child() {
            let opacity = self.data().opacity;

            // Skip painting if fully transparent
            if opacity <= 0.0 {
                return;
            }

            // Paint child directly if fully opaque
            if opacity >= 1.0 {
                child.paint(painter, offset);
                return;
            }

            // TODO: Apply opacity to child painting
            // In egui, we would need to:
            // 1. Create a temporary layer/texture
            // 2. Paint child to that layer
            // 3. Blend the layer with opacity
            //
            // For now, we just paint the child normally
            // A real implementation would use:
            // - painter.with_layer_opacity() if available
            // - Or render to texture and composite

            child.paint(painter, offset);

            // Note: Full opacity support requires:
            // - Off-screen rendering
            // - Alpha blending
            // - Or egui Layer support
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animated_opacity_data_new() {
        let data = AnimatedOpacityData::new(0.5);
        assert_eq!(data.opacity, 0.5);
        assert!(!data.animating);
    }

    #[test]
    fn test_animated_opacity_data_opaque() {
        let data = AnimatedOpacityData::opaque();
        assert_eq!(data.opacity, 1.0);
        assert!(!data.animating);
    }

    #[test]
    fn test_animated_opacity_data_transparent() {
        let data = AnimatedOpacityData::transparent();
        assert_eq!(data.opacity, 0.0);
        assert!(!data.animating);
    }

    #[test]
    fn test_animated_opacity_data_animating_to() {
        let data = AnimatedOpacityData::animating_to(0.75);
        assert_eq!(data.opacity, 0.75);
        assert!(data.animating);
    }

    #[test]
    fn test_animated_opacity_data_clamping() {
        let data1 = AnimatedOpacityData::new(-0.5);
        assert_eq!(data1.opacity, 0.0);

        let data2 = AnimatedOpacityData::new(1.5);
        assert_eq!(data2.opacity, 1.0);
    }

    #[test]
    fn test_render_animated_opacity_new() {
        let animated = SingleRenderBox::new(AnimatedOpacityData::new(0.5));
        assert_eq!(animated.opacity(), 0.5);
        assert!(!animated.is_animating());
    }

    #[test]
    fn test_render_animated_opacity_set_opacity() {
        let mut animated = SingleRenderBox::new(AnimatedOpacityData::opaque());

        animated.set_opacity(0.3);
        assert_eq!(animated.opacity(), 0.3);
        assert!(RenderBoxMixin::needs_paint(&animated));
    }

    #[test]
    fn test_render_animated_opacity_set_animating() {
        let mut animated = SingleRenderBox::new(AnimatedOpacityData::new(0.5));

        animated.set_animating(true);
        assert!(animated.is_animating());
        assert!(RenderBoxMixin::needs_paint(&animated));
    }

    #[test]
    fn test_render_animated_opacity_layout() {
        let mut animated = SingleRenderBox::new(AnimatedOpacityData::new(0.5));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = animated.layout(constraints);

        // No child, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}

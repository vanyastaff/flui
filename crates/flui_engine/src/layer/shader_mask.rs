//! ShaderMaskLayer - Applies a shader as a mask to child content
//!
//! This layer type enables advanced masking effects like gradient fades and vignettes
//! by rendering child content to an offscreen texture and applying a GPU shader as a mask.

use crate::renderer::CommandRenderer;
use flui_types::{
    geometry::Rect,
    painting::{BlendMode, ShaderSpec},
};

/// Layer that applies a shader as a mask to its child
///
/// # Architecture
///
/// ```text
/// Child Content → Offscreen Texture → Apply Shader Mask → Composite to Framebuffer
/// ```
///
/// # Rendering Process
///
/// 1. Allocate offscreen texture (or acquire from pool)
/// 2. Render child layer to texture
/// 3. Apply shader as mask (GPU shader operation)
/// 4. Composite masked result to main framebuffer with blend mode
/// 5. Release texture back to pool
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::layer::ShaderMaskLayer;
/// use flui_types::painting::{BlendMode, ShaderSpec};
/// use flui_types::styling::Color32;
/// use flui_types::geometry::Rect;
///
/// // Create gradient fade mask
/// let mask_layer = ShaderMaskLayer::new(
///     child_layer,
///     ShaderSpec::LinearGradient {
///         start: (0.0, 0.0),
///         end: (1.0, 0.0),
///         colors: vec![Color32::TRANSPARENT, Color32::WHITE],
///     },
///     BlendMode::SrcOver,
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
/// );
/// ```
#[derive(Debug)]
pub struct ShaderMaskLayer {
    /// Shader specification (gradient, pattern, etc.)
    pub shader: ShaderSpec,

    /// Blend mode for compositing masked result
    pub blend_mode: BlendMode,

    /// Bounds for rendering (pre-computed for performance)
    pub bounds: Rect,
}

impl ShaderMaskLayer {
    /// Create new shader mask layer
    ///
    /// # Arguments
    ///
    /// * `shader` - Shader specification (linear gradient, radial gradient, etc.)
    /// * `blend_mode` - Blend mode for compositing
    /// * `bounds` - Bounding rectangle for rendering
    pub fn new(shader: ShaderSpec, blend_mode: BlendMode, bounds: Rect) -> Self {
        Self {
            shader,
            blend_mode,
            bounds,
        }
    }

    /// Get the bounding rectangle of this layer
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Render this layer using the provided renderer
    ///
    /// This is a placeholder implementation. Full GPU rendering will be
    /// implemented in Phase 1.3 (Offscreen Rendering).
    ///
    /// # TODO
    ///
    /// - Allocate offscreen texture
    /// - Render child to texture (need child layer reference)
    /// - Apply shader mask via GPU
    /// - Composite to framebuffer
    pub fn render(&self, _renderer: &mut dyn CommandRenderer) {
        // TODO: Implement actual GPU rendering in Phase 1.3
        // For now, this is a placeholder to establish the API
        tracing::warn!(
            "ShaderMaskLayer::render() called but not yet implemented (Phase 1.3 pending)"
        );
    }
}

// Thread safety: ShaderMaskLayer contains only owned, Send types
unsafe impl Send for ShaderMaskLayer {}
unsafe impl Sync for ShaderMaskLayer {}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::Color32;

    #[test]
    fn test_shader_mask_layer_new() {
        let shader = ShaderSpec::Solid(Color32::WHITE);
        let blend_mode = BlendMode::SrcOver;
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let layer = ShaderMaskLayer::new(shader, blend_mode, bounds);

        assert_eq!(layer.bounds(), bounds);
        assert_eq!(layer.blend_mode, BlendMode::SrcOver);
    }

    #[test]
    fn test_shader_mask_layer_bounds() {
        let shader = ShaderSpec::Solid(Color32::BLACK);
        let bounds = Rect::from_xywh(10.0, 20.0, 200.0, 150.0);

        let layer = ShaderMaskLayer::new(shader, BlendMode::SrcOver, bounds);

        let retrieved_bounds = layer.bounds();
        assert_eq!(retrieved_bounds, bounds);
        assert_eq!(retrieved_bounds.width(), 200.0);
        assert_eq!(retrieved_bounds.height(), 150.0);
    }

    #[test]
    fn test_shader_mask_layer_linear_gradient() {
        let shader = ShaderSpec::LinearGradient {
            start: (0.0, 0.0),
            end: (1.0, 1.0),
            colors: vec![Color32::RED, Color32::BLUE],
        };
        let bounds = Rect::from_xywh(0.0, 0.0, 50.0, 50.0);

        let layer = ShaderMaskLayer::new(shader, BlendMode::Multiply, bounds);

        assert_eq!(layer.blend_mode, BlendMode::Multiply);
        match layer.shader {
            ShaderSpec::LinearGradient { start, end, .. } => {
                assert_eq!(start, (0.0, 0.0));
                assert_eq!(end, (1.0, 1.0));
            }
            _ => panic!("Expected LinearGradient"),
        }
    }

    #[test]
    fn test_shader_mask_layer_radial_gradient() {
        let shader = ShaderSpec::RadialGradient {
            center: (0.5, 0.5),
            radius: 1.0,
            colors: vec![Color32::WHITE, Color32::BLACK],
        };
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let layer = ShaderMaskLayer::new(shader, BlendMode::Screen, bounds);

        assert_eq!(layer.blend_mode, BlendMode::Screen);
        match layer.shader {
            ShaderSpec::RadialGradient { center, radius, .. } => {
                assert_eq!(center, (0.5, 0.5));
                assert_eq!(radius, 1.0);
            }
            _ => panic!("Expected RadialGradient"),
        }
    }

    #[test]
    fn test_shader_mask_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ShaderMaskLayer>();
        assert_sync::<ShaderMaskLayer>();
    }
}

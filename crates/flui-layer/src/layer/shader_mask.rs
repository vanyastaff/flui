//! ShaderMaskLayer - Applies a shader as a mask to child content
//!
//! This layer type enables advanced masking effects like gradient fades and vignettes
//! by rendering child content to an offscreen texture and applying a GPU shader as a mask.

use flui_types::{
    geometry::{Pixels, Rect},
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
/// ```rust
/// use flui_layer::ShaderMaskLayer;
/// use flui_types::painting::{BlendMode, ShaderSpec};
/// use flui_types::styling::Color32;
/// use flui_types::geometry::Rect;
///
/// // Create gradient fade mask
/// let mask_layer = ShaderMaskLayer::new(
///     ShaderSpec::LinearGradient {
///         start: (0.0, 0.0),
///         end: (1.0, 0.0),
///         colors: vec![Color32::TRANSPARENT, Color32::WHITE],
///     },
///     BlendMode::SrcOver,
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ShaderMaskLayer {
    /// Shader specification (gradient, pattern, etc.)
    shader: ShaderSpec,

    /// Blend mode for compositing masked result
    blend_mode: BlendMode,

    /// Bounds for rendering (pre-computed for performance)
    bounds: Rect<Pixels>,
}

impl ShaderMaskLayer {
    /// Create new shader mask layer
    ///
    /// # Arguments
    ///
    /// * `shader` - Shader specification (linear gradient, radial gradient, etc.)
    /// * `blend_mode` - Blend mode for compositing
    /// * `bounds` - Bounding rectangle for rendering
    pub fn new(shader: ShaderSpec, blend_mode: BlendMode, bounds: Rect<Pixels>) -> Self {
        Self {
            shader,
            blend_mode,
            bounds,
        }
    }

    /// Get the shader specification.
    pub fn shader(&self) -> &ShaderSpec {
        &self.shader
    }

    /// Get the blend mode.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Get the bounding rectangle of this layer.
    pub fn bounds(&self) -> Rect<Pixels> {
        self.bounds
    }

    /// Set new bounds for this layer.
    pub fn set_bounds(&mut self, bounds: Rect<Pixels>) {
        self.bounds = bounds;
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
        assert_eq!(layer.blend_mode(), BlendMode::SrcOver);
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

        assert_eq!(layer.blend_mode(), BlendMode::Multiply);
        match layer.shader() {
            ShaderSpec::LinearGradient { start, end, .. } => {
                assert_eq!(*start, (0.0, 0.0));
                assert_eq!(*end, (1.0, 1.0));
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

        assert_eq!(layer.blend_mode(), BlendMode::Screen);
        match layer.shader() {
            ShaderSpec::RadialGradient { center, radius, .. } => {
                assert_eq!(*center, (0.5, 0.5));
                assert_eq!(*radius, 1.0);
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

    #[test]
    fn test_shader_mask_layer_clone() {
        let shader = ShaderSpec::Solid(Color32::RED);
        let bounds = Rect::from_xywh(0.0, 0.0, 50.0, 50.0);
        let layer = ShaderMaskLayer::new(shader, BlendMode::SrcOver, bounds);

        let cloned = layer.clone();
        assert_eq!(cloned.bounds(), layer.bounds());
        assert_eq!(cloned.blend_mode(), layer.blend_mode());
    }
}

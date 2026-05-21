//! ShaderMaskLayer - Applies a shader as a mask to child content
//!
//! This layer type enables advanced masking effects like gradient fades and
//! vignettes by rendering child content to an offscreen texture and applying a
//! GPU shader as a mask.

use flui_types::{
    geometry::{Pixels, Rect},
    painting::{BlendMode, Shader},
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
/// use flui_types::{
///     geometry::{Offset, Rect, px},
///     painting::{BlendMode, Shader},
///     styling::Color,
/// };
///
/// // Create gradient fade mask
/// let mask_layer = ShaderMaskLayer::new(
///     Shader::simple_linear(
///         Offset::ZERO,
///         Offset::new(px(100.0), px(0.0)),
///         vec![Color::TRANSPARENT, Color::WHITE],
///     ),
///     BlendMode::SrcOver,
///     Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ShaderMaskLayer {
    /// Shader (gradient, solid, etc.)
    shader: Shader,

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
    /// * `shader` - Shader (linear gradient, radial gradient, solid, etc.)
    /// * `blend_mode` - Blend mode for compositing
    /// * `bounds` - Bounding rectangle for rendering
    pub fn new(shader: Shader, blend_mode: BlendMode, bounds: Rect<Pixels>) -> Self {
        Self {
            shader,
            blend_mode,
            bounds,
        }
    }

    /// Get the shader.
    pub fn shader(&self) -> &Shader {
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

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact f32 values they just set; ULP slop would mask real regressions"
)]
mod tests {
    use flui_types::{
        geometry::{Offset, px},
        styling::Color,
    };

    use super::*;

    #[test]
    fn test_shader_mask_layer_new() {
        let shader = Shader::solid(Color::WHITE);
        let blend_mode = BlendMode::SrcOver;
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        let layer = ShaderMaskLayer::new(shader, blend_mode, bounds);

        assert_eq!(layer.bounds(), bounds);
        assert_eq!(layer.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn test_shader_mask_layer_bounds() {
        let shader = Shader::solid(Color::BLACK);
        let bounds = Rect::from_xywh(px(10.0), px(20.0), px(200.0), px(150.0));

        let layer = ShaderMaskLayer::new(shader, BlendMode::SrcOver, bounds);

        let retrieved_bounds = layer.bounds();
        assert_eq!(retrieved_bounds, bounds);
        assert_eq!(retrieved_bounds.width(), px(200.0));
        assert_eq!(retrieved_bounds.height(), px(150.0));
    }

    #[test]
    fn test_shader_mask_layer_linear_gradient() {
        let shader = Shader::simple_linear(
            Offset::ZERO,
            Offset::new(px(50.0), px(50.0)),
            vec![Color::RED, Color::BLUE],
        );
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));

        let layer = ShaderMaskLayer::new(shader, BlendMode::Multiply, bounds);

        assert_eq!(layer.blend_mode(), BlendMode::Multiply);
        match layer.shader() {
            Shader::LinearGradient { from, to, .. } => {
                assert_eq!(*from, Offset::ZERO);
                assert_eq!(*to, Offset::new(px(50.0), px(50.0)));
            }
            _ => panic!("Expected LinearGradient"),
        }
    }

    #[test]
    fn test_shader_mask_layer_radial_gradient() {
        let shader = Shader::simple_radial(
            Offset::new(px(50.0), px(50.0)),
            50.0,
            vec![Color::WHITE, Color::BLACK],
        );
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        let layer = ShaderMaskLayer::new(shader, BlendMode::Screen, bounds);

        assert_eq!(layer.blend_mode(), BlendMode::Screen);
        match layer.shader() {
            Shader::RadialGradient { center, radius, .. } => {
                assert_eq!(*center, Offset::new(px(50.0), px(50.0)));
                assert_eq!(*radius, 50.0);
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
        let shader = Shader::solid(Color::RED);
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let layer = ShaderMaskLayer::new(shader, BlendMode::SrcOver, bounds);

        let cloned = layer.clone();
        assert_eq!(cloned.bounds(), layer.bounds());
        assert_eq!(cloned.blend_mode(), layer.blend_mode());
    }
}

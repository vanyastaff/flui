//! `ShaderMaskLayer` — masks its subtree with a shader: gradient fades, vignettes.

use flui_foundation::geometry::Rect;
use flui_painting::paint::{BlendMode, Shader};

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
/// use flui_foundation::geometry::{Offset, Rect};
/// use flui_painting::{paint::{BlendMode, Shader}, styling::Color};
///
/// // Create gradient fade mask
/// let mask_layer = ShaderMaskLayer::new(
///     Shader::simple_linear(
///         Offset::ZERO,
///         Offset::new(100.0, 0.0),
///         vec![Color::TRANSPARENT, Color::WHITE],
///     ),
///     BlendMode::SrcOver,
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ShaderMaskLayer {
    /// Shader (gradient, solid, etc.)
    shader: Shader,

    /// Blend mode for compositing masked result
    blend_mode: BlendMode,

    /// Bounds for rendering (pre-computed for performance)
    bounds: Rect<f64>,
}

impl ShaderMaskLayer {
    /// Masks the subtree with `shader` inside `bounds`, compositing with `blend_mode`.
    pub fn new(shader: Shader, blend_mode: BlendMode, bounds: Rect<f64>) -> Self {
        Self {
            shader,
            blend_mode,
            bounds,
        }
    }

    /// The mask shader (gradient, solid, …).
    pub fn shader(&self) -> &Shader {
        &self.shader
    }

    /// How the masked subtree composites onto its parent.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// The rectangle the mask covers.
    pub fn bounds(&self) -> Rect<f64> {
        self.bounds
    }
}

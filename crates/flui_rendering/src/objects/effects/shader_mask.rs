//! RenderShaderMask - Applies a shader as a mask
//!
//! This render object applies a shader (gradient, pattern, etc.) as a mask
//! to its child, controlling which parts are visible.

use flui_core::render::{
    RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::{painting::BlendMode, styling::Color32, Size};

// ===== Data Structure =====
/// FIXME: This is a placeholder structure for shader mask data. All types should be in flui_types.
/// Shader specification (simplified for now)
#[derive(Debug, Clone)]
pub enum ShaderSpec {
    /// Linear gradient shader
    LinearGradient {
        /// Start point (relative to size)
        start: (f32, f32),
        /// End point (relative to size)
        end: (f32, f32),
        /// Colors
        colors: Vec<Color32>,
    },
    /// Radial gradient shader
    RadialGradient {
        /// Center point (relative to size)
        center: (f32, f32),
        /// Radius (relative to size)
        radius: f32,
        /// Colors
        colors: Vec<Color32>,
    },
    /// Solid color (for testing)
    Solid(Color32),
}

// ===== RenderObject =====

/// RenderShaderMask - Applies a shader as a mask to a child_id
///
/// Uses a shader (gradient, pattern, etc.) to mask the child_id's rendering.
/// Common use cases:
/// - Gradient fades
/// - Vignette effects
/// - Custom masking patterns
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderShaderMask;
/// use flui_types::styling::Color32;
///
/// // Create gradient fade mask
/// let mask = RenderShaderMask::linear_gradient(
///     (0.0, 0.0),
///     (1.0, 0.0),
///     vec![
///         Color32::from_rgba_unmultiplied(255, 255, 255, 0),
///         Color32::from_rgba_unmultiplied(255, 255, 255, 255),
///     ],
/// );
/// ```
#[derive(Debug)]
pub struct RenderShaderMask {
    /// Shader to apply as mask
    pub shader: ShaderSpec,
    /// Blend mode
    pub blend_mode: BlendMode,
}

// ===== Methods =====

impl RenderShaderMask {
    /// Create new shader mask with linear gradient
    pub fn linear_gradient(start: (f32, f32), end: (f32, f32), colors: Vec<Color32>) -> Self {
        Self {
            shader: ShaderSpec::LinearGradient { start, end, colors },
            blend_mode: BlendMode::default(),
        }
    }

    /// Create new shader mask with radial gradient
    pub fn radial_gradient(center: (f32, f32), radius: f32, colors: Vec<Color32>) -> Self {
        Self {
            shader: ShaderSpec::RadialGradient {
                center,
                radius,
                colors,
            },
            blend_mode: BlendMode::default(),
        }
    }

    /// Create with solid color (for testing)
    pub fn solid(color: Color32) -> Self {
        Self {
            shader: ShaderSpec::Solid(color),
            blend_mode: BlendMode::default(),
        }
    }

    /// Set blend mode
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Get the shader specification
    pub fn shader(&self) -> &ShaderSpec {
        &self.shader
    }

    /// Set the shader
    pub fn set_shader(&mut self, shader: ShaderSpec) {
        self.shader = shader;
    }

    /// Get the blend mode
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Set the blend mode
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) {
        self.blend_mode = blend_mode;
    }
}

// ===== RenderObject Implementation =====

impl RenderBox<Single> for RenderShaderMask {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();

        // Note: Full shader masking requires compositor support
        // For now, we'll paint child normally
        // In production, this would use a ShaderMaskLayer with egui's shader system
        // or a custom compositor
        //
        // TODO: Implement ShaderMaskLayer when compositor supports it

        ctx.paint_child(child_id, ctx.offset);
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_blend_mode_default() {
        assert_eq!(BlendMode::default(), BlendMode::SrcOver);
    }

    #[test]
    fn test_render_shader_mask_new() {
        let mask = RenderShaderMask::solid(Color32::WHITE);

        match mask.shader() {
            ShaderSpec::Solid(color) => {
                assert_eq!(*color, Color32::WHITE);
            }
            _ => panic!("Expected solid shader"),
        }
        assert_eq!(mask.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn test_render_shader_mask_set_shader() {
        let mut mask = RenderShaderMask::solid(Color32::WHITE);

        let new_shader = ShaderSpec::Solid(Color32::BLACK);
        mask.set_shader(new_shader);

        match mask.shader() {
            ShaderSpec::Solid(color) => {
                assert_eq!(*color, Color32::BLACK);
            }
            _ => panic!("Expected solid shader"),
        }
    }

    #[test]
    fn test_render_shader_mask_set_blend_mode() {
        let mut mask = RenderShaderMask::solid(Color32::WHITE);

        mask.set_blend_mode(BlendMode::Multiply);
        assert_eq!(mask.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn test_render_shader_mask_with_blend_mode() {
        let mask = RenderShaderMask::solid(Color32::WHITE).with_blend_mode(BlendMode::Multiply);
        assert_eq!(mask.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn test_render_shader_mask_linear_gradient() {
        let colors = vec![Color32::WHITE, Color32::BLACK];
        let mask = RenderShaderMask::linear_gradient((0.0, 0.0), (1.0, 1.0), colors);

        match mask.shader() {
            ShaderSpec::LinearGradient { start, end, .. } => {
                assert_eq!(*start, (0.0, 0.0));
                assert_eq!(*end, (1.0, 1.0));
            }
            _ => panic!("Expected linear gradient"),
        }
    }

    #[test]
    fn test_render_shader_mask_radial_gradient() {
        let colors = vec![Color32::RED, Color32::BLUE];
        let mask = RenderShaderMask::radial_gradient((0.5, 0.5), 1.0, colors);

        match mask.shader() {
            ShaderSpec::RadialGradient { center, radius, .. } => {
                assert_eq!(*center, (0.5, 0.5));
                assert_eq!(*radius, 1.0);
            }
            _ => panic!("Expected radial gradient"),
        }
    }
}

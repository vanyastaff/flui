//! WGSL sources and mask-shader selection for offscreen effects.

use flui_types::painting::Shader;

/// Shader type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ShaderType {
    /// Solid color mask shader
    SolidMask,
    /// Linear gradient mask shader
    LinearGradientMask,
    /// Radial gradient mask shader
    RadialGradientMask,
    /// Sweep (angular/conic) gradient mask shader
    SweepGradientMask,
    /// Dual Kawase blur downsample pass shader
    DualKawaseDownsample,
    /// Dual Kawase blur upsample pass shader
    DualKawaseUpsample,
}

impl ShaderType {
    /// Get the WGSL source code for this shader type
    pub(super) fn source_code(self) -> &'static str {
        match self {
            ShaderType::SolidMask => include_str!("../shaders/masks/solid.wgsl"),
            ShaderType::LinearGradientMask => include_str!("../shaders/masks/linear_gradient.wgsl"),
            ShaderType::RadialGradientMask => include_str!("../shaders/masks/radial_gradient.wgsl"),
            ShaderType::SweepGradientMask => include_str!("../shaders/masks/sweep_gradient.wgsl"),
            ShaderType::DualKawaseDownsample => {
                include_str!("../shaders/effects/blur_downsample.wgsl")
            }
            ShaderType::DualKawaseUpsample => {
                include_str!("../shaders/effects/blur_upsample.wgsl")
            }
        }
    }

    /// Get the shader label (for debugging)
    pub(super) fn label(self) -> &'static str {
        match self {
            ShaderType::SolidMask => "Solid Mask Shader",
            ShaderType::LinearGradientMask => "Linear Gradient Mask Shader",
            ShaderType::RadialGradientMask => "Radial Gradient Mask Shader",
            ShaderType::SweepGradientMask => "Sweep Gradient Mask Shader",
            ShaderType::DualKawaseDownsample => "Dual Kawase Downsample",
            ShaderType::DualKawaseUpsample => "Dual Kawase Upsample",
        }
    }

    /// Get the shader type from a Shader
    pub(super) fn from_shader(shader: &Shader) -> Self {
        match shader {
            Shader::LinearGradient { .. } => ShaderType::LinearGradientMask,
            Shader::RadialGradient { .. } => ShaderType::RadialGradientMask,
            Shader::SweepGradient { .. } => ShaderType::SweepGradientMask,
            Shader::Solid { .. } => ShaderType::SolidMask,
            // Image shader masks use full-opacity (white) solid mask because texture-based
            // masking requires a separate texture binding slot that the current mask pipeline
            // does not support. A dedicated image-mask pipeline is future work.
            _ => {
                tracing::debug!(
                    "ShaderType::from_shader: unsupported shader variant, using SolidMask (full opacity)"
                );
                ShaderType::SolidMask
            }
        }
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use flui_types::styling::Color;

    use super::*;

    #[test]
    fn test_shader_type_from_shader() {
        use flui_types::geometry::{Offset, px};

        let solid = Shader::solid(Color::WHITE);
        assert_eq!(ShaderType::from_shader(&solid), ShaderType::SolidMask);

        let linear = Shader::simple_linear(
            Offset::ZERO,
            Offset::new(px(1.0), px(1.0)),
            vec![Color::RED, Color::BLUE],
        );
        assert_eq!(
            ShaderType::from_shader(&linear),
            ShaderType::LinearGradientMask
        );

        let radial = Shader::simple_radial(
            Offset::new(px(0.5), px(0.5)),
            1.0,
            vec![Color::WHITE, Color::BLACK],
        );
        assert_eq!(
            ShaderType::from_shader(&radial),
            ShaderType::RadialGradientMask
        );
    }
}

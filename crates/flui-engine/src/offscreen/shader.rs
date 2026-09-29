//! WGSL sources and mask-shader selection for offscreen effects.

use flui_painting::paint::Shader;

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

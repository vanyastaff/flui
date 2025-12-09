//! WGSL shaders for wgpu backend.

// Basic shapes
/// Basic shape rendering shader.
pub const SHAPE: &str = include_str!("shape.wgsl");
/// Solid fill shader.
pub const FILL: &str = include_str!("fill.wgsl");

// Instanced rendering
/// Instanced rectangle rendering shader.
pub const RECT_INSTANCED: &str = include_str!("rect_instanced.wgsl");
/// Instanced circle rendering shader.
pub const CIRCLE_INSTANCED: &str = include_str!("circle_instanced.wgsl");
/// Instanced arc rendering shader.
pub const ARC_INSTANCED: &str = include_str!("arc_instanced.wgsl");
/// Instanced texture rendering shader.
pub const TEXTURE_INSTANCED: &str = include_str!("texture_instanced.wgsl");

/// Mask shaders for clipping and masking operations.
pub mod masks {
    /// Solid color mask shader.
    pub const SOLID: &str = include_str!("masks/solid.wgsl");
    /// Linear gradient mask shader.
    pub const LINEAR_GRADIENT: &str = include_str!("masks/linear_gradient.wgsl");
    /// Radial gradient mask shader.
    pub const RADIAL_GRADIENT: &str = include_str!("masks/radial_gradient.wgsl");
}

/// Effect shaders for blur, shadow, and other visual effects.
pub mod effects {
    /// Horizontal blur pass shader.
    pub const BLUR_HORIZONTAL: &str = include_str!("effects/blur_horizontal.wgsl");
    /// Vertical blur pass shader.
    pub const BLUR_VERTICAL: &str = include_str!("effects/blur_vertical.wgsl");
    /// Blur downsample shader for efficient multi-pass blur.
    pub const BLUR_DOWNSAMPLE: &str = include_str!("effects/blur_downsample.wgsl");
    /// Blur upsample shader for efficient multi-pass blur.
    pub const BLUR_UPSAMPLE: &str = include_str!("effects/blur_upsample.wgsl");
    /// Drop shadow shader.
    pub const SHADOW: &str = include_str!("effects/shadow.wgsl");
}

/// Gradient shaders for linear and radial gradients.
pub mod gradients {
    /// Linear gradient shader.
    pub const LINEAR: &str = include_str!("gradients/linear.wgsl");
    /// Radial gradient shader.
    pub const RADIAL: &str = include_str!("gradients/radial.wgsl");
}

/// Common shader utilities.
pub mod common {
    /// Signed distance field utilities.
    pub const SDF: &str = include_str!("common/sdf.wgsl");
}

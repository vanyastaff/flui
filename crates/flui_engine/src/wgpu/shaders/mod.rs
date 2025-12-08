//! WGSL shaders for wgpu backend.

// Basic shapes
pub const SHAPE: &str = include_str!("shape.wgsl");
pub const FILL: &str = include_str!("fill.wgsl");

// Instanced rendering
pub const RECT_INSTANCED: &str = include_str!("rect_instanced.wgsl");
pub const CIRCLE_INSTANCED: &str = include_str!("circle_instanced.wgsl");
pub const ARC_INSTANCED: &str = include_str!("arc_instanced.wgsl");
pub const TEXTURE_INSTANCED: &str = include_str!("texture_instanced.wgsl");

// Masks
pub mod masks {
    pub const SOLID: &str = include_str!("masks/solid.wgsl");
    pub const LINEAR_GRADIENT: &str = include_str!("masks/linear_gradient.wgsl");
    pub const RADIAL_GRADIENT: &str = include_str!("masks/radial_gradient.wgsl");
}

// Effects
pub mod effects {
    pub const BLUR_HORIZONTAL: &str = include_str!("effects/blur_horizontal.wgsl");
    pub const BLUR_VERTICAL: &str = include_str!("effects/blur_vertical.wgsl");
    pub const BLUR_DOWNSAMPLE: &str = include_str!("effects/blur_downsample.wgsl");
    pub const BLUR_UPSAMPLE: &str = include_str!("effects/blur_upsample.wgsl");
    pub const SHADOW: &str = include_str!("effects/shadow.wgsl");
}

// Gradients
pub mod gradients {
    pub const LINEAR: &str = include_str!("gradients/linear.wgsl");
    pub const RADIAL: &str = include_str!("gradients/radial.wgsl");
}

// Common utilities
pub mod common {
    pub const SDF: &str = include_str!("common/sdf.wgsl");
}

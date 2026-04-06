//! Render pipeline for linear and radial gradients.
//!
//! **Placeholder:** Both pipelines currently use the rect shader.

/// Creates the linear gradient render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated gradient shader is wired.
pub fn create_linear_gradient_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(
        device,
        format,
        bind_group_layout,
        "linear_gradient",
    )
}

/// Creates the radial gradient render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated gradient shader is wired.
pub fn create_radial_gradient_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(
        device,
        format,
        bind_group_layout,
        "radial_gradient",
    )
}

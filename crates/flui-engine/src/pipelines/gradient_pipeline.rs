//! Render pipeline for linear and radial gradients.
//!
//! Gradient shaders require storage buffers for dynamic color stops, which
//! adds complexity. For v1, both pipelines use the rect instanced shader as a
//! placeholder with RectInstance layout. Gradient rendering will be wired in a
//! future pass when the storage buffer infrastructure is in place.

/// Creates the linear gradient render pipeline.
///
/// **Placeholder:** Uses the rect shader until the gradient storage buffer
/// infrastructure is wired.
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
/// **Placeholder:** Uses the rect shader until the gradient storage buffer
/// infrastructure is wired.
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

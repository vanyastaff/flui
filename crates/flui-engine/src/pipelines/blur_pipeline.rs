//! Render pipeline for Gaussian blur effects and final compositing.
//!
//! **Placeholder:** All pipelines currently use the rect shader.

/// Creates the blur downsample render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated blur shader is wired.
pub fn create_blur_downsample_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(
        device,
        format,
        bind_group_layout,
        "blur_downsample",
    )
}

/// Creates the blur upsample render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated blur shader is wired.
pub fn create_blur_upsample_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(
        device,
        format,
        bind_group_layout,
        "blur_upsample",
    )
}

/// Creates the final compositing render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated compositing shader is wired.
pub fn create_compositing_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(
        device,
        format,
        bind_group_layout,
        "compositing",
    )
}

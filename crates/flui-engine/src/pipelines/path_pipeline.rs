//! Render pipeline for tessellated vector paths.
//!
//! Handles both fill and stroke rendering of lyon-tessellated geometry.
//! **Placeholder:** Both pipelines currently use the rect shader.

/// Creates the path fill render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated fill shader is wired.
pub fn create_path_fill_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(device, format, bind_group_layout, "path_fill")
}

/// Creates the path stroke render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated stroke shader is wired.
pub fn create_path_stroke_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(device, format, bind_group_layout, "path_stroke")
}

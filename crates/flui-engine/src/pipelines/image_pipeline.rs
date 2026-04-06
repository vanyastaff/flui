//! Render pipeline for textured image quads.
//!
//! **Placeholder:** Uses the rect shader until the dedicated image shader is wired.

/// Creates the textured image quad render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated image shader is wired.
pub fn create_image_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(device, format, bind_group_layout, "image")
}

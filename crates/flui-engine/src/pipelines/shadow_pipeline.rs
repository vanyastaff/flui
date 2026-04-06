//! Render pipeline for box shadow rendering.
//!
//! **Placeholder:** Uses the rect shader until the dedicated shadow shader is wired.

/// Creates the box shadow render pipeline.
///
/// **Placeholder:** Uses the rect shader until the dedicated shadow shader is wired.
pub fn create_shadow_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    super::shape_pipeline::create_placeholder_pipeline(device, format, bind_group_layout, "shadow")
}

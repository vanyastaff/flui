//! Cached intermediate-to-surface presentation blit.
//! Layer masks and backdrops use ordered painter IR, not this helper.
use crate::device_domain::{DeviceDomain, PreparedCost};
use crate::error::EngineResult;
use std::sync::Arc;
mod blit;
/// Cached presentation resources for intermediate-to-surface copies.
#[cfg_attr(not(feature = "testing"), expect(unreachable_pub))]
pub struct OffscreenRenderer {
    domain: Arc<DeviceDomain>,
    device: Arc<wgpu::Device>,
    blit_pipeline: Option<BlitPipeline>,
}
struct BlitPipeline {
    pipeline: Arc<wgpu::RenderPipeline>,
    bind_group_layout: Arc<wgpu::BindGroupLayout>,
    /// Nearest-neighbour sampler — cached because its parameters never change.
    sampler: Arc<wgpu::Sampler>,
    /// Fullscreen-quad vertex buffer — cached because its contents never change.
    vertex_buffer: Arc<wgpu::Buffer>,
}

impl OffscreenRenderer {
    /// Construct the presentation helper with a shared GPU device.
    #[cfg(feature = "testing")]
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self::with_domain(DeviceDomain::new(device, queue))
    }
    pub(crate) fn with_domain(domain: Arc<DeviceDomain>) -> Self {
        let device = Arc::clone(domain.device());
        Self {
            domain,
            device,
            blit_pipeline: None,
        }
    }
}
/// Vertex for fullscreen quad rendering
///
/// Used by the intermediate-to-surface presentation blit.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FullscreenVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
}

impl FullscreenVertex {
    /// Create fullscreen quad vertices
    ///
    /// Returns 6 vertices forming 2 triangles that cover the entire screen.
    pub(crate) fn fullscreen_quad() -> [FullscreenVertex; 6] {
        [
            // Triangle 1
            FullscreenVertex {
                position: [-1.0, -1.0],
                tex_coords: [0.0, 1.0],
            },
            FullscreenVertex {
                position: [1.0, -1.0],
                tex_coords: [1.0, 1.0],
            },
            FullscreenVertex {
                position: [-1.0, 1.0],
                tex_coords: [0.0, 0.0],
            },
            // Triangle 2
            FullscreenVertex {
                position: [-1.0, 1.0],
                tex_coords: [0.0, 0.0],
            },
            FullscreenVertex {
                position: [1.0, -1.0],
                tex_coords: [1.0, 1.0],
            },
            FullscreenVertex {
                position: [1.0, 1.0],
                tex_coords: [1.0, 0.0],
            },
        ]
    }
}

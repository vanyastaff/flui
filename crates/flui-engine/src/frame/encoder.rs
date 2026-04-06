//! Command encoder wrapper for building GPU command buffers.
//!
//! [`FrameEncoder`] ties together the render surface, GPU device, batchers,
//! and state stacks into a single per-frame recording context. It is created
//! by [`RenderSurface::begin_frame`](crate::context::render_surface::RenderSurface::begin_frame)
//! and consumed by [`finish`](FrameEncoder::finish) which submits all recorded
//! GPU work and presents the frame.

use std::sync::Arc;

use crate::context::gpu_device::GpuDevice;
use crate::context::render_surface::RenderSurface;
use crate::error::RenderResult;
use crate::frame::dispatch::{traverse_scene, Batchers};
use crate::frame::state_stack::StateStack;
use flui_layer::Scene;

/// Per-frame command encoder. Created by
/// [`RenderSurface::begin_frame`](crate::context::render_surface::RenderSurface::begin_frame).
///
/// Records draw commands via [`render_scene`](Self::render_scene), then submits
/// to the GPU and presents via [`finish`](Self::finish).
pub struct FrameEncoder<'surface> {
    surface: &'surface RenderSurface,
    gpu: Arc<GpuDevice>,
    surface_texture: wgpu::SurfaceTexture,
    surface_view: wgpu::TextureView,
    batchers: Batchers,
    state: StateStack,
    scale_factor: f32,
}

impl<'surface> FrameEncoder<'surface> {
    /// Create a new frame encoder. Called by `RenderSurface::begin_frame`.
    pub(crate) fn new(
        surface: &'surface RenderSurface,
        surface_texture: wgpu::SurfaceTexture,
    ) -> Self {
        let gpu = Arc::clone(surface.gpu());
        let scale_factor = surface.scale_factor();
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            surface,
            gpu,
            surface_texture,
            surface_view,
            batchers: Batchers::new(),
            state: StateStack::new(),
            scale_factor,
        }
    }

    /// Traverse a [`Scene`]'s layer tree and record all draw commands into
    /// internal batchers.
    pub fn render_scene(&mut self, scene: &Scene) -> RenderResult<()> {
        let _span = tracing::debug_span!("render_scene").entered();
        traverse_scene(scene, &mut self.batchers, &mut self.state, self.scale_factor);
        Ok(())
    }

    /// Submit all recorded GPU work and present the frame.
    ///
    /// On error the frame is dropped -- the caller should call
    /// [`RenderSurface::resize`](crate::context::render_surface::RenderSurface::resize)
    /// and retry on the next frame.
    pub fn finish(self) -> RenderResult<()> {
        let _span = tracing::debug_span!("finish_frame").entered();

        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("flui_frame_encoder"),
                });

        // Create render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("flui_main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Set viewport
            render_pass.set_viewport(
                0.0,
                0.0,
                self.surface.width() as f32,
                self.surface.height() as f32,
                0.0,
                1.0,
            );

            // TODO: Upload batcher data to GPU buffers and execute draw calls.
            // For now we just clear the screen -- actual draw submission will
            // be wired when pipelines are fully connected to shaders.
            let _span = tracing::debug_span!("submit_draws").entered();

            // Shape instanced draws would go here:
            //   for each rect batch: set pipeline, set buffers, draw_indexed
            //   for each circle batch: similar
            //   etc.
        }

        // Submit
        self.gpu
            .queue()
            .submit(std::iter::once(encoder.finish()));

        // Present
        self.surface_texture.present();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batchers_start_empty() {
        let b = Batchers::new();
        assert!(b.is_all_empty());
    }

    // FrameEncoder needs a real GPU surface, so real tests go in integration
    // tests (Task 17).
}

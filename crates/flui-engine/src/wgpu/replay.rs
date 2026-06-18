//! Texture-batch replay/submit component for `WgpuPainter`.
//!
//! `GpuReplay` owns the per-frame texture-instance scratch batch and the
//! three flush helpers that submit it to the GPU.  This is the **skeleton** of
//! the record/replay split introduced in T10:
//!
//! | T10 step | What moves                                                |
//! |----------|-----------------------------------------------------------|
//! | T10b (this) | `texture_batch` field + `flush_texture_batch*` family |
//! | T10c     | Segment-flush helpers (`flush_segment*`)                  |
//! | T10d     | Top-level `render()` / `flush_opacity_layer`              |
//!
//! ## Borrow-split strategy
//!
//! The GPU plumbing fields (`device`, `queue`, `pipelines`, `viewport_bind_group`,
//! `unit_quad_buffer`, `unit_quad_index_buffer`, `default_sampler`) still live on
//! `WgpuPainter` in T10b.  They are passed as borrowed parameters to each flush
//! method so that `&mut GpuReplay` and `&WgpuPainter`-owned fields can coexist
//! in the same call without `&mut self` on the painter.  T10c/d will move those
//! fields here as the split progresses.
//!
//! ## Invariants
//!
//! - `texture_batch` is allocated once at construction time with a capacity of
//!   1 024 instances.  It is cleared at the end of each flush, preserving the
//!   allocated capacity across frames (no per-frame heap allocation).
//! - Every `flush_texture_batch*` call leaves the batch empty on return.
//! - Neither this module nor its callers bring in `flui_types::Matrix4` — the
//!   C4 rule (port-check Trigger 19) keeps `Matrix4` out of the batches and
//!   replay path.

use std::sync::Arc;

use super::{
    command_ir::ScissorRect,
    instancing::{InstanceBatch, TextureInstance},
    pipelines::PipelineSet,
    resources::GpuResources,
};

/// Owns the texture-instance scratch batch and the GPU flush helpers that
/// submit it.
///
/// Created once per `WgpuPainter` via [`GpuReplay::new`] and stored as the
/// `replay` field.  Callers accumulate instances with
/// `replay.texture_batch.add(instance)` and submit the batch with one of the
/// three flush methods, passing the GPU plumbing they need as borrowed
/// parameters.
// `wgpu::Device` / `wgpu::Queue` do not implement `Debug`.
#[allow(missing_debug_implementations)]
pub(super) struct GpuReplay {
    /// Per-frame scratch batch for texture instances.
    ///
    /// Allocated once at construction time (1 024-instance capacity) and
    /// cleared after each flush.  Accumulate with `.texture_batch.add(instance)`;
    /// submit with one of the `flush_texture_batch*` methods.
    pub(super) texture_batch: InstanceBatch<TextureInstance>,
}

// The flush methods temporarily accept GPU plumbing fields (device, queue,
// pipelines, buffers, sampler, resources) as borrowed parameters because those
// fields still live on `WgpuPainter` in T10b.  T10c/d will move them here,
// eliminating the argument lists entirely.  The `too_many_arguments` lint is
// suppressed for this transitional state only.
//
// `cast_possible_truncation` and `cast_sign_loss` are suppressed for the same
// reason as in `painter.rs`: GPU rendering converts between numeric types
// (pixel coords, buffer indices, instance counts) intentionally.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl GpuReplay {
    /// Create a new [`GpuReplay`] with a pre-allocated texture-instance scratch
    /// batch.
    ///
    /// Mirrors the `texture_batch` initialisation that previously lived in
    /// `WgpuPainter::with_shared_device`.
    pub(super) fn new() -> Self {
        Self {
            texture_batch: InstanceBatch::new(1024),
        }
    }

    /// Flush the texture instance batch with straight-alpha blending.
    ///
    /// Renders all batched textures in a single draw call using GPU instancing.
    /// This is 50–100× faster than individual draw calls for image-heavy UIs.
    ///
    /// This is the **straight-alpha** entry point used for normal decoded-image
    /// draws, whose samples carry straight (non-premultiplied) alpha. Offscreen
    /// *layer* composites must instead use
    /// [`flush_texture_batch_premultiplied`](Self::flush_texture_batch_premultiplied),
    /// because their texels are premultiplied — see that method and
    /// `WgpuPainter::flush_opacity_layer`.
    ///
    /// `scissor` is the clip rect to apply for this draw call.  Pass `None` to
    /// render unclipped (full viewport), matching the behaviour of the
    /// rect/circle instanced batches when no scissor is active.
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `queue` - wgpu queue
    /// * `pipelines` - pipeline collection (selects the straight-alpha pipeline)
    /// * `viewport_bind_group` - group 0 viewport uniform bind group
    /// * `unit_quad_buffer` - shared unit-quad vertex buffer
    /// * `unit_quad_index_buffer` - shared unit-quad index buffer
    /// * `default_sampler` - linear/clamp-to-edge sampler
    /// * `resources` - GPU resource managers (buffer pool used for instance upload)
    /// * `viewport_size` - current viewport `(width, height)` in physical pixels
    /// * `encoder` - command encoder
    /// * `view` - render target view
    /// * `texture_view` - texture to use for all instances in this batch
    /// * `scissor` - optional scissor rect `(x, y, w, h)` in physical pixels
    pub(super) fn flush_texture_batch(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        viewport_bind_group: &wgpu::BindGroup,
        unit_quad_buffer: &wgpu::Buffer,
        unit_quad_index_buffer: &wgpu::Buffer,
        default_sampler: &wgpu::Sampler,
        resources: &mut GpuResources,
        viewport_size: (u32, u32),
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
    ) {
        self.flush_texture_batch_with_blend(
            device,
            queue,
            pipelines,
            viewport_bind_group,
            unit_quad_buffer,
            unit_quad_index_buffer,
            default_sampler,
            resources,
            viewport_size,
            encoder,
            view,
            texture_view,
            scissor,
            false,
        );
    }

    /// Flush the texture instance batch using **premultiplied** source-over
    /// blending.
    ///
    /// Used to composite offscreen *layer* textures (opacity / ColorFilter /
    /// ShaderMask / backdrop results). Those offscreens are cleared transparent
    /// then drawn into with straight `ALPHA_BLENDING`, which leaves their texels
    /// premultiplied (`rgb = straight_rgb * a`). Compositing them with the
    /// straight pipeline would re-multiply rgb by alpha, darkening translucent/AA
    /// content. This routes the batch through
    /// [`PipelineSet::instanced_texture_premul`] (src factor `One`) so the
    /// composite is correct, with the per-channel `tint` carrying group opacity
    /// and any ColorFilter chroma as `(C.r*O, C.g*O, C.b*O, O)`.
    pub(super) fn flush_texture_batch_premultiplied(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        viewport_bind_group: &wgpu::BindGroup,
        unit_quad_buffer: &wgpu::Buffer,
        unit_quad_index_buffer: &wgpu::Buffer,
        default_sampler: &wgpu::Sampler,
        resources: &mut GpuResources,
        viewport_size: (u32, u32),
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
    ) {
        self.flush_texture_batch_with_blend(
            device,
            queue,
            pipelines,
            viewport_bind_group,
            unit_quad_buffer,
            unit_quad_index_buffer,
            default_sampler,
            resources,
            viewport_size,
            encoder,
            view,
            texture_view,
            scissor,
            true,
        );
    }

    /// Shared body for [`Self::flush_texture_batch`] and
    /// [`Self::flush_texture_batch_premultiplied`].
    ///
    /// `premultiplied` selects the blend pipeline: `false` =
    /// straight-alpha (decoded images), `true` = premultiplied source-over
    /// (offscreen-layer composites).
    fn flush_texture_batch_with_blend(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        viewport_bind_group: &wgpu::BindGroup,
        unit_quad_buffer: &wgpu::Buffer,
        unit_quad_index_buffer: &wgpu::Buffer,
        default_sampler: &wgpu::Sampler,
        resources: &mut GpuResources,
        viewport_size: (u32, u32),
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
        premultiplied: bool,
    ) {
        if self.texture_batch.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "GpuReplay::flush_texture_batch: {} instances",
            self.texture_batch.len()
        );

        // Create texture bind group for this batch
        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Texture Instance Bind Group"),
            layout: &pipelines.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(default_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
            ],
        });

        // Upload instance buffer (using buffer pool for efficient zero-copy reuse)
        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "Texture Instance Buffer",
            self.texture_batch.as_bytes(),
        );

        // Create render pass
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Instanced Texture Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load, // Don't clear - render on top
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        // Set pipeline and buffers. Offscreen-layer composites use the
        // premultiplied pipeline; normal decoded-image draws use straight alpha.
        // Selection logic is behavior-preserving (round-5c color-correctness fix).
        let pipeline = if premultiplied {
            &pipelines.instanced_texture_premul
        } else {
            &pipelines.instanced_texture
        };
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, viewport_bind_group, &[]);
        render_pass.set_bind_group(1, &texture_bind_group, &[]);
        render_pass.set_vertex_buffer(0, unit_quad_buffer.slice(..));
        render_pass.set_vertex_buffer(1, instance_buffer.slice(..));
        render_pass.set_index_buffer(unit_quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        // Apply scissor rect, mirroring the rect/circle/arc instanced batch pattern.
        if let Some((x, y, w, h)) = scissor {
            render_pass.set_scissor_rect(x, y, w, h);
        } else {
            render_pass.set_scissor_rect(0, 0, viewport_size.0, viewport_size.1);
        }

        // Draw all instances in ONE draw call.
        render_pass.draw_indexed(0..6, 0, 0..self.texture_batch.len() as u32);

        drop(render_pass);

        // Clear batch for next frame
        self.texture_batch.clear();
    }
}

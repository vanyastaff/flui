//! Offscreen-layer render helper for opacity and compositing layers.
//!
//! This module provides `GpuReplay::render_layer_to_offscreen`, extracted
//! from the `flush_opacity_layer` body so the renderer driver can reuse the
//! same routine for both standard opacity compositing and backdrop-read compositing.
//!
//! ## Invariants preserved from `flush_opacity_layer`
//!
//! - **R1** — arm order (Segment / OffscreenTexture / OpacityLayer) is
//!   load-bearing; it is preserved verbatim.
//! - **R2** — `texture_batch` drain: every `flush_texture_batch*` call drains
//!   and clears `self.texture_batch` before returning so depth-N+1 content
//!   cannot leak into depth-N.
//! - **R3** — `LoadOp::Clear(TRANSPARENT)` on the offscreen pass; all inner
//!   passes use `LoadOp::Load`.

use std::sync::Arc;

use super::{
    command_ir::{DrawItem, PendingOpacityLayer},
    pipelines::PipelineSet,
    render_target::RenderTarget,
    replay::GpuReplay,
    resources::GpuResources,
    texture_pool::PooledTexture,
};

#[allow(clippy::too_many_arguments)]
impl GpuReplay {
    /// Render a pending opacity layer's content to a pooled offscreen texture.
    ///
    /// Acquires an offscreen texture from the pool, clears it to transparent,
    /// then flushes all items in `layer` (including the `final_segment`) to that
    /// texture.  Returns the acquired [`PooledTexture`]; the caller is responsible
    /// for compositing it onto the parent target and dropping it (which returns it
    /// to the pool).
    ///
    /// ## Caller contract
    ///
    /// The caller (`flush_opacity_layer`) must composite the returned texture onto
    /// the parent surface before dropping it.  Dropping without compositing is not
    /// a correctness error — the texture simply returns to the pool — but would
    /// produce a blank opacity layer.
    ///
    /// ## R3 — `LoadOp` correctness
    ///
    /// The offscreen clear pass uses `LoadOp::Clear(TRANSPARENT)` so only
    /// actually-drawn pixels contribute to the composite.  All inner render
    /// passes (inside `flush_segment`, `flush_texture_batch*`) use
    /// `LoadOp::Load`, preserving prior offscreen content.
    pub(in crate::wgpu) fn render_layer_to_offscreen(
        &mut self,
        layer: &mut PendingOpacityLayer,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> PooledTexture {
        let (vp_w, vp_h) = viewport_size;

        // Acquire a pooled offscreen texture for the layer's content.
        let offscreen = resources
            .layer_texture_pool_mut()
            .acquire(vp_w, vp_h, surface_format);
        let offscreen_view = offscreen.view();

        // R3: Clear the offscreen target to fully transparent BEFORE drawing
        // any layer content.  LoadOp::Clear here; all subsequent passes use Load.
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Opacity Layer Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: offscreen_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Pass dropped immediately — just clearing.
        }

        // Flush all inner draw items to the offscreen texture.
        // R1: arm order is preserved (Segment / OffscreenTexture / OpacityLayer).
        let offscreen_target = RenderTarget::view_only(offscreen_view);
        for item in layer.items.drain(..) {
            match item {
                DrawItem::Segment(mut seg) => {
                    self.flush_segment(
                        &mut seg,
                        viewport_size,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        offscreen_view,
                    );
                }
                DrawItem::OffscreenTexture(p) => {
                    // A nested OffscreenTexture (shader-mask / backdrop-blur
                    // result) is itself premultiplied — composite with the
                    // premultiplied pipeline and an identity tint so it is not
                    // re-multiplied by its own alpha.
                    let instance = super::instancing::TextureInstance::new(
                        p.bounds,
                        flui_types::styling::Color::WHITE,
                    );
                    let _ = self.texture_batch.add(instance);
                    // R2: flush_texture_batch_premultiplied drains + clears
                    // texture_batch before returning.
                    self.flush_texture_batch_premultiplied(
                        device,
                        queue,
                        pipelines,
                        resources,
                        viewport_size,
                        encoder,
                        offscreen_view,
                        p.texture.view(),
                        None,
                    );
                }
                DrawItem::OpacityLayer(nested) => {
                    // Recursively handle nested opacity layers.
                    // R2: &mut self serializes access to texture_batch.
                    self.flush_opacity_layer(
                        nested,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        offscreen_target,
                    );
                }
            }
        }

        // Flush the final segment (content drawn after the last draw-order item).
        if !layer.final_segment.is_empty() {
            self.flush_segment(
                &mut layer.final_segment,
                viewport_size,
                device,
                queue,
                pipelines,
                resources,
                encoder,
                offscreen_view,
            );
        }

        offscreen
    }
}

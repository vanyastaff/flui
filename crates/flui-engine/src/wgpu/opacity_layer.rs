//! Offscreen-layer render helpers for opacity and compositing layers.
//!
//! This module provides two `GpuReplay` methods:
//!
//! - `GpuReplay::render_segment_to_offscreen` — renders a single
//!   `DrawSegment` into a fresh pooled texture.  Used by the
//!   `DrawItem::AdvancedShape` arm in `submit` to build the foreground for
//!   `flush_advanced_layer`.
//!
//! - `GpuReplay::render_layer_to_offscreen` — renders all items in a
//!   `PendingOpacityLayer` (including its `final_segment`) into a pooled
//!   texture.  Called by `GpuReplay::flush_opacity_layer`.  It flushes the
//!   layer's full draw-item list (not a single segment), so it keeps its own
//!   `LoadOp::Clear(TRANSPARENT)` pass rather than delegating to
//!   `render_segment_to_offscreen`; both paths use the identical
//!   clear-then-`LoadOp::Load` sequence (R3).
//!
//! ## Invariants preserved from `flush_opacity_layer`
//!
//! - **R1** — arm order (Segment / OffscreenTexture / OpacityLayer /
//!   AdvancedShape) is load-bearing; it is preserved verbatim.
//! - **R2** — `texture_batch` drain: every `flush_texture_batch*` call drains
//!   and clears `self.texture_batch` before returning so depth-N+1 content
//!   cannot leak into depth-N.
//! - **R3** — `LoadOp::Clear(TRANSPARENT)` on every offscreen pass; all inner
//!   passes use `LoadOp::Load`.

use std::sync::Arc;

use super::{
    advanced_blend::{AdvancedBlendOp, flush_advanced_layer},
    command_ir::{DrawItem, DrawSegment, PendingOpacityLayer},
    pipelines::PipelineSet,
    render_target::RenderTarget,
    replay::GpuReplay,
    resources::GpuResources,
    texture_pool::PooledTexture,
};

#[allow(clippy::too_many_arguments)]
impl GpuReplay {
    /// Render a single [`DrawSegment`] into a fresh full-viewport pooled
    /// offscreen texture.
    ///
    /// This is the primitive building block used by both:
    /// - [`Self::render_layer_to_offscreen`] (clear pass for the layer texture),
    /// - [`GpuReplay::submit`] for [`DrawItem::AdvancedShape`] (foreground texture).
    ///
    /// ## R3 — `LoadOp` correctness
    ///
    /// The offscreen texture is cleared to `TRANSPARENT` before `flush_segment`
    /// writes into it.  Only actually-drawn pixels carry non-zero alpha, which
    /// is required for correct premultiplied compositing in
    /// `flush_advanced_layer`.
    ///
    /// ## Caller contract
    ///
    /// The returned [`PooledTexture`] is RAII: returning to the pool on drop.
    /// The caller must composite or otherwise use the texture before dropping it.
    pub(in crate::wgpu) fn render_segment_to_offscreen(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> PooledTexture {
        let (vp_w, vp_h) = viewport_size;

        // Acquire a full-viewport pooled texture for the foreground.
        let offscreen = resources
            .layer_texture_pool_mut()
            .acquire(vp_w, vp_h, surface_format);
        let offscreen_view = offscreen.view();

        // R3: Clear the offscreen target to fully transparent before drawing.
        // This guarantees that pixels outside the shape are transparent and do
        // not contribute spurious foreground colour to the advanced-blend pass.
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Advanced Shape Offscreen Clear Pass"),
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

        // Render the segment into the cleared offscreen texture.
        self.flush_segment(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            offscreen_view,
        );

        offscreen
    }

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
        // R1: arm order is preserved (Segment / OffscreenTexture / OpacityLayer
        //     / AdvancedShape).
        //
        // Use `sampleable` so a nested advanced-blend OpacityLayer or
        // AdvancedShape can dst-read this offscreen as its backdrop (DECISION 2).
        // The pool allocates offscreen textures with TEXTURE_BINDING | COPY_SRC |
        // RENDER_ATTACHMENT so `sampleable` is always valid here.
        let offscreen_target = RenderTarget::sampleable(offscreen_view, offscreen.texture());
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
                DrawItem::AdvancedShape(mut op) => {
                    // An advanced shape nested inside a layer: the backdrop is the
                    // offscreen_target (pool texture with COPY_SRC — DECISION 2).
                    // `flush_advanced_layer` copies the backdrop from
                    // `offscreen_target.texture` (always Some for pool targets).
                    if let Some(backdrop_texture) = offscreen_target.texture {
                        let foreground = self.render_segment_to_offscreen(
                            &mut op.segment,
                            viewport_size,
                            surface_format,
                            device,
                            queue,
                            pipelines,
                            resources,
                            encoder,
                        );
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "vp_w/vp_h are u32 viewport dims; \
                                      precision loss at >16 M pixels is acceptable"
                        )]
                        let viewport_width_f32 = vp_w as f32;
                        let viewport_height_f32 = vp_h as f32;
                        let blend_op = AdvancedBlendOp {
                            foreground,
                            mode: op.mode,
                            device_bounds: op.device_bounds,
                            // Identity tint + full opacity: shape color/alpha is
                            // already baked into the premul vertex colors.
                            opacity: 1.0,
                            tint: [1.0, 1.0, 1.0],
                            src_uv_min: [
                                op.device_bounds.left().0 / viewport_width_f32,
                                op.device_bounds.top().0 / viewport_height_f32,
                            ],
                            src_uv_max: [
                                op.device_bounds.right().0 / viewport_width_f32,
                                op.device_bounds.bottom().0 / viewport_height_f32,
                            ],
                        };
                        flush_advanced_layer(
                            blend_op,
                            backdrop_texture,
                            offscreen_view,
                            surface_format,
                            viewport_size,
                            &pipelines.advanced_blend,
                            resources,
                            device,
                            encoder,
                        );
                        tracing::trace!(
                            mode = ?op.mode,
                            bounds = ?op.device_bounds,
                            "GpuReplay: advanced shape composited onto layer offscreen"
                        );
                    } else {
                        // Offscreen target lacks COPY_SRC — this cannot happen for
                        // pool textures (they always have COPY_SRC); defensive fallback.
                        tracing::warn!(
                            mode = ?op.mode,
                            "Advanced shape inside layer: offscreen target lacks COPY_SRC; \
                             falling back to SrcOver (invariant violation — pool textures \
                             should always have COPY_SRC)"
                        );
                        self.flush_segment(
                            &mut op.segment,
                            viewport_size,
                            device,
                            queue,
                            pipelines,
                            resources,
                            encoder,
                            offscreen_view,
                        );
                    }
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

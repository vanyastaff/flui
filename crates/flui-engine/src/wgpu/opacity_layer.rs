//! Offscreen-layer render helpers for opacity and compositing layers.
//!
//! This module provides three `GpuReplay` methods:
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
//! - `GpuReplay::flush_opacity_layer` — composite a rendered layer onto the
//!   main surface, dispatching to the advanced-blend path or the premultiplied
//!   SrcOver path as appropriate.  Moved here from `replay.rs` to keep that
//!   file under the 1500-LOC spec limit.
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
                // ── SSAA-supersampled path nested inside a layer ───────────────
                DrawItem::SsaaPath(mut op) => {
                    // Composite the SSAA tile onto the layer's offscreen texture
                    // at the correct Z position (R1 arm order).
                    self.render_ssaa_path(
                        &mut op,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        offscreen_view,
                    );
                    tracing::trace!(
                        bounds = ?op.device_bounds,
                        "GpuReplay: SSAA path tile composited onto layer offscreen"
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

    // =========================================================================
    // Opacity-layer composite (moved from replay.rs to honour the 1500-LOC limit)
    // =========================================================================

    /// Render an opacity layer's content to an offscreen texture and composite
    /// the result onto `main_target` with group opacity.
    ///
    /// Correct group opacity: all children are rendered at full opacity into an
    /// offscreen texture, then the entire texture is composited with the layer's
    /// alpha. This avoids the incorrect per-primitive alpha that would result
    /// from blending each child independently.
    ///
    /// ## R3 — LoadOp correctness
    ///
    /// - The offscreen clear pass uses `LoadOp::Clear(TRANSPARENT)` so only
    ///   actually-drawn pixels contribute to the composite.
    /// - All other render passes (flush_segment, flush_texture_batch) use
    ///   `LoadOp::Load` — preserving prior content in the target.
    ///   A Clear↔Load swap here would blank or ghost a layer.
    ///
    /// ## R2 — `texture_batch` invariant in recursion
    ///
    /// `&mut self` serializes the recursion.  Every `flush_texture_batch*`
    /// call drains and clears `self.texture_batch` before returning, so a
    /// depth-N+1 flush cannot leave instances that appear in the depth-N
    /// composite.
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub(in crate::wgpu) fn flush_opacity_layer(
        &mut self,
        mut layer: PendingOpacityLayer,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        main_target: RenderTarget<'_>,
    ) {
        // Zero-size viewports produce no visible pixels; skip GPU work entirely.
        // The UV composite below divides by vp_w/vp_h, so proceeding would push
        // inf/NaN into texture instances.  The pool clamps acquire to 1×1 but
        // the resulting composite would be meaningless.
        let (vp_w, vp_h) = viewport_size;
        if vp_w == 0 || vp_h == 0 {
            return;
        }

        let offscreen = self.render_layer_to_offscreen(
            &mut layer,
            viewport_size,
            surface_format,
            device,
            queue,
            pipelines,
            resources,
            encoder,
        );

        // ── Advanced-blend dispatch (DECISION 1 / CRITICAL GATE) ────────────
        //
        // `layer.blend.is_advanced()` is set when the layer carries a W3C
        // advanced blend mode (Multiply, Screen, Overlay, …, Luminosity).
        // Advanced blends require a backdrop read from the main surface — they
        // MUST NOT go through the SrcOver premultiplied composite below, which
        // would produce a white-over-dst tinted composite instead of the actual
        // advanced blend.
        //
        // The COPY_SRC guard: `main_target.texture` is `Some` only when the
        // surface was created with COPY_SRC usage.  Without it `copy_backdrop_region`
        // cannot copy the backdrop, so we fall back to SrcOver + a one-shot warning.
        if layer.blend.is_advanced() {
            if let Some(surface_texture) = main_target.texture {
                let o = layer.opacity.clamp(0.0, 1.0);
                // UV remap: layer.bounds → [0,1] in viewport space.
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
                )]
                let uv_left = layer.bounds.left().0 / vp_w as f32;
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
                )]
                let uv_top = layer.bounds.top().0 / vp_h as f32;
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
                )]
                let uv_right = layer.bounds.right().0 / vp_w as f32;
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
                )]
                let uv_bottom = layer.bounds.bottom().0 / vp_h as f32;

                let op = AdvancedBlendOp {
                    foreground: offscreen,
                    mode: layer.blend,
                    device_bounds: layer.bounds,
                    opacity: o,
                    tint: layer.tint_rgb,
                    src_uv_min: [uv_left, uv_top],
                    src_uv_max: [uv_right, uv_bottom],
                };
                flush_advanced_layer(
                    op,
                    surface_texture,
                    main_target.view,
                    surface_format,
                    viewport_size,
                    &pipelines.advanced_blend,
                    resources,
                    device,
                    encoder,
                );
                tracing::trace!(
                    mode = ?layer.blend,
                    opacity = layer.opacity,
                    bounds = ?layer.bounds,
                    "GpuReplay: composited advanced-blend opacity layer"
                );
                return;
            }
            // A `view_only` target has no sampleable backdrop; advanced modes
            // degrade to SrcOver here (warn once).  Production producers pass a
            // sampleable target — surface-with-COPY_SRC, the COPY_SRC-less
            // intermediate, or a pooled offscreen — so this is only reached by
            // genuinely view-only callers (benches/headless/ShaderMask-style).
            tracing::warn!(
                mode = ?layer.blend,
                "Advanced blend layer reached a view_only target; \
                 falling back to SrcOver compositing (caller must pass sampleable target)"
            );
        }

        let offscreen_view = offscreen.view();

        // Composite the premultiplied offscreen onto the main surface.
        //
        // The offscreen texel `T` is premultiplied: `T.rgb = straight_rgb * a`,
        // `T.a = a`.  With group opacity `O` and ColorFilter chroma `C`
        // (white = no-op), the correct result is premultiplied source-over of
        // `T * (C.r*O, C.g*O, C.b*O, O)` — every premultiplied channel scaled
        // by its tint, then OVER the destination.  The shader applies
        // `tex * tint` and the premultiplied pipeline (src factor `One`)
        // performs the OVER.
        //
        // - White tint, O<1  → tint (O,O,O,O): uniform group opacity (BUG 2 fix).
        // - Chroma tint       → modulates hue while preserving premultiplication
        //                       (BUG 3 fix).
        let o = layer.opacity.clamp(0.0, 1.0);
        let tint = [
            layer.tint_rgb[0] * o,
            layer.tint_rgb[1] * o,
            layer.tint_rgb[2] * o,
            o,
        ];

        // Use layer bounds as the destination rect; UV coordinates map the
        // bounds region from the full-viewport texture.
        #[allow(
            clippy::cast_precision_loss,
            reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
        )]
        let uv_left = layer.bounds.left().0 / vp_w as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
        )]
        let uv_top = layer.bounds.top().0 / vp_h as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
        )]
        let uv_right = layer.bounds.right().0 / vp_w as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "vp_w/vp_h are u32 viewport dims; precision loss at >16 M pixels is acceptable"
        )]
        let uv_bottom = layer.bounds.bottom().0 / vp_h as f32;

        let instance = super::instancing::TextureInstance::with_uv_tint_f32(
            layer.bounds,
            [uv_left, uv_top, uv_right, uv_bottom],
            tint,
        );
        let _ = self.texture_batch.add(instance);
        // Opacity-layer composite onto main surface — full-viewport, no scissor.
        // Premultiplied: offscreen texels are premultiplied (see above).
        // R2: flush_texture_batch_premultiplied drains + clears texture_batch.
        self.flush_texture_batch_premultiplied(
            device,
            queue,
            pipelines,
            resources,
            viewport_size,
            encoder,
            main_target.view,
            offscreen_view,
            None,
        );

        tracing::trace!(
            opacity = layer.opacity,
            bounds = ?layer.bounds,
            "GpuReplay: composited opacity layer"
        );

        // offscreen texture returned to pool when `offscreen` is dropped here.
    }
}

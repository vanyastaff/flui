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
//! - **R1** — arm order (Segment / Backdrop / OpacityLayer /
//!   AdvancedShape) is load-bearing; it is preserved verbatim.
//! - **R2** — `texture_batch` drain: every `flush_texture_batch*` call drains
//!   and clears `self.texture_batch` before returning so depth-N+1 content
//!   cannot leak into depth-N.
//! - **R3** — `LoadOp::Clear(TRANSPARENT)` on every offscreen pass; all inner
//!   passes use `LoadOp::Load`.

use std::sync::Arc;

use crate::error::EngineResult;

use crate::{
    advanced_blend::{AdvancedBlendOp, flush_advanced_layer},
    blur::apply_blur,
    color_matrix::apply_color_matrix,
    command_ir::{
        DrawItem, DrawSegment, ImageFilterPass, LayerFilter, LayerFilterChain, PendingOpacityLayer,
    },
    gamma::apply_gamma,
    mode::apply_mode,
    morphology::apply_morphology,
    pipeline_set::PipelineSet,
    render_target::RenderTarget,
    replay::GpuReplay,
    resources::GpuResources,
    texture_pool::PooledTexture,
};

#[expect(clippy::too_many_arguments)]
impl GpuReplay {
    /// Replay in a bounded root-space working domain. Each target binding is
    /// immutable, so nested effects can change the attachment without changing
    /// bindings already captured by encoded work. Restore CPU state on refusal.
    pub(crate) fn render_filter_input(
        &mut self,
        op: &mut crate::command_ir::FilterOp,
        _viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<PooledTexture> {
        resources.admit_foreground_filter(
            op.fb_dim,
            surface_format,
            &op.passes,
            &op.input.budget,
        )?;
        let previous_depth = self.filter_attachment_depth;
        self.filter_attachment_depth += 1;
        let previous_origin = std::mem::replace(&mut self.attachment_origin, op.fb_origin);
        let previous_size = std::mem::replace(&mut self.uniform_size, op.fb_dim);
        let previous_binding = self.viewport_bind_group.clone();
        let result = if op.items.is_empty() {
            self.render_segment_to_offscreen(
                &op.input,
                op.fb_dim,
                surface_format,
                device,
                queue,
                pipelines,
                resources,
                encoder,
            )
        } else {
            let mut layer = PendingOpacityLayer {
                items: std::mem::take(&mut op.items),
                final_segment: {
                    let replacement = op.input.empty_sibling();
                    std::mem::replace(&mut op.input, replacement.seal())
                },
                opacity: 1.0,
                tint_rgb: [1.0; 3],
                bounds: op.input_support,
                blend: flui_painting::paint::BlendMode::SrcOver,
                filters: LayerFilterChain::default(),
                composite_clip: None,
            };
            self.render_layer_to_offscreen(
                &mut layer,
                op.fb_dim,
                surface_format,
                device,
                queue,
                pipelines,
                resources,
                encoder,
            )
        };
        self.filter_attachment_depth = previous_depth;
        self.attachment_origin = previous_origin;
        self.uniform_size = previous_size;
        self.viewport_bind_group = previous_binding;
        result
    }

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
    pub(crate) fn render_segment_to_offscreen(
        &mut self,
        segment: &DrawSegment,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<PooledTexture> {
        let (vp_w, vp_h) = viewport_size;
        if self.filter_attachment_depth != 0 {
            resources.admit_foreground_target(viewport_size, surface_format, 1)?;
        }

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
            RenderTarget::sampleable(offscreen_view, offscreen.texture()),
        )?;

        Ok(offscreen)
    }

    /// Compose the supersample tile mapping into analytic clips. Foreground
    /// filters keep geometry and analytic clips in root space and use the
    /// attachment origin in the viewport binding instead.
    pub(super) fn remap_segment_clips(
        segment: &mut crate::command_ir::DrawSegment,
        origin: (f32, f32),
        scale: (f32, f32),
    ) {
        for inst in &mut segment.rect_batch.instances {
            Self::rebase_clip(
                &mut inst.clip_device_to_local,
                &mut inst.clip_local_origin,
                inst.clip_kind,
                origin,
                scale,
            );
        }
        for inst in &mut segment.circle_batch.instances {
            Self::rebase_clip(
                &mut inst.clip_device_to_local,
                &mut inst.clip_local_origin,
                inst.clip_kind,
                origin,
                scale,
            );
        }
        // Tessellated geometry carries its clip on the BATCH, not on instances
        // — and unlike shadows/gradients/images it IS repositioned into a
        // shrunken intermediate (`content_aabb` returns `Some` for a
        // vertices-only segment), so it needs the rebase just as much.
        for batch in &mut segment.tess_batches {
            let mut m = [
                batch.clip.device_to_local[0],
                batch.clip.device_to_local[1],
                batch.clip.device_to_local[2],
                batch.clip.device_to_local[3],
            ];
            let mut t = [
                batch.clip.device_to_local[4],
                batch.clip.device_to_local[5],
                0.0,
                0.0,
            ];
            Self::rebase_clip(&mut m, &mut t, batch.clip.kind, origin, scale);
            batch.clip.device_to_local = [m[0], m[1], m[2], m[3], t[0], t[1]];
        }
    }

    /// Compose a framebuffer rebase into a clip's device-to-local mapping.
    ///
    /// The intermediate's fragments arrive at `p' = (p − origin) · scale`, so
    /// the mapping that used to consume `p` must now consume `p'`:
    ///
    /// ```text
    /// p     = p' / scale + origin
    /// local = M · p + t
    ///       = (M · diag(1/scale)) · p' + (M · origin + t)
    /// ```
    ///
    /// Composing rather than moving the bounds is what makes this exact under
    /// rotation: there is no AABB step to lose the orientation in.
    fn rebase_clip(
        device_to_local: &mut [f32; 4],
        local_origin: &mut [f32; 4],
        clip_kind: [u32; 4],
        origin: (f32, f32),
        scale: (f32, f32),
    ) {
        if clip_kind[0] == 0 {
            return;
        }
        let (origin_x, origin_y) = origin;
        let (scale_x, scale_y) = scale;
        let [a, b, c, d] = *device_to_local;

        // t' = M · origin + t, using the PRE-scale columns.
        local_origin[0] += a * origin_x + c * origin_y;
        local_origin[1] += b * origin_x + d * origin_y;

        // M' = M · diag(1/sx, 1/sy) — scale each column by its own axis.
        if scale_x != 0.0 {
            device_to_local[0] = a / scale_x;
            device_to_local[1] = b / scale_x;
        }
        if scale_y != 0.0 {
            device_to_local[2] = c / scale_y;
            device_to_local[3] = d / scale_y;
        }
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
    ///
    /// Text inside the layer is a batch of its segment, so it is flushed
    /// into the offscreen with the segment and composited with the layer.
    pub(crate) fn render_layer_to_offscreen(
        &mut self,
        layer: &mut PendingOpacityLayer,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<PooledTexture> {
        let (vp_w, vp_h) = viewport_size;
        if self.filter_attachment_depth != 0 {
            resources.admit_foreground_target(viewport_size, surface_format, 1)?;
        }

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
        // R1: arm order is preserved (Segment / Backdrop / OpacityLayer
        //     / AdvancedShape).
        //
        // Use `sampleable` so a nested advanced-blend OpacityLayer or
        // AdvancedShape can dst-read this offscreen as its backdrop.
        // The pool allocates offscreen textures with TEXTURE_BINDING | COPY_SRC |
        // RENDER_ATTACHMENT so `sampleable` is always valid here.
        let offscreen_target = RenderTarget::sampleable(offscreen_view, offscreen.texture());
        for item in layer.items.drain(..) {
            match item {
                DrawItem::Backdrop(op) => self.replay_backdrop(
                    op,
                    viewport_size,
                    surface_format,
                    device,
                    queue,
                    pipelines,
                    resources,
                    encoder,
                    offscreen_target,
                )?,
                DrawItem::Segment(seg) => {
                    self.flush_segment(
                        &seg,
                        viewport_size,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        offscreen_target,
                    )?;
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
                    )?;
                }
                DrawItem::AdvancedShape(op) => {
                    // An advanced shape nested inside a layer: the backdrop is the
                    // offscreen_target (pool texture with COPY_SRC ).
                    // `flush_advanced_layer` copies the backdrop from
                    // `offscreen_target.texture` (always Some for pool targets).
                    if let Some(backdrop_texture) = offscreen_target.texture {
                        let foreground = self.render_segment_to_offscreen(
                            &op.segment,
                            viewport_size,
                            surface_format,
                            device,
                            queue,
                            pipelines,
                            resources,
                            encoder,
                        )?;
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
                                ((op.device_bounds.left() - self.attachment_origin.0 as f64)
                                    / f64::from(viewport_width_f32))
                                    as f32,
                                ((op.device_bounds.top() - self.attachment_origin.1 as f64)
                                    / f64::from(viewport_height_f32))
                                    as f32,
                            ],
                            src_uv_max: [
                                ((op.device_bounds.right() - self.attachment_origin.0 as f64)
                                    / f64::from(viewport_width_f32))
                                    as f32,
                                ((op.device_bounds.bottom() - self.attachment_origin.1 as f64)
                                    / f64::from(viewport_height_f32))
                                    as f32,
                            ],
                            clip: None,
                        };
                        self.prepare_viewport_binding(device, pipelines, resources)?;
                        self.admit_filter_composite(viewport_size, surface_format, resources)?;
                        flush_advanced_layer(
                            blend_op,
                            backdrop_texture,
                            offscreen_target.view,
                            surface_format,
                            viewport_size,
                            &pipelines.advanced_blend,
                            resources,
                            device,
                            encoder,
                            Some(&self.viewport_bind_group),
                            self.attachment_origin,
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
                            &op.segment,
                            viewport_size,
                            device,
                            queue,
                            pipelines,
                            resources,
                            encoder,
                            offscreen_target,
                        )?;
                    }
                }
                // ── Image-filter path nested inside a layer ───────────────────
                //
                // Z-order within the layer follows each item's `draw_order` position
                // and the replay loop — NOT match-arm textual position. The filter's
                // input segment is rendered to an isolated grown-bounds offscreen
                // (`fb_dim` sized, not full-viewport), the pass chain is folded,
                // and the result is composited onto the layer's offscreen_view at the
                // integer-grid dst_rect with full-texture src_uv = [0, 0, 1, 1].
                //
                // Deliberately no `_` arm: a new `Slice` variant must fail to
                // compile here rather than be silently skipped during folding.
                DrawItem::Filter(mut op) => {
                    // 1. Render content to the grown-bounds intermediate.
                    let content_tex = self.render_filter_input(
                        &mut op,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                    )?;
                    // 2. Fold the pass chain over the grown-bounds intermediate.
                    let filtered_tex = apply_image_filter_passes(
                        &op.passes,
                        content_tex,
                        op.input_support,
                        op.fb_origin,
                        op.fb_dim,
                        surface_format,
                        pipelines,
                        resources,
                        device,
                        encoder,
                    )?;
                    // Restrict the final composite, independently of the wider
                    // input/intermediate working frame. Integer texel-grid UVs
                    // preserve a 1:1 sample at fractional source boundaries.
                    let dst_rect = op.output_support;
                    let uv = [
                        ((dst_rect.left() - op.fb_origin.0 as f64) / f64::from(op.fb_dim.0)) as f32,
                        ((dst_rect.top() - op.fb_origin.1 as f64) / f64::from(op.fb_dim.1)) as f32,
                        ((dst_rect.right() - op.fb_origin.0 as f64) / f64::from(op.fb_dim.0))
                            as f32,
                        ((dst_rect.bottom() - op.fb_origin.1 as f64) / f64::from(op.fb_dim.1))
                            as f32,
                    ];
                    self.composite_group_texture(
                        filtered_tex,
                        dst_rect,
                        uv,
                        1.0,
                        [1.0; 3],
                        flui_painting::paint::BlendMode::SrcOver,
                        op.composite_clip.as_ref(),
                        &op.input,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        offscreen_target,
                        None,
                    )?;
                    tracing::trace!(
                        passes = op.passes.len(),
                        fb_origin = ?op.fb_origin,
                        fb_dim = ?op.fb_dim,
                        "GpuReplay: image-filter composited onto layer offscreen (grown-bounds)"
                    );
                }
                // ── SSAA-supersampled path nested inside a layer ───────────────
                DrawItem::SsaaPath(mut op) => {
                    // Composite the SSAA tile onto the layer's offscreen texture
                    // at the correct Z position (R1 arm order).
                    //
                    // Pass `offscreen_target.texture` (a pool texture with
                    // COPY_SRC | TEXTURE_BINDING ) so that advanced
                    // (dst-read) blend modes on SSAA paths nested inside a layer
                    // can dst-read the offscreen as their backdrop.  This mirrors
                    // the AdvancedShape arm above (lines 246-301) which also passes
                    // `offscreen_target.texture` for the same reason.
                    //
                    // The SSAA 1× tile is a SEPARATE pooled texture from the
                    // offscreen, so there is no read/write aliasing: the backdrop
                    // copy reads from `offscreen_target.texture` while the SSAA
                    // tile is written to the same offscreen via `offscreen_view`
                    // only AFTER the copy completes (sequential encoder commands).
                    self.render_ssaa_path(
                        &mut op,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        offscreen_target.view,
                        offscreen_target.texture, // sampleable pool texture for advanced dst-read
                    )?;
                    tracing::trace!(
                        mode = ?op.blend,
                        bounds = ?op.device_bounds,
                        "GpuReplay: SSAA path tile composited onto layer offscreen"
                    );
                }
            }
        }

        // Flush the final segment (content drawn after the last draw-order item).
        if !layer.final_segment.is_empty() {
            self.flush_segment(
                &layer.final_segment,
                viewport_size,
                device,
                queue,
                pipelines,
                resources,
                encoder,
                offscreen_target,
            )?;
        }

        Ok(offscreen)
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
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn flush_opacity_layer(
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
    ) -> EngineResult<()> {
        // Zero-size viewports produce no visible pixels; skip GPU work entirely.
        // The UV composite below divides by vp_w/vp_h, so proceeding would push
        // inf/NaN into texture instances.  The pool clamps acquire to 1×1 but
        // the resulting composite would be meaningless.
        let (vp_w, vp_h) = viewport_size;
        if vp_w == 0 || vp_h == 0 {
            return Ok(());
        }

        let layer_tex = self.render_layer_to_offscreen(
            &mut layer,
            viewport_size,
            surface_format,
            device,
            queue,
            pipelines,
            resources,
            encoder,
        )?;

        if self.filter_attachment_depth != 0 && !layer.filters.is_empty() {
            let work = (viewport_size.0 as usize)
                .checked_mul(viewport_size.1 as usize)
                .and_then(|pixels| pixels.checked_mul(layer.filters.len()))
                .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
            layer.final_segment.budget.admit_effect_work(work)?;
            resources.admit_foreground_target(
                viewport_size,
                surface_format,
                layer.filters.len(),
            )?;
            let metadata = layer
                .filters
                .len()
                .checked_mul(80)
                .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
            resources.reserve_prepared(crate::device_domain::PreparedCost {
                gpu_bytes: metadata,
                cpu_bytes: metadata,
                objects: layer
                    .filters
                    .len()
                    .checked_mul(6)
                    .ok_or(crate::error::EngineError::PreparedResourceOverflow)?,
            })?;
        }

        // ── Color-filter chain fold (ping-pong) ──────────────────────────────
        //
        // Fold `layer.filters` left-to-right over the offscreen texture:
        //
        // - Empty chain (common path): alias `layer_tex` with zero extra acquire
        //   (bit-exact fast path).
        // - Non-empty chain: ping-pong — each pass acquires its own destination,
        //   reads `acc` as source, then `acc = next` drops the prior texture back
        //   to the pool. At most 2 live textures at any instant regardless of N.
        //
        // No `_` catch-all: the compiler forces new match arms when Slice 2/3
        // add `LayerFilter::Mode`/`LayerFilter::Gamma` variants.
        let offscreen = fold_layer_filter_chain(
            &layer.filters,
            layer_tex,
            viewport_size,
            surface_format,
            pipelines,
            resources,
            device,
            encoder,
        );

        let local_bounds = flui_foundation::geometry::Rect::from_ltrb(
            layer.bounds.left() - self.attachment_origin.0 as f64,
            layer.bounds.top() - self.attachment_origin.1 as f64,
            layer.bounds.right() - self.attachment_origin.0 as f64,
            layer.bounds.bottom() - self.attachment_origin.1 as f64,
        );
        let uv = [
            (local_bounds.left() / f64::from(vp_w)) as f32,
            (local_bounds.top() / f64::from(vp_h)) as f32,
            (local_bounds.right() / f64::from(vp_w)) as f32,
            (local_bounds.bottom() / f64::from(vp_h)) as f32,
        ];
        self.composite_group_texture(
            offscreen,
            layer.bounds,
            uv,
            layer.opacity,
            layer.tint_rgb,
            layer.blend,
            layer.composite_clip.as_ref(),
            &layer.final_segment,
            viewport_size,
            surface_format,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            main_target,
            None,
        )
    }
}

// ─── Color-filter chain fold ──────────────────────────────────────────────────

/// Fold a [`LayerFilterChain`] over `input_tex` left-to-right.
///
/// ## Fast-path (empty chain)
///
/// Returns `input_tex` by value with zero extra pool acquire — a bit-exact alias
/// of the pre-fold path (PERF-GATE: zero-acquire on the common path).
///
/// ## Non-empty chain (ping-pong)
///
/// Each pass acquires its own destination texture, reads `acc` as source, then
/// `acc = next` drops the prior texture back to the pool. At most 2 live textures
/// at any instant regardless of chain length N.
///
/// ## Exhaustiveness discipline
///
/// No `_ =>` catch-all arm: the compiler forces a new match arm when Slice 2/3
/// add `LayerFilter::Mode`/`LayerFilter::Gamma` variants.
#[expect(clippy::too_many_arguments)]
fn fold_layer_filter_chain(
    filters: &LayerFilterChain,
    input_tex: PooledTexture,
    viewport_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipelines: &mut crate::pipeline_set::PipelineSet,
    resources: &mut crate::resources::GpuResources,
    device: &std::sync::Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
    if filters.is_empty() {
        return input_tex;
    }
    let mut acc = input_tex;
    for filter in filters {
        let next = match filter {
            LayerFilter::ColorMatrix(matrix_values) => {
                tracing::trace!("fold_layer_filter_chain: applying ColorMatrix pass");
                apply_color_matrix(
                    *matrix_values,
                    &acc,
                    viewport_size,
                    surface_format,
                    &pipelines.color_matrix,
                    resources,
                    device,
                    encoder,
                )
            }
            LayerFilter::Mode { color, blend_mode } => {
                tracing::trace!(
                    blend_mode = ?blend_mode,
                    "fold_layer_filter_chain: applying Mode pass"
                );
                apply_mode(
                    *color,
                    *blend_mode,
                    &acc,
                    viewport_size,
                    surface_format,
                    &pipelines.mode,
                    resources,
                    device,
                    encoder,
                )
            }
            LayerFilter::Gamma(direction) => {
                tracing::trace!(
                    direction = ?direction,
                    "fold_layer_filter_chain: applying Gamma pass"
                );
                apply_gamma(
                    *direction,
                    &acc,
                    viewport_size,
                    surface_format,
                    &pipelines.gamma,
                    resources,
                    device,
                    encoder,
                )
            }
        };
        // `acc` (the source for this pass) is dropped here, returning to the pool.
        acc = next;
    }
    acc
}

// ─── Image-filter pass chain fold (DrawItem::Filter) ─────────────────────────

/// Apply a chain of [`ImageFilterPass`]es to `input_tex`, returning the result.
///
/// Folds the chain for [`DrawItem::Filter`] replay. Each arm acquires a fresh
/// destination texture sized to `fb_dim` (the integer-aligned grown bounds),
/// renders the pass, and drops the prior `acc` (returning it to the pool) —
/// the bounded ping-pong discipline, identical to `fold_layer_filter_chain`.
///
/// The match has **no `_ =>` catch-all**: Slice 4 (`Blur`) is compiler-forced to
/// add an arm when its variant is introduced.
///
/// ## Parameters shared across all arms
///
/// - `input_support` — AABB of the content in physical pixels; used by the
///   morphology/blur passes to compute the decal UV guard (samples outside
///   `input_support` return the neutral element rather than the clamped edge texel).
/// - `fb_origin` — integer-aligned top-left of the offscreen frame in device pixels.
///   Used by blur/morph to rebase `input_support` into `fb`-local UV coordinates
///   (`content_rect_uv = (input_support - fb_origin) / fb_dim`).
/// - `fb_dim` — integer dimensions of the intermediate textures; all pool acquires
///   use this size, and the `texture_size` uniform in blur/morph shaders is set to
///   `fb_dim` (the denominator must be integer fb_dim, not float
///   `grown.width()`).
/// - `viewport_size`, `surface_format`, `pipelines`, `resources`, `device`,
///   `encoder` — GPU context forwarded unchanged to every GPU pass arm.
#[expect(
    clippy::too_many_arguments,
    reason = "GPU pass fold threads device/encoder/pipeline/resources to every arm; \
              a context struct would add indirection without a semantic boundary"
)]
pub(crate) fn apply_image_filter_passes(
    passes: &[ImageFilterPass],
    input_tex: PooledTexture,
    input_support: flui_foundation::geometry::Rect<f64>,
    fb_origin: (i64, i64),
    fb_dim: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipelines: &mut PipelineSet,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> EngineResult<PooledTexture> {
    // Each pass consumes the preceding intermediate support, not the original
    // source bounds. The working frame is the bounded backwards dependency.
    let frame = flui_foundation::geometry::Rect::from_xywh(
        fb_origin.0 as f64,
        fb_origin.1 as f64,
        f64::from(fb_dim.0),
        f64::from(fb_dim.1),
    );
    let mut support = input_support;
    // Fold left-to-right; `acc` owns the current intermediate texture.
    let mut acc = input_tex;
    for pass in passes {
        acc = match pass {
            ImageFilterPass::Identity => acc, // no-op: pass texture through unchanged

            ImageFilterPass::Morph { radius, op } => {
                // Two separable sub-passes (H then V) inside apply_morphology.
                // Drops `acc` (the prior intermediate) before returning v_tex.
                apply_morphology(
                    *radius,
                    *op,
                    &acc,
                    support,
                    fb_origin,
                    fb_dim,
                    surface_format,
                    &pipelines.morphology,
                    resources,
                    device,
                    encoder,
                )
                // `acc` drops here after `apply_morphology` returns, returning
                // the prior intermediate to the pool.
            }

            ImageFilterPass::Blur { sigma_x, sigma_y } => {
                // Two separable sub-passes (H then V) inside apply_blur.
                // The H pass decals at current intermediate support in fb-local UV
                // (fb-local, not full-frame): samples outside contribute transparent black.
                // The V pass decals at the texture edge [0,1] to read the full H halo.
                apply_blur(
                    *sigma_x,
                    *sigma_y,
                    &acc,
                    support,
                    fb_origin,
                    fb_dim,
                    surface_format,
                    &pipelines.blur,
                    resources,
                    device,
                    encoder,
                )
                // `acc` drops here after `apply_blur` returns, returning the
                // prior intermediate (the content offscreen) to the pool.
            }

            ImageFilterPass::ColorMatrix(matrix) => {
                // Bounds-PRESERVING color-matrix pass (grows 0 px).
                //
                // The output texture is sized to `fb_dim` (same as the input),
                // cleared to TRANSPARENT before the pass writes into it (LoadOp::Clear
                // inside `apply_color_matrix`), so no prior halo leaks through.
                //
                // `acc` drops after `apply_color_matrix` returns, returning the
                // prior intermediate to the pool (bounded ping-pong preserved).
                apply_color_matrix(
                    *matrix,
                    &acc,
                    fb_dim,
                    surface_format,
                    &pipelines.color_matrix,
                    resources,
                    device,
                    encoder,
                )
            }
        };
        let (rx, ry) = pass.support_radius()?;
        support = flui_foundation::geometry::Rect::from_ltrb(
            support.left() - f64::from(rx),
            support.top() - f64::from(ry),
            support.right() + f64::from(rx),
            support.bottom() + f64::from(ry),
        )
        .intersect(&frame)
        .unwrap_or_default();
        if matches!(pass, ImageFilterPass::ColorMatrix(matrix) if matrix[19] > 0.0) {
            support = frame;
        }
    }
    Ok(acc)
}

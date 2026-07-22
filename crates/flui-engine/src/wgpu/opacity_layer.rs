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
    blur::apply_blur,
    color_matrix::apply_color_matrix,
    command_ir::{
        DrawItem, DrawSegment, ImageFilterPass, LayerFilter, LayerFilterChain, PendingOpacityLayer,
    },
    gamma::apply_gamma,
    mode::apply_mode,
    morphology::apply_morphology,
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

    /// Render a single [`DrawSegment`] into a **grown-bounds** pooled offscreen
    /// texture for a [`DrawItem::Filter`] intermediate.
    ///
    /// Unlike [`Self::render_segment_to_offscreen`] (which acquires a full-viewport
    /// texture), this method acquires a texture sized to `fb_dim` (the integer-
    /// aligned grown bounds computed in `painter::layer`'s `restore_layer`). The segment
    /// vertices are pre-transformed so that dividing by the UNCHANGED shared viewport
    /// uniform yields the correct NDC inside the `fb_dim` render target.
    ///
    /// ## Why not touch `render_segment_to_offscreen`
    ///
    /// That method is the advanced-shape hot path. Keeping it byte-identical is a
    /// structural bit-identity firewall — changes there would affect every
    /// `AdvancedShape` and `SsaaPath` arm. The grown-offscreen variant is a new
    /// method rather than a parameterised version (rule-of-three: only 2 callers,
    /// scale-1 vs scale-2 diverge in the scissor factor).
    ///
    /// ## Vertex pre-transform (non-negotiable #2)
    ///
    /// The shape shader computes `clip_x = (pos.x / vp_w) * 2 - 1` against the
    /// UNCHANGED shared viewport uniform `(vp_w, vp_h)`. We cannot hot-patch that
    /// uniform mid-encoder. A vertex at full-frame device pixel `(px, py)` must fill
    /// the `fb_dim` render target as if it were the whole viewport:
    ///
    /// ```text
    /// new_pos.x = (px - fb_origin.x) * (vp_w / fb_dim.x)
    /// new_pos.y = (py - fb_origin.y) * (vp_h / fb_dim.y)
    /// ```
    ///
    /// Denominator MUST be integer `fb_dim`, NOT float `grown.width()` (SSAA-tile
    /// bug class: they differ by one floor/ceil ulp on fractional regions).
    ///
    /// ## Scissor remap (non-negotiable #6, mirrors ssaa.rs:483-512 without ×2)
    ///
    /// Full-frame scissors are rebased to `fb_origin` and clamped to `[0, fb_dim]`.
    /// Empty-intersection uses the sentinel `(fb_dim.x, fb_dim.y, 1, 1)` (wgpu
    /// requires non-zero extent; the sentinel is outside the attachment → nothing
    /// drawn, matching the SSAA approach without the ×2 supersampling factor).
    ///
    /// Cross-reference: `ssaa.rs` `GpuReplay::render_ssaa_path` lines 443-512
    /// for the analogous full-frame→tile remap at 2× scale.
    pub(in crate::wgpu) fn render_segment_to_grown_offscreen(
        &mut self,
        segment: &mut DrawSegment,
        fb_origin: (u32, u32),
        fb_dim: (u32, u32),
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> PooledTexture {
        let (vp_w, vp_h) = viewport_size;
        let (fb_x, fb_y) = fb_origin;
        let (fb_w, fb_h) = fb_dim;

        // Acquire a pooled texture sized to the grown bounds (not the full viewport).
        let offscreen = resources
            .layer_texture_pool_mut()
            .acquire(fb_w, fb_h, surface_format);
        let offscreen_view = offscreen.view();

        // R3: Clear the offscreen target to fully transparent.
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Grown-Bounds Filter Offscreen Clear Pass"),
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

        // ── Vertex pre-transform (non-negotiable #2) ──────────────────────────
        //
        // Mirror ssaa.rs:443-461 with scale = vp / fb_dim (not vp / (tile * 2)):
        //   new_pos = (old_pos - fb_origin) * (vp / fb_dim_f32)
        //
        // Denominator = INTEGER fb_dim, NOT grown.width() — using float grown width
        // gives the wrong scale when grown_bounds is fractional (the SSAA-tile bug
        // class). The integer fb_dim is exactly what the pool acquired.
        #[allow(
            clippy::cast_precision_loss,
            reason = "fb_w/fb_h/vp_w/vp_h are u32 texture dims ≤ 16 M px; f32 precision \
                      is sufficient for device-pixel coordinate remapping"
        )]
        let scale_x = vp_w as f32 / fb_w as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "fb_h/vp_h are u32 texture dims ≤ 16 M px; f32 precision is sufficient"
        )]
        let scale_y = vp_h as f32 / fb_h as f32;
        let origin_x = fb_x as f32;
        let origin_y = fb_y as f32;

        let mut remapped_segment = segment.clone();
        for v in &mut remapped_segment.vertices {
            v.position[0] = (v.position[0] - origin_x) * scale_x;
            v.position[1] = (v.position[1] - origin_y) * scale_y;
        }

        // ── Instanced-batch pre-transform (extends vertex remap to rect/circle/arc) ──
        //
        // The rect shader computes NDC from instance.bounds.xy (baked-AABB path) or
        // instance.transform_translate.xy (affine path) using the STATIC viewport
        // uniform (original vp_w × vp_h).  To render at the correct fb-local
        // position in the fb_dim-sized texture the device-pixel position must be
        // remapped by the same formula as the tessellated vertices:
        //   new_pos = (old_pos - fb_origin) * (vp / fb_dim)
        //
        // The width/height must also scale (vp/fb_dim) because they contribute to
        // the device-pixel extent.  After the pre-transform the shader's NDC
        // computation yields the correct fb-local clip coordinate.
        //
        // Circle/arc: center lives in transform_translate; the transform 2×2 matrix
        // holds the radius-as-scale.  Rebasing transform_translate and scaling the
        // linear columns yields the correct fb-local center and radius.
        //
        // Rule-of-three note: this helper duplicates the SSAA vertex remap logic;
        // SSAA only touches tessellated vertices (its DrawSegment never holds rect
        // instances).  There are now 2 callers of this remapping pattern.  Extract
        // into a shared helper if a third caller appears.
        //
        // Clip rrect: stored in device-pixel space ([x, y, w, h, radii…]); apply
        // the same (pos-origin)*scale, dim*scale transform to x/y/w/h.
        // Identity-transform and zero-translate are painter-set constants, not
        // computed floats — the bit-pattern checks below are intentional exact
        // equality, not a floating-point near-equal question.
        for inst in &mut remapped_segment.rect_batch.instances {
            let is_identity_transform =
                inst.transform.map(f32::to_bits) == [1.0_f32, 0.0, 0.0, 1.0].map(f32::to_bits);
            let is_zero_translate =
                inst.transform_translate.map(f32::to_bits) == [0.0_f32; 4].map(f32::to_bits);
            let is_baked_aabb = is_identity_transform && is_zero_translate;

            if is_baked_aabb {
                // Baked-AABB: device origin is in bounds.xy; w/h scale proportionally.
                inst.bounds[0] = (inst.bounds[0] - origin_x) * scale_x;
                inst.bounds[1] = (inst.bounds[1] - origin_y) * scale_y;
                inst.bounds[2] *= scale_x;
                inst.bounds[3] *= scale_y;
            } else {
                // Affine path: constant translation is in transform_translate;
                // scale the linear 2×2 columns by the corresponding axis scale.
                inst.transform_translate[0] = (inst.transform_translate[0] - origin_x) * scale_x;
                inst.transform_translate[1] = (inst.transform_translate[1] - origin_y) * scale_y;
                // Scale x-column and y-column of the 2×2 linear part.
                inst.transform[0] *= scale_x; // a: x-col.x
                inst.transform[1] *= scale_y; // b: x-col.y  (cross-axis — scale_y)
                inst.transform[2] *= scale_x; // c: y-col.x  (cross-axis — scale_x)
                inst.transform[3] *= scale_y; // d: y-col.y
            }

            // Clip rrect (if active): [x, y, w, h, radii…].
            // Radii are dimension-like; scale them proportionally. For non-square
            // fb crops (scale_x ≠ scale_y) this slightly warps circle-corner radii —
            // acceptable for a filter intermediate (the composite restores shape).
            if inst.clip_kind[0] != 0 {
                inst.clip_rrect[0] = (inst.clip_rrect[0] - origin_x) * scale_x;
                inst.clip_rrect[1] = (inst.clip_rrect[1] - origin_y) * scale_y;
                inst.clip_rrect[2] *= scale_x;
                inst.clip_rrect[3] *= scale_y;
                // Radii [4..8]: scale by average of scale_x and scale_y.
                let avg_scale = (scale_x + scale_y) * 0.5;
                for r in &mut inst.clip_rrect[4..8] {
                    *r *= avg_scale;
                }
            }
        }

        // Circle and arc instances: center is in transform_translate; the linear
        // 2×2 matrix encodes the radius (baked fast path: [r,0,0,r]; affine: radius
        // varies).  Apply the same scale/translate remap as for affine rects.
        for inst in &mut remapped_segment.circle_batch.instances {
            inst.transform_translate[0] = (inst.transform_translate[0] - origin_x) * scale_x;
            inst.transform_translate[1] = (inst.transform_translate[1] - origin_y) * scale_y;
            inst.transform[0] *= scale_x;
            inst.transform[1] *= scale_y;
            inst.transform[2] *= scale_x;
            inst.transform[3] *= scale_y;
        }
        for inst in &mut remapped_segment.arc_batch.instances {
            inst.transform_translate[0] = (inst.transform_translate[0] - origin_x) * scale_x;
            inst.transform_translate[1] = (inst.transform_translate[1] - origin_y) * scale_y;
            inst.transform[0] *= scale_x;
            inst.transform[1] *= scale_y;
            inst.transform[2] *= scale_x;
            inst.transform[3] *= scale_y;
        }

        // ── Scissor remap: full-frame → fb-local (non-negotiable #6) ─────────
        //
        // Mirror ssaa.rs:483-512 without the ×2 supersampling factor:
        //   1. Intersect full-frame scissor with [fb_origin, fb_origin+fb_dim].
        //   2. Translate to fb-local coords (subtract fb_origin).
        //   3. On empty intersection: emit sentinel (fb_w, fb_h, 1, 1).
        //
        // Applied to every scissor type in the segment: tess_batches,
        // rect_scissors, circle_scissors, arc_scissors.
        //
        // Shadow / gradient / image scissors are NOT remapped here because those
        // kinds are excluded from the sub-viewport path by the `content_aabb` gate
        // (which returns `None` when any of those kinds is non-empty, forcing
        // `fb_dim == viewport` → identity remap → those fields render correctly
        // without any repositioning).
        //
        // Factored as a closure to eliminate the 4× identical copy of the
        // intersect+remap+sentinel logic.  The closure borrows `fb_x`, `fb_y`,
        // `fb_w`, `fb_h` from the enclosing scope (all non-`Copy` references are
        // immutable; `u32` copies are fine).
        let remap_scissor = |scissor: &mut Option<(u32, u32, u32, u32)>| {
            if let Some((sx, sy, sw, sh)) = *scissor {
                let fb_right = fb_x + fb_w;
                let fb_bottom = fb_y + fb_h;

                let scis_right = sx + sw;
                let scis_bottom = sy + sh;

                // Intersect full-frame scissor with fb region.
                let inter_x = sx.max(fb_x);
                let inter_y = sy.max(fb_y);
                let inter_right = scis_right.min(fb_right);
                let inter_bottom = scis_bottom.min(fb_bottom);

                if inter_right <= inter_x || inter_bottom <= inter_y {
                    // Empty intersection — nothing should draw here.
                    // Sentinel: off-target 1×1 rect (wgpu requires non-zero extent).
                    *scissor = Some((fb_w, fb_h, 1, 1));
                } else {
                    // Translate to fb-local (no scale factor — 1:1 not 2×).
                    *scissor = Some((
                        inter_x - fb_x,
                        inter_y - fb_y,
                        inter_right - inter_x,
                        inter_bottom - inter_y,
                    ));
                }
            }
        };

        // Tess-batch scissors (one per TessellatedBatch, directly on `batch.scissor`).
        for batch in &mut remapped_segment.tess_batches {
            remap_scissor(&mut batch.scissor);
        }

        // Instanced-kind scissors: rect / circle / arc.
        // Each ScissorRegion covers a contiguous run of instances that share a
        // scissor; remap each region's scissor from full-frame to fb-local.
        for region in &mut remapped_segment.rect_scissors {
            remap_scissor(&mut region.scissor);
        }
        for region in &mut remapped_segment.circle_scissors {
            remap_scissor(&mut region.scissor);
        }
        for region in &mut remapped_segment.arc_scissors {
            remap_scissor(&mut region.scissor);
        }

        // Flush the remapped segment into the fb-sized texture.
        // viewport_size = fb_dim so flush_segment sets its scissor against the
        // fb attachment; the static viewport uniform (vp_w, vp_h) combined with
        // the pre-scaled positions yields correct NDC.
        self.flush_segment(
            &mut remapped_segment,
            fb_dim,
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
                // ── Image-filter path nested inside a layer ───────────────────
                //
                // Z-order within the layer follows each item's `draw_order` position
                // and the replay loop — NOT match-arm textual position. The filter's
                // input segment is rendered to an isolated grown-bounds offscreen
                // (Task 6: fb_dim sized, not full-viewport), the pass chain is folded,
                // and the result is composited onto the layer's offscreen_view at the
                // integer-grid dst_rect with src_uv=[0,1] (non-negotiable #1).
                //
                // G2: no `_` arm — future Slice variants force a compile error here.
                DrawItem::Filter(mut op) => {
                    // 1. Render content to grown-bounds intermediate (Task 6).
                    let content_tex = self.render_segment_to_grown_offscreen(
                        &mut op.input,
                        op.fb_origin,
                        op.fb_dim,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                    );
                    // 2. Fold the pass chain over the grown-bounds intermediate.
                    let filtered_tex = apply_image_filter_passes(
                        &op.passes,
                        content_tex,
                        op.content_bounds,
                        op.fb_origin,
                        op.fb_dim,
                        surface_format,
                        pipelines,
                        resources,
                        device,
                        encoder,
                    );
                    // 3. Integer-grid composite (non-negotiable #1):
                    //    dst_rect = Rect(fb_origin, fb_far); src_uv = [0, 0, 1, 1].
                    //
                    //    The intermediate is `fb_dim`-sized with the content starting
                    //    at pixel (0,0) of the texture.  src_uv=[0,1] maps the whole
                    //    fb texture onto dst_rect — a pixel-aligned 1:1 blit.
                    //    Using the fractional `grown_bounds` as dst_rect over an
                    //    integer-origin texture would shift every pixel by
                    //    frac(grown_left) (the composite-grid shift, risk #1).
                    let (fb_origin_x, fb_origin_y) = op.fb_origin;
                    let (fb_w, fb_h) = op.fb_dim;
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "fb coords are u32 pixel dims ≤ viewport; f32 precision is sufficient"
                    )]
                    let dst_rect = flui_types::Rect::from_xywh(
                        flui_types::geometry::px(fb_origin_x as f32),
                        flui_types::geometry::px(fb_origin_y as f32),
                        flui_types::geometry::px(fb_w as f32),
                        flui_types::geometry::px(fb_h as f32),
                    );
                    let instance = super::instancing::TextureInstance::with_uv(
                        dst_rect,
                        [0.0, 0.0, 1.0, 1.0],
                        flui_types::styling::Color::WHITE,
                    );
                    let _ = self.texture_batch.add(instance);
                    self.flush_texture_batch_premultiplied(
                        device,
                        queue,
                        pipelines,
                        resources,
                        viewport_size,
                        encoder,
                        offscreen_view,
                        filtered_tex.view(),
                        None,
                    );
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
                    // COPY_SRC | TEXTURE_BINDING — DECISION 2) so that advanced
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
                        offscreen_view,
                        offscreen_target.texture, // sampleable pool texture for advanced dst-read
                    );
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

        let layer_tex = self.render_layer_to_offscreen(
            &mut layer,
            viewport_size,
            surface_format,
            device,
            queue,
            pipelines,
            resources,
            encoder,
        );

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
#[allow(clippy::too_many_arguments)]
fn fold_layer_filter_chain(
    filters: &LayerFilterChain,
    input_tex: PooledTexture,
    viewport_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipelines: &mut super::pipelines::PipelineSet,
    resources: &mut super::resources::GpuResources,
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
/// the ≤2-live-textures ping-pong discipline, identical to `fold_layer_filter_chain`.
///
/// The match has **no `_ =>` catch-all**: Slice 4 (`Blur`) is compiler-forced to
/// add an arm when its variant is introduced.
///
/// ## Parameters shared across all arms
///
/// - `content_bounds` — AABB of the content in physical pixels; used by the
///   morphology/blur passes to compute the decal UV guard (samples outside
///   `content_bounds` return the neutral element rather than the clamped edge texel).
/// - `fb_origin` — integer-aligned top-left of the offscreen frame in device pixels.
///   Used by blur/morph to rebase `content_bounds` into `fb`-local UV coordinates
///   (non-negotiable #3: `content_rect_uv = (content_bounds - fb_origin) / fb_dim`).
/// - `fb_dim` — integer dimensions of the intermediate textures; all pool acquires
///   use this size, and the `texture_size` uniform in blur/morph shaders is set to
///   `fb_dim` (non-negotiable #2: denominator must be integer fb_dim, not float
///   `grown.width()`).
/// - `viewport_size`, `surface_format`, `pipelines`, `resources`, `device`,
///   `encoder` — GPU context forwarded unchanged to every GPU pass arm.
#[allow(
    clippy::too_many_arguments,
    reason = "GPU pass fold threads device/encoder/pipeline/resources to every arm; \
              a context struct would add indirection without a semantic boundary"
)]
pub(in crate::wgpu) fn apply_image_filter_passes(
    passes: &[ImageFilterPass],
    input_tex: PooledTexture,
    content_bounds: flui_types::Rect<flui_types::geometry::Pixels>,
    fb_origin: (u32, u32),
    fb_dim: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipelines: &mut PipelineSet,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
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
                    content_bounds,
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
                // The H pass decals at `content_bounds` rebased to fb-local UV
                // (non-negotiable #3): samples outside contribute transparent black.
                // The V pass decals at the texture edge [0,1] to read the full H halo.
                apply_blur(
                    *sigma_x,
                    *sigma_y,
                    &acc,
                    content_bounds,
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
                // prior intermediate to the pool (≤2-live ping-pong preserved).
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
    }
    acc
}

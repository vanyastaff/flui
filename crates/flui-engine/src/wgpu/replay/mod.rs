//! Segment-flush, dispatch-loop, and GPU-plumbing component for `WgpuPainter`.
//!
//! `GpuReplay` owns the five static GPU plumbing fields shared by every flush
//! method, the per-frame texture-instance scratch batch, all six segment
//! flushers that submit recorded `DrawSegment` IR to the GPU, the top-level
//! `submit` dispatch loop, and opacity-layer recursion.  This completes
//! the T10d step of the record/replay split.
//!
//! | T10 step    | What moved                                                     |
//! |-------------|----------------------------------------------------------------|
//! | T10b        | `texture_batch` field + `flush_texture_batch*` family          |
//! | T10c        | `viewport_buffer`, `viewport_bind_group`, `unit_quad_buffer`,  |
//! |             | `unit_quad_index_buffer`, `default_sampler` + the six segment  |
//! |             | flushers (`flush_segment`, `flush_all_instanced_batches`,       |
//! |             | `flush_gradient_batches`, `flush_tessellated_geometry`,         |
//! |             | `flush_segment_cached_images`, `flush_segment_external_images`) |
//! | T10d (this) | `submit` dispatch loop, `flush_opacity_layer`,                 |
//! |             | `reintegrate_offscreen_content`                                |
//!
//! ## Flush order — R1 invariant
//!
//! `GpuReplay::flush_segment` orchestrates a fixed five-phase submit order:
//!
//! ```text
//! 1. flush_all_instanced_batches   (rect / circle / arc / shadow)
//! 2. flush_gradient_batches        (linear / radial / sweep)
//! 3. flush_tessellated_geometry    (lyon tessellated paths / vertices)
//! 4. flush_segment_cached_images   (texture-cache images, grouped by TextureId)
//! 5. flush_segment_external_images (external textures, resolved at replay time)
//! ```
//!
//! This order is **load-bearing** for z-ordering correctness — a reorder
//! silently corrupts draw results with no compile error.  Do not change it
//! without an explicit architecture review.
//!
//! ## Viewport bind-group layout identity
//!
//! `viewport_bind_group` is created in `GpuReplay::new` against
//! `pipelines.viewport_bind_group_layout()`, the same layout object every
//! pipeline in `PipelineSet` was built against.  wgpu
//! requires bind group and pipeline to share the **exact same** layout object —
//! substituting any structurally-equal-but-distinct layout causes a validation
//! error.  See the `pipelines.rs` module doc for the full hazard description.
//!
//! ## C4 rule — no `Matrix4` in this module
//!
//! This module is `Matrix4`-free by port-check Trigger 19 (same rule as
//! `batches/`).  Transforms live in `GpuStateStack` (glam internally) and cross
//! the record/replay boundary as baked float arrays in the `DrawSegment` IR.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::error::EngineResult;

use super::{
    advanced_blend::{AdvancedBlendOp, flush_advanced_layer},
    command_ir::{DrawItem, DrawSegment},
    instancing::{InstanceBatch, TextureInstance},
    opacity_layer::apply_image_filter_passes,
    pipelines::PipelineSet,
    render_target::RenderTarget,
    resources::GpuResources,
    text::TextRenderer,
};

/// Owns the five GPU plumbing fields, the per-frame texture-instance scratch
/// batch, all segment-flush methods, the top-level `submit` dispatch loop,
/// and opacity-layer recursion.
///
/// Created once per `WgpuPainter` via `GpuReplay::new` and stored as the
/// `replay` field.  The painter retains `device`, `queue`, `pipelines`,
/// `resources`, `surface_format`, and `size`; those are passed as borrowed
/// parameters so that `&mut GpuReplay` and the painter's own fields can
/// coexist in the same call without an `&mut self` on the painter.
// `wgpu::Device` / `wgpu::Queue` / `wgpu::Buffer` / `wgpu::BindGroup` /
// `wgpu::Sampler` are opaque GPU handles with no useful `Debug` impl.
#[allow(missing_debug_implementations)]
pub(super) struct GpuReplay {
    // ── Static GPU plumbing (moved from WgpuPainter in T10c) ─────────────────
    /// Viewport uniform buffer (updated on resize, read by all instanced
    /// and gradient pipelines as group 0 binding 0).
    viewport_buffer: wgpu::Buffer,

    /// Viewport bind group (group 0 for all instanced / gradient / shadow
    /// pipelines).
    ///
    /// Created against `PipelineSet::viewport_bind_group_layout` to satisfy
    /// the wgpu identity requirement: bind group and pipeline must share the
    /// exact same layout object.
    viewport_bind_group: wgpu::BindGroup,

    /// Shared unit-quad vertex buffer (0,0 to 1,1) reused by all instanced
    /// pipelines.
    unit_quad_buffer: wgpu::Buffer,

    /// Shared unit-quad index buffer (two triangles: 0,1,2 and 0,2,3).
    unit_quad_index_buffer: wgpu::Buffer,

    /// Default texture sampler (linear filtering, clamp-to-edge).
    ///
    /// `pub(super)` so the sibling `ssaa` module's `impl GpuReplay` block can
    /// reuse this sampler for the box-downsample bind group without adding a
    /// second sampler field.  The `wgpu` module boundary is `super` here.
    pub(super) default_sampler: wgpu::Sampler,

    // ── Per-frame texture-instance scratch batch (from T10b) ─────────────────
    /// Per-frame scratch batch for texture instances.
    ///
    /// Allocated once at construction time (1 024-instance capacity) and
    /// cleared after each flush.  Accumulate with `.texture_batch.add(instance)`;
    /// submit with one of the `flush_texture_batch*` methods.
    pub(super) texture_batch: InstanceBatch<TextureInstance>,
}

// `submit` and all flush methods accept `device`, `queue`, `pipelines`,
// `resources`, and `viewport_size` as borrowed parameters because those live
// on `WgpuPainter`.  The painter retains `size` for record-side methods that
// also need it; passing it explicitly keeps the borrow-seam clean.
//
// `cast_possible_truncation`, `cast_sign_loss`, and `cast_possible_wrap` are
// suppressed for the same reason as in `painter.rs`: GPU rendering converts
// between numeric types (pixel coords, buffer indices, instance counts)
// intentionally.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl GpuReplay {
    /// Construct a new [`GpuReplay`] with all five GPU plumbing fields initialised.
    ///
    /// `pipelines` must already be constructed (it owns the
    /// `viewport_bind_group_layout`); the bind group created here is built
    /// against that exact layout object, satisfying the wgpu identity
    /// requirement.
    ///
    /// Mirrors the viewport-buffer / bind-group / unit-quad / sampler
    /// construction that previously lived inside `WgpuPainter::with_shared_device`.
    pub(super) fn new(
        device: &wgpu::Device,
        pipelines: &PipelineSet,
        initial_width: u32,
        initial_height: u32,
    ) -> Self {
        // ── Viewport uniform buffer ───────────────────────────────────────────
        // [width, height, padding, padding] — matches the shader uniform layout.
        let viewport_data = [
            initial_width as f32,
            initial_height as f32,
            0.0_f32,
            0.0_f32,
        ];
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Viewport Uniform Buffer"),
            contents: bytemuck::cast_slice(&viewport_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // ── Viewport bind group ───────────────────────────────────────────────
        // Must be built against the layout from `PipelineSet` — see module doc.
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Viewport Bind Group"),
            layout: pipelines.viewport_bind_group_layout(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        // ── Shared unit quad geometry ─────────────────────────────────────────
        #[rustfmt::skip]
        let unit_quad_vertices: &[f32] = &[
            0.0, 0.0,  // Top-left
            1.0, 0.0,  // Top-right
            1.0, 1.0,  // Bottom-right
            0.0, 1.0,  // Bottom-left
        ];
        let unit_quad_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Unit Quad Vertex Buffer"),
            contents: bytemuck::cast_slice(unit_quad_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let unit_quad_indices: &[u16] = &[
            0, 1, 2, // Triangle 1
            0, 2, 3, // Triangle 2
        ];
        let unit_quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Unit Quad Index Buffer"),
            contents: bytemuck::cast_slice(unit_quad_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // ── Default texture sampler ───────────────────────────────────────────
        let default_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Default Texture Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 100.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });

        Self {
            viewport_buffer,
            viewport_bind_group,
            unit_quad_buffer,
            unit_quad_index_buffer,
            default_sampler,
            texture_batch: InstanceBatch::new(1024),
        }
    }

    /// Write new viewport dimensions into the GPU uniform buffer.
    ///
    /// Must be called from `WgpuPainter::resize` whenever the window size
    /// changes.  The write is byte-identical to what the painter previously
    /// did directly: `[width, height, 0.0, 0.0]` as four `f32` values.
    pub(super) fn update_viewport(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        let viewport_data = [width as f32, height as f32, 0.0_f32, 0.0_f32];
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::cast_slice(&viewport_data),
        );
    }

    // =========================================================================
    // Top-level dispatch loop (T10d)
    // =========================================================================

    /// Consume the drained draw-item list and submit all recorded GPU work to
    /// the encoder.
    ///
    /// ## Dispatch order — load-bearing
    ///
    /// Items are processed in the order they were drained from `draw_order`:
    ///
    /// - `DrawItem::Segment`          → `flush_segment` (R1 five-phase order)
    /// - `DrawItem::OffscreenTexture` → premultiplied texture composite
    /// - `DrawItem::OpacityLayer`     → `flush_opacity_layer` (recursive)
    ///
    /// After all geometry, `text_renderer.render` is called **last** — text is
    /// always on top (global final phase).
    ///
    /// ## R2 — `texture_batch` drain invariant
    ///
    /// `texture_batch` is a single scratch buffer shared across the dispatch
    /// loop and all `flush_opacity_layer` recursion.  Every `flush_texture_batch*`
    /// call drains and clears it before returning, so depth-N+1 content cannot
    /// leak into depth-N.  `&mut self` serializes the recursion.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn submit(
        &mut self,
        items: Vec<DrawItem>,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        text_renderer: &mut TextRenderer,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
    ) -> EngineResult<()> {
        // R1: arm order (Segment / OffscreenTexture / OpacityLayer /
        // AdvancedShape) is load-bearing for z-ordering — do not reorder.
        for item in items {
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
                        target.view,
                    );
                }
                DrawItem::OffscreenTexture(p) => {
                    let instance = super::instancing::TextureInstance::new(
                        p.bounds,
                        flui_types::styling::Color::WHITE,
                    );
                    let _ = self.texture_batch.add(instance);
                    // Offscreen compositing is always full-viewport — no scissor.
                    //
                    // These are shader-mask / backdrop-blur results from
                    // `OffscreenRenderer`, which clears its target transparent
                    // and draws with straight `ALPHA_BLENDING` — leaving the
                    // result *premultiplied*, exactly like an opacity-layer
                    // offscreen.  Composite with the premultiplied pipeline and
                    // an identity (white) tint so it is not re-multiplied by
                    // its own alpha (same defect class as BUG 2; fixed
                    // consistently here).
                    self.flush_texture_batch_premultiplied(
                        device,
                        queue,
                        pipelines,
                        resources,
                        viewport_size,
                        encoder,
                        target.view,
                        p.texture.view(),
                        None,
                    );
                    // p.texture dropped here, returns to pool
                }
                DrawItem::OpacityLayer(layer) => {
                    self.flush_opacity_layer(
                        layer,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        target,
                    );
                }
                // ── Advanced (dst-read) shape — DECISION 5 ─────────────────
                //
                // Z-correctness: all prior draw_order items have been flushed to
                // `target.view` earlier in this loop before this arm executes, so
                // the backdrop copy in `flush_advanced_layer` reads the correct
                // content-so-far from the surface.
                //
                // AA note: tessellated shapes run at sample_count=1 with no SDF
                // anti-aliasing — edges are aliased.  This is consistent with the
                // Phase-A quality note in `batches/shapes.rs`.
                //
                // Damage-straddle hazard: `flush_advanced_layer` issues its render
                // pass with `LoadOp::Load` and NO scissor, writing the blend result
                // to the full `op.device_bounds` on the surface.  If a partial
                // damage scissor was applied at record time, the foreground texture
                // is transparent OUTSIDE the scissor; the blend pass then computes
                // `blend(transparent_fg, stale_backdrop)` there, potentially
                // preserving prior-frame stale pixels in the out-of-damage slice.
                //
                // Self-healing: `renderer.rs` detects straddling advanced shapes
                // after `render_layer_recursive` and sets
                // `force_full_repaint_next_frame`, so the NEXT frame repaints the
                // full `device_bounds` without scissor restriction.  Acceptable
                // because partial damage is currently unused (callers use
                // `mark_full_repaint`); a this-frame re-record or a precomputed
                // Scene bit is the future upgrade when partial damage becomes hot.
                DrawItem::AdvancedShape(mut op) => {
                    if let Some(surface_texture) = target.texture {
                        // Render the shape into a full-viewport offscreen foreground.
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
                        let (vp_w, vp_h) = viewport_size;
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "vp_w/vp_h are u32 viewport dims; \
                                      precision loss at >16 M pixels is acceptable"
                        )]
                        let (viewport_width_f32, viewport_height_f32) = (vp_w as f32, vp_h as f32);

                        let blend_op = AdvancedBlendOp {
                            foreground,
                            mode: op.mode,
                            device_bounds: op.device_bounds,
                            // Identity tint + full opacity: shape color/alpha is
                            // already baked into premul vertex colors at record time.
                            // Applying opacity or tint here would double-apply it.
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
                            surface_texture,
                            target.view,
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
                            "GpuReplay: advanced shape blended onto surface"
                        );
                    } else {
                        // A `view_only` target has no sampleable backdrop; advanced
                        // modes degrade to SrcOver here (warn once).  Production
                        // producers pass a sampleable target — surface-with-COPY_SRC,
                        // the COPY_SRC-less intermediate, or a pooled offscreen — so
                        // this is only reached by genuinely view-only callers
                        // (benches/headless/ShaderMask-style child rendering).
                        tracing::warn!(
                            mode = ?op.mode,
                            "Advanced shape blend reached a view_only target; \
                             falling back to SrcOver (caller must pass sampleable target)"
                        );
                        self.flush_segment(
                            &mut op.segment,
                            viewport_size,
                            device,
                            queue,
                            pipelines,
                            resources,
                            encoder,
                            target.view,
                        );
                    }
                }
                // ── SSAA-supersampled path — PR-3 (SrcOver) / PR-4 (all modes) ──
                //
                // Z-correctness: all prior draw_order items have been flushed
                // before this arm executes (loop order), so the SSAA tile
                // composites on top of prior content — correct stacking for all
                // blend modes.
                //
                // Surface stays sample_count=1; the 2× texture is a normal texture.
                //
                // Advanced-blend composite (PR-4): `target.texture` is the sampleable
                // surface required by flush_advanced_layer for the backdrop copy.
                // View-only targets pass `None` → advanced falls back to SrcOver
                // (same fallback as AdvancedShape; warns once in that case).
                DrawItem::SsaaPath(mut op) => {
                    self.render_ssaa_path(
                        &mut op,
                        viewport_size,
                        surface_format,
                        device,
                        queue,
                        pipelines,
                        resources,
                        encoder,
                        target.view,
                        target.texture,
                    );
                    tracing::trace!(
                        mode = ?op.blend,
                        bounds = ?op.device_bounds,
                        "GpuReplay: SSAA path tile composited"
                    );
                }
                // ── Image-filter (bounds-growing) — Task 0 / Slice 0 ────────
                //
                // Z-correctness: z-order is set by each item's position in
                // `draw_order` and the `for item in items` replay loop — NOT by
                // match-arm textual position (the match is pure dispatch). When this
                // arm runs, every earlier draw-order item is already flushed to
                // `target.view`, so the filter result composites on top
                // (R1 z-order invariant; see `flush_segment`).
                //
                // Pool discipline: content_tex and filtered_tex are acquired at
                // replay time, never held in the IR. Both drop at arm end, returning
                // to the pool. The `apply_image_filter_passes` fold maintains ≤2
                // live textures regardless of chain length.
                DrawItem::Filter(mut op) => {
                    // 1. Render the isolated input segment to a GROWN-BOUNDS offscreen
                    //    (Task 6): sized to fb_dim instead of the full viewport.
                    //    Vertex positions are pre-transformed to fb-local NDC so that
                    //    dividing by the unchanged viewport uniform yields correct NDC
                    //    inside the smaller render target (non-negotiable #2).
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
                    //    Task 0: Identity → returns content_tex unchanged.
                    //    Blur/Morph: each sub-pass acquires a fb_dim texture and uses
                    //    fb-local UV for the content_rect decal (non-negotiable #3).
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
                    //    `filtered_tex` is fb_dim-sized with content at pixel (0,0).
                    //    src_uv=[0,1] maps the full fb texture onto dst_rect — a
                    //    pixel-aligned 1:1 blit via the bilinear composite sampler.
                    //
                    //    Using fractional grown_bounds as dst_rect over an integer-
                    //    origin texture would shift every pixel by frac(grown_left)
                    //    (the composite-grid shift, risk #1 in the Task 6 spec).
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
                        target.view,
                        filtered_tex.view(),
                        None,
                    );
                    // filtered_tex (and content_tex if distinct) dropped here → pool.
                    tracing::trace!(
                        content_bounds = ?op.content_bounds,
                        fb_origin = ?op.fb_origin,
                        fb_dim = ?op.fb_dim,
                        pass_count = op.passes.len(),
                        "GpuReplay: image filter composited"
                    );
                }
            }
        }

        // Text is always the global final phase — rendered on top of all geometry.
        text_renderer.render(device, queue, target.view, encoder, viewport_size)?;

        Ok(())
    }

    /// Re-integrate offscreen draw content back into the parent draw order.
    ///
    /// Fallback path used when full offscreen render-to-texture compositing is
    /// not needed (opacity ≈ 1.0, white tint).  Appends the offscreen segments
    /// and draw items into the provided target collections.
    ///
    /// When `_opacity` < 1.0 this produces incorrect results for overlapping
    /// children (each child gets independent alpha instead of the group being
    /// composited as a unit), but it preserves existing behavior until the full
    /// offscreen path is wired for all cases.
    pub(super) fn reintegrate_offscreen_content(
        offscreen_segment: DrawSegment,
        offscreen_order: Vec<DrawItem>,
        _opacity: f32,
        draw_order: &mut Vec<DrawItem>,
    ) {
        for item in offscreen_order {
            draw_order.push(item);
        }
        if !offscreen_segment.is_empty() {
            draw_order.push(DrawItem::Segment(offscreen_segment));
        }
    }
}

// The five-phase segment-flush machinery (flush_segment + every per-bucket
// flush helper it drives) is split out to restore the C1 <1500-LOC cap.
mod flush;

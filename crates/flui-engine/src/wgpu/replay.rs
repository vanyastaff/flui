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
    command_ir::{DrawItem, DrawSegment, PendingOpacityLayer, ScissorRect},
    instancing::{InstanceBatch, TextureInstance},
    pipeline::PipelineKey,
    pipelines::PipelineSet,
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
    default_sampler: wgpu::Sampler,

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
        view: &wgpu::TextureView,
    ) -> EngineResult<()> {
        // R1: arm order (Segment / OffscreenTexture / OpacityLayer) is
        // load-bearing for z-ordering — do not reorder.
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
                        view,
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
                        view,
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
                        view,
                    );
                }
            }
        }

        // Text is always the global final phase — rendered on top of all geometry.
        text_renderer.render(device, queue, view, encoder, viewport_size)?;

        Ok(())
    }

    // =========================================================================
    // Opacity-layer recursion (T10d)
    // =========================================================================

    /// Render an opacity layer's content to an offscreen texture and composite
    /// the result onto `main_view` with group opacity.
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
    fn flush_opacity_layer(
        &mut self,
        mut layer: PendingOpacityLayer,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        main_view: &wgpu::TextureView,
    ) {
        let (vp_w, vp_h) = viewport_size;
        if vp_w == 0 || vp_h == 0 {
            return;
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
        // R1: arm order is preserved (Segment / OffscreenTexture / OpacityLayer).
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
                        offscreen_view,
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
        let uv_left = layer.bounds.left().0 / vp_w as f32;
        let uv_top = layer.bounds.top().0 / vp_h as f32;
        let uv_right = layer.bounds.right().0 / vp_w as f32;
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
            main_view,
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

    // =========================================================================
    // Segment-flush entry point
    // =========================================================================

    /// Flush a single `DrawSegment` to the GPU in the canonical five-phase order.
    ///
    /// ## R1 — flush order invariant
    ///
    /// The five phases are executed in this exact sequence.  This order is
    /// **load-bearing** for z-ordering correctness — a reorder silently corrupts
    /// draw results with no compile error:
    ///
    /// 1. `flush_all_instanced_batches`   — rect / circle / arc / shadow
    /// 2. `flush_gradient_batches`        — linear / radial / sweep
    /// 3. `flush_tessellated_geometry`    — lyon tessellated paths / vertices
    /// 4. `flush_segment_cached_images`   — texture-cache images
    /// 5. `flush_segment_external_images` — external (registered) textures
    pub(super) fn flush_segment(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        // R1: five-phase order is load-bearing — do not reorder.
        self.flush_all_instanced_batches(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_gradient_batches(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_tessellated_geometry(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_segment_cached_images(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_segment_external_images(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
    }

    // =========================================================================
    // Phase 1: instanced batches (rect / circle / arc / shadow)
    // =========================================================================

    /// Flush all instanced batches in a single render pass (Phase 9 optimisation).
    ///
    /// Combines shadow → rect → circle → arc into one combined instance buffer
    /// and one render pass, switching pipelines dynamically.  The shadow
    /// instances are prepended first for correct z-ordering (background →
    /// foreground).
    ///
    /// Before (Phase 8): 1 buffer upload + 3 render passes + 3 draw calls.
    /// After  (Phase 9): 1 buffer upload + 1 render pass  + 3 draw calls.
    fn flush_all_instanced_batches(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        use super::multi_draw::{MultiDrawBatcher, PipelineId};

        let has_rects = !segment.rect_batch.is_empty();
        let has_circles = !segment.circle_batch.is_empty();
        let has_arcs = !segment.arc_batch.is_empty();
        let has_shadows = !segment.shadow_batch.is_empty();

        if !has_rects && !has_circles && !has_arcs && !has_shadows {
            return;
        }

        let rect_size =
            segment.rect_batch.len() * std::mem::size_of::<super::instancing::RectInstance>();
        let circle_size =
            segment.circle_batch.len() * std::mem::size_of::<super::instancing::CircleInstance>();
        let arc_size =
            segment.arc_batch.len() * std::mem::size_of::<super::instancing::ArcInstance>();
        let shadow_size =
            segment.shadow_batch.len() * std::mem::size_of::<super::instancing::ShadowInstance>();

        // IMPORTANT: Shadows FIRST for correct z-ordering (background → foreground).
        let mut combined_buffer =
            Vec::with_capacity(shadow_size + rect_size + circle_size + arc_size);
        let mut multi_batcher = MultiDrawBatcher::new();

        let shadow_offset = combined_buffer.len() as u64;
        if has_shadows {
            combined_buffer.extend_from_slice(segment.shadow_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Rectangle, // shadow pipeline rendered first for z-order
                segment.shadow_batch.len() as u32,
                shadow_offset,
                shadow_size as u64,
            );
        }

        let rect_offset = combined_buffer.len() as u64;
        if has_rects {
            combined_buffer.extend_from_slice(segment.rect_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Rectangle,
                segment.rect_batch.len() as u32,
                rect_offset,
                rect_size as u64,
            );
        }

        let circle_offset = combined_buffer.len() as u64;
        if has_circles {
            combined_buffer.extend_from_slice(segment.circle_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Circle,
                segment.circle_batch.len() as u32,
                circle_offset,
                circle_size as u64,
            );
        }

        let arc_offset = combined_buffer.len() as u64;
        if has_arcs {
            combined_buffer.extend_from_slice(segment.arc_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Arc,
                segment.arc_batch.len() as u32,
                arc_offset,
                arc_size as u64,
            );
        }

        #[cfg(debug_assertions)]
        {
            let stats = multi_batcher.stats();
            tracing::trace!(
                "GpuReplay::flush_all_instanced_batches: draws={}, instances={}, buffer={}B",
                stats.active_draws,
                stats.active_instances,
                combined_buffer.len()
            );
        }

        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "Combined Instance Buffer",
            &combined_buffer,
        );

        // ===== SINGLE RENDER PASS FOR ALL PRIMITIVES =====
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Combined Instanced Primitives Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let (full_w, full_h) = viewport_size;

        // --- Shadows (rendered first for correct z-ordering) ---
        if has_shadows {
            render_pass.set_pipeline(&pipelines.shadow);
            let buf_start = shadow_offset;
            let buf_end = buf_start + shadow_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));
            render_pass.set_scissor_rect(0, 0, full_w, full_h);
            render_pass.draw_indexed(0..6, 0, 0..segment.shadow_batch.len() as u32);
        }

        // --- Rectangles (per-scissor-region) ---
        if has_rects {
            render_pass.set_pipeline(&pipelines.instanced_rect);
            let buf_start = rect_offset;
            let buf_end = buf_start + rect_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &segment.rect_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // --- Circles (per-scissor-region) ---
        if has_circles {
            render_pass.set_pipeline(&pipelines.instanced_circle);
            let buf_start = circle_offset;
            let buf_end = buf_start + circle_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &segment.circle_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // --- Arcs (per-scissor-region) ---
        if has_arcs {
            render_pass.set_pipeline(&pipelines.instanced_arc);
            let buf_start = arc_offset;
            let buf_end = buf_start + arc_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &segment.arc_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        drop(render_pass);

        // Clear batches for next frame.
        segment.rect_batch.clear();
        segment.circle_batch.clear();
        segment.arc_batch.clear();
        segment.shadow_batch.clear();
        segment.rect_scissors.clear();
        segment.circle_scissors.clear();
        segment.arc_scissors.clear();
    }

    // =========================================================================
    // Phase 2: gradient batches (linear / radial / sweep)
    // =========================================================================

    /// Flush all gradient batches (linear, radial, sweep) in a single render pass.
    ///
    /// Uploads gradient stops and the combined instance buffer, then renders
    /// all three gradient types in one render pass with pipeline switches.
    fn flush_gradient_batches(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let has_linear = !segment.linear_gradient_batch.is_empty();
        let has_radial = !segment.radial_gradient_batch.is_empty();
        let has_sweep = !segment.sweep_gradient_batch.is_empty();

        if !has_linear && !has_radial && !has_sweep {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "GpuReplay::flush_gradient_batches: linear={}, radial={}, sweep={}, stops={}",
            segment.linear_gradient_batch.len(),
            segment.radial_gradient_batch.len(),
            segment.sweep_gradient_batch.len(),
            segment.current_gradient_stops.len()
        );

        // ===== Upload gradient stops to GPU =====
        if !segment.current_gradient_stops.is_empty() {
            pipelines.refresh_gradient_bind_group(
                device,
                queue,
                bytemuck::cast_slice(&segment.current_gradient_stops),
            );
        }

        let linear_size = segment.linear_gradient_batch.len()
            * std::mem::size_of::<super::instancing::LinearGradientInstance>();
        let radial_size = segment.radial_gradient_batch.len()
            * std::mem::size_of::<super::instancing::RadialGradientInstance>();
        let sweep_size = segment.sweep_gradient_batch.len()
            * std::mem::size_of::<super::instancing::SweepGradientInstance>();

        let mut combined_buffer = Vec::with_capacity(linear_size + radial_size + sweep_size);

        let linear_offset = 0;
        if has_linear {
            combined_buffer.extend_from_slice(segment.linear_gradient_batch.as_bytes());
        }

        let radial_offset = combined_buffer.len();
        if has_radial {
            combined_buffer.extend_from_slice(segment.radial_gradient_batch.as_bytes());
        }

        let sweep_offset = combined_buffer.len();
        if has_sweep {
            combined_buffer.extend_from_slice(segment.sweep_gradient_batch.as_bytes());
        }

        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "Gradient Instance Buffer",
            &combined_buffer,
        );

        // ===== SINGLE RENDER PASS FOR ALL GRADIENTS =====
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Gradient Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
            render_pass.set_bind_group(1, gradient_bind_group, &[]);
        }
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let (full_w, full_h) = viewport_size;

        // ===== Draw Linear Gradients (per-scissor-region) =====
        if has_linear {
            render_pass.set_pipeline(&pipelines.linear_gradient);
            // Re-set bind groups after pipeline switch (WebGPU invalidates bind
            // groups when the new pipeline's PipelineLayout is a different object).
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let linear_start = linear_offset as u64;
            let linear_end = linear_start + linear_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(linear_start..linear_end));

            for region in &segment.linear_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // ===== Draw Radial Gradients (per-scissor-region) =====
        if has_radial {
            render_pass.set_pipeline(&pipelines.radial_gradient);
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let radial_start = radial_offset as u64;
            let radial_end = radial_start + radial_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(radial_start..radial_end));

            for region in &segment.radial_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // ===== Draw Sweep Gradients (per-scissor-region) =====
        if has_sweep {
            render_pass.set_pipeline(&pipelines.sweep_gradient);
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let sweep_start = sweep_offset as u64;
            let sweep_end = sweep_start + sweep_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(sweep_start..sweep_end));

            for region in &segment.sweep_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        drop(render_pass);

        // Clear batches for next frame.
        segment.linear_gradient_batch.clear();
        segment.radial_gradient_batch.clear();
        segment.sweep_gradient_batch.clear();
        segment.current_gradient_stops.clear();
        segment.linear_grad_scissors.clear();
        segment.radial_grad_scissors.clear();
        segment.sweep_grad_scissors.clear();
    }

    // =========================================================================
    // Phase 3: tessellated geometry
    // =========================================================================

    /// Flush tessellated geometry from the segment.
    ///
    /// Uploads vertices/indices and renders all recorded tessellated batches in
    /// a single render pass, switching pipelines as needed.
    fn flush_tessellated_geometry(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        if segment.vertices.is_empty() || segment.tess_batches.is_empty() {
            return;
        }

        let (vertex_buffer, index_buffer) =
            resources.buffer_pool_mut().get_vertex_and_index_buffers(
                device,
                queue,
                "Shape Vertex Buffer",
                bytemuck::cast_slice(&segment.vertices),
                "Shape Index Buffer",
                bytemuck::cast_slice(&segment.indices),
            );

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shape Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        let (full_w, full_h) = viewport_size;
        let mut active_key: Option<PipelineKey> = None;
        for batch in &segment.tess_batches {
            if active_key != Some(batch.pipeline_key) {
                // `pipelines` and `device` are disjoint from the encoder/render_pass
                // borrows — no borrow conflict.
                let pipeline = pipelines
                    .shape_cache_mut()
                    .get_or_create(device, batch.pipeline_key);
                render_pass.set_pipeline(pipeline);
                active_key = Some(batch.pipeline_key);
            }

            if let Some((x, y, w, h)) = batch.scissor {
                render_pass.set_scissor_rect(x, y, w, h);
            } else {
                render_pass.set_scissor_rect(0, 0, full_w, full_h);
            }

            let start = batch.index_start;
            let end = start + batch.index_count;
            render_pass.draw_indexed(start..end, 0, 0..1);
        }

        drop(render_pass);

        segment.vertices.clear();
        segment.indices.clear();
        segment.tess_batches.clear();
        segment.current_pipeline_key = None;
    }

    // =========================================================================
    // Phase 4: segment-cached images
    // =========================================================================

    /// Flush all texture-cache image draws recorded in the segment.
    ///
    /// Groups consecutive draws by `TextureId` to minimise draw calls.  When
    /// a texture-ID change forces an early flush, the previous batch is
    /// submitted before the new `TextureId` takes over.
    fn flush_segment_cached_images(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let mut pending_images: Vec<(
            super::texture_cache::TextureId,
            super::instancing::TextureInstance,
            ScissorRect,
        )> = segment.cached_images.drain(..).collect();

        if pending_images.is_empty() {
            return;
        }

        let mut active_texture_id: Option<super::texture_cache::TextureId> = None;
        let mut active_texture_view: Option<wgpu::TextureView> = None;
        // Scissor of the most-recently buffered instance — forwarded when a
        // texture-change forces an early flush.
        let mut active_scissor: ScissorRect = None;

        for (texture_id, instance, scissor) in pending_images.drain(..) {
            if active_texture_id.as_ref() != Some(&texture_id) {
                if let Some(texture_view) = active_texture_view.as_ref() {
                    self.flush_texture_batch(
                        device,
                        queue,
                        pipelines,
                        resources,
                        viewport_size,
                        encoder,
                        view,
                        texture_view,
                        active_scissor,
                    );
                }
                active_texture_id = Some(texture_id.clone());
                active_texture_view = resources
                    .texture_cache_mut()
                    .get(&texture_id)
                    .map(|cached| cached.view.clone());
            }

            active_scissor = scissor;
            if let Some(texture_view) = active_texture_view.as_ref()
                && self.texture_batch.add(instance)
            {
                self.flush_texture_batch(
                    device,
                    queue,
                    pipelines,
                    resources,
                    viewport_size,
                    encoder,
                    view,
                    texture_view,
                    active_scissor,
                );
            }
        }

        if let Some(texture_view) = active_texture_view.as_ref() {
            self.flush_texture_batch(
                device,
                queue,
                pipelines,
                resources,
                viewport_size,
                encoder,
                view,
                texture_view,
                active_scissor,
            );
        }
    }

    // =========================================================================
    // Phase 5: external (registered) textures
    // =========================================================================

    /// Flush all external-texture draws recorded in the segment.
    ///
    /// Each entry carries a `flui_types::painting::TextureId` stored at
    /// record time.  Here, at replay time, each ID is resolved to a
    /// `wgpu::TextureView` via the external texture registry.  If an ID is not
    /// found (texture was unregistered between record and flush), a warning is
    /// emitted and the entry is skipped — identical behavior to before, now on
    /// the correct replay side of the record/replay seam.
    ///
    /// Because `wgpu::TextureView` is not `PartialEq`, instances are flushed
    /// individually (one draw call per instance) rather than grouped by view
    /// equality.  External textures are uncommon in typical UI; the extra draw
    /// calls are not a hot path.
    fn flush_segment_external_images(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        if segment.external_images.is_empty() {
            return;
        }

        // Drain into a local vec so we can call `&mut self` methods
        // (`flush_texture_batch`) while iterating without holding a borrow on
        // `segment.external_images`.
        let pending: Vec<(
            flui_types::painting::TextureId,
            super::instancing::TextureInstance,
            ScissorRect,
        )> = segment.external_images.drain(..).collect();

        for (texture_id, instance, scissor) in pending {
            // Resolve ID → view at replay time.  Clone the view to release the
            // borrow on `resources` before calling `flush_texture_batch` (which
            // takes `&mut resources`).  External textures are uncommon; the
            // clone is not a hot-path concern.
            let tex_view =
                if let Some(entry) = resources.external_texture_registry().get(texture_id) {
                    entry.view.clone()
                } else {
                    tracing::warn!(
                        "External texture {} not found at flush time — skipping draw",
                        texture_id.get()
                    );
                    continue;
                };

            let _ = self.texture_batch.add(instance);
            self.flush_texture_batch(
                device,
                queue,
                pipelines,
                resources,
                viewport_size,
                encoder,
                view,
                &tex_view,
                scissor,
            );
        }
    }

    // =========================================================================
    // Texture-batch flush methods (from T10b, updated: 5 params → self fields)
    // =========================================================================

    /// Flush the texture instance batch with straight-alpha blending.
    ///
    /// Used for normal decoded-image draws whose samples carry straight
    /// (non-premultiplied) alpha.  Offscreen layer composites must use
    /// `flush_texture_batch_premultiplied` instead.
    ///
    /// `scissor` is the clip rect to apply.  Pass `None` for full-viewport
    /// (unclipped), matching the rect/circle instanced batches.
    pub(super) fn flush_texture_batch(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
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
    /// Used to composite offscreen layer textures (opacity / ColorFilter /
    /// ShaderMask / backdrop results) whose texels are premultiplied
    /// (`rgb = straight_rgb * a`).  Compositing with the straight pipeline
    /// would re-multiply rgb by alpha, darkening translucent/AA content.
    /// Routes through `PipelineSet::instanced_texture_premul` (src factor
    /// `One`); the per-channel tint carries group opacity and any ColorFilter
    /// chroma as `(C.r*O, C.g*O, C.b*O, O)`.
    pub(super) fn flush_texture_batch_premultiplied(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
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
            resources,
            viewport_size,
            encoder,
            view,
            texture_view,
            scissor,
            true,
        );
    }

    /// Shared body for the two public texture-batch flush methods.
    ///
    /// `premultiplied` selects the blend pipeline: `false` = straight-alpha
    /// (decoded images), `true` = premultiplied source-over (offscreen-layer
    /// composites).
    fn flush_texture_batch_with_blend(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
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

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Texture Instance Bind Group"),
            layout: &pipelines.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.default_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
            ],
        });

        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "Texture Instance Buffer",
            self.texture_batch.as_bytes(),
        );

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Instanced Texture Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        // Offscreen-layer composites use the premultiplied pipeline; normal
        // decoded-image draws use straight alpha.  Selection logic is
        // behavior-preserving (round-5c color-correctness fix).
        let pipeline = if premultiplied {
            &pipelines.instanced_texture_premul
        } else {
            &pipelines.instanced_texture
        };
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_bind_group(1, &texture_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_vertex_buffer(1, instance_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let (full_w, full_h) = viewport_size;
        if let Some((x, y, w, h)) = scissor {
            render_pass.set_scissor_rect(x, y, w, h);
        } else {
            render_pass.set_scissor_rect(0, 0, full_w, full_h);
        }

        render_pass.draw_indexed(0..6, 0, 0..self.texture_batch.len() as u32);
        drop(render_pass);
        self.texture_batch.clear();
    }
}

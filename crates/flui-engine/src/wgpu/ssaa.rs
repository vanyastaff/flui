//! SSAA (2× supersampled) path anti-aliasing pipeline and replay helpers.
//!
//! This module provides:
//!
//! - `SsaaDownsamplePipeline` — the `box_downsample.wgsl` render pipeline +
//!   its bind-group layout.  Converts a 2× supersampled source tile into a
//!   premultiplied 1× tile via a 4-tap box filter.
//!
//! - `GpuReplay::render_ssaa_path` — the replay-time implementation for
//!   `DrawItem::SsaaPath` items.  Acquires a 2× pooled texture, renders the
//!   path segment into it (clearing to transparent first), box-downsamples to
//!   a 1× tile, and composites via the existing premultiplied texture batch path.
//!
//! ## Surface / sample-count invariant
//!
//! The SSAA tile is a plain normal (non-multisampled) texture, just twice the
//! logical resolution.  `sample_count` stays 1 everywhere.  No stencil, no
//! `resolve_target`.  This is safe for Phase B (advanced blend / opacity layers)
//! which also uses `sample_count: 1` pooled textures.
//!
//! ## Premultiplied correctness
//!
//! `shape.wgsl` emits PREMULTIPLIED colour (`vec4(rgb*a, a)`, shape.wgsl:51-53),
//! and the SrcOver tessellated pipeline uses `PREMULTIPLIED_ALPHA_BLENDING`
//! (src factor `One`, pipeline.rs:133). The 2× tile starts clear-transparent, so
//! the path accumulates premultiplied values over transparent. The box downsample
//! averages premultiplied values, which is linear-correct (premultiplied colour is
//! linear in coverage). The 1× tile is then composited via
//! `flush_texture_batch_premultiplied_with_mode` using the exact blend factors
//! for `op.blend` (src factor `One` for all tile-safe premultiplied modes).

use std::sync::Arc;

use flui_types::{Rect, geometry::Pixels};

use super::{
    advanced_blend::{AdvancedBlendOp, flush_advanced_layer},
    command_ir::SsaaPathOp,
    pipeline::is_tile_safe_for_ssaa,
    pipelines::PipelineSet,
    replay::GpuReplay,
    resources::GpuResources,
    texture_pool::PooledTexture,
};

// ─── Downsample pipeline ───────────────────────────────────────────────────────

/// Pipeline for the 2×→1× box-filter downsample pass used by SSAA path AA.
///
/// The pipeline samples a 2× supersampled source texture (premultiplied RGBA)
/// and averages the four sub-texels for each output pixel, producing a
/// premultiplied 1× tile ready for compositing.
///
/// One `SsaaDownsamplePipeline` is created per `PipelineSet` (keyed to the
/// surface format) and reused for every SSAA path in the frame.
// wgpu handle types do not implement Debug.
#[allow(missing_debug_implementations)]
pub(crate) struct SsaaDownsamplePipeline {
    /// Render pipeline for the 4-tap box downsample.
    pub(crate) pipeline: wgpu::RenderPipeline,
    /// Bind group layout: binding 0 = source texture, binding 1 = linear sampler.
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
}

impl SsaaDownsamplePipeline {
    /// Create the downsample pipeline for `output_format`.
    ///
    /// `output_format` is the target format of the 1× tile — always
    /// `surface_format` so the tile is compatible with the premultiplied
    /// texture compositor.
    pub(crate) fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SSAA Box Downsample Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("shaders/effects/box_downsample.wgsl").into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("SSAA Downsample Bind Group Layout"),
            entries: &[
                // binding 0: source 2× texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 1: linear sampler (for the 4-tap average)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SSAA Downsample Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("SSAA Box Downsample Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    // Two f32 position + two f32 UV per vertex.
                    array_stride: 4 * std::mem::size_of::<f32>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    // Premultiplied source-over: the downsampled tile is premultiplied;
                    // compositing onto a pre-cleared transparent output uses src-factor One.
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }
}

// ─── Replay helper ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
impl GpuReplay {
    /// Replay a `DrawItem::SsaaPath`: render the path into a 2× tile,
    /// box-downsample to a premultiplied 1× tile, then composite onto the target.
    ///
    /// ## Composite step (Step 5) — PR-4 blend routing
    ///
    /// After producing the AA'd 1× tile, the composite is selected by `op.blend`:
    ///
    /// - **tile-safe** (`is_tile_safe_for_ssaa(op.blend)` = true): composite via
    ///   `flush_texture_batch_premultiplied` with `blend_state_for(op.blend)`.
    ///   Transparent SSAA padding is a no-op for these modes (dst preserved).
    ///   This is an extension over PR-3 (SrcOver-only); now handles Dst, DstOver,
    ///   DstOut, SrcATop, Xor, Plus as well.
    ///
    /// - **advanced** (`op.blend.is_advanced()` = true): composite via
    ///   `flush_advanced_layer` with the 1× tile as foreground.  Requires a
    ///   sampleable `surface_texture`; falls back to tile-safe SrcOver if absent.
    ///
    /// Coverage-destructive modes (Clear, Src, SrcIn, DstIn, SrcOut, DstATop,
    /// Modulate) never reach `render_ssaa_path` — they are kept on the tessellated
    /// (aliased) path in the record-side batchers.
    ///
    /// ## Algorithm
    ///
    /// 1. Compute the integer device tile rect = `ceil(device_bounds)` clamped
    ///    to `[1, viewport]`.
    /// 2. Acquire a 2× pooled texture `(tile_w*2, tile_h*2)` from the layer pool.
    /// 3. Render the path segment into the 2× tile:
    ///    - Clear to transparent (`LoadOp::Clear`).
    ///    - Translate vertices by `-tile_origin` so the tile maps to `[0, tile_size]`.
    ///    - `flush_segment` with a `tile_size`-wide viewport (1× logical size) so
    ///      the 1× geometry fills the full 2× texture → 2× supersampled.
    /// 4. Acquire a 1× pooled texture `(tile_w, tile_h)`.  Clear to transparent.
    ///    Run `SsaaDownsamplePipeline` to box-average the 2× → 1×.
    /// 5. Composite the 1× tile using the routing above.
    ///
    /// Both pooled textures are RAII (return to pool on drop).  The 2× tile is
    /// dropped before the composite step; the 1× tile drops after the composite.
    pub(in crate::wgpu) fn render_ssaa_path(
        &mut self,
        op: &mut SsaaPathOp,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        // Surface texture for advanced (dst-read) blend composite.
        // Pass `None` for view-only targets (advanced falls back to SrcOver).
        surface_texture: Option<&wgpu::Texture>,
    ) {
        let (vp_w, vp_h) = viewport_size;

        // ── Step 1: integer tile rect, covering ceil(right)−floor(left) ──────
        //
        // Round each edge independently before deriving extent:
        //
        //   tile_x = floor(left)          (start of leftmost sub-pixel column)
        //   tile_y = floor(top)
        //   tile_w = ceil(right)−tile_x   (ensures tile covers [floor(l), ceil(r)])
        //   tile_h = ceil(bottom)−tile_y
        //
        // Adding 1px fringe on each dimension provides the AA fringe budget that
        // Skia/Impeller use, so the antialiasing gradient on the extreme edge is
        // not truncated.
        //
        // Previous scheme `tile_w = ceil(width)` was WRONG:
        //   left=5.6, right=25.4 → width=19.8 → tile_x=5, tile_w=ceil(19.8)=20
        //   → tile right = 25, which is < right(25.4) → the rightmost 0.4 px
        //   plus its AA fringe were hardware-clipped (HIGH finding).
        //
        // The max_tile_half cap ensures the 2× texture never exceeds the device
        // max_texture_dimension_2d.  Tile dimensions are clamped to the viewport
        // as before; the additional half-max cap handles the "vp≈max/2" crash
        // scenario (HIGH finding — no-op .min(vp_w*2) was the only previous bound).
        let max_tex_dim = device.limits().max_texture_dimension_2d;
        // Half of max dim: the 2× texture must be ≤ max_tex_dim, so each tile
        // side must be ≤ max_tex_dim/2.  Use saturating_div to avoid u32 overflow.
        let max_tile_half = max_tex_dim.saturating_div(2).max(1);

        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "device_bounds is non-negative device-pixel coordinates; \
                      floor/ceil→u32 is analytically safe and clamped to [1,vp]"
        )]
        let tile_x = op.device_bounds.left().0.floor().max(0.0) as u32;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "device_bounds is non-negative device-pixel coordinates"
        )]
        let tile_y = op.device_bounds.top().0.floor().max(0.0) as u32;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "device_bounds right/bottom edges are non-negative; ceil is safe"
        )]
        let tile_right_edge = (op.device_bounds.right().0.ceil() as u32 + 1).min(vp_w); // +1px AA fringe
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "device_bounds right/bottom edges are non-negative; ceil is safe"
        )]
        let tile_bottom_edge = (op.device_bounds.bottom().0.ceil() as u32 + 1).min(vp_h); // +1px AA fringe

        let tile_w = tile_right_edge.saturating_sub(tile_x).max(1);
        let tile_h = tile_bottom_edge.saturating_sub(tile_y).max(1);

        // No silent truncation: a path whose 2× tile would exceed the device's
        // max texture dimension is rendered DIRECTLY onto the target (aliased but
        // COMPLETE) rather than cropped into a clamped tile. Clamping the tile to
        // `max_tile_half` (the previous behavior) hardware-clipped every vertex
        // past the clamp, silently dropping the right/bottom of large path fills.
        if tile_w > max_tile_half || tile_h > max_tile_half {
            tracing::warn!(
                tile_w,
                tile_h,
                max_tile_half,
                "SSAA path exceeds max tile size; rendering aliased (complete, no AA) \
                 to avoid silent truncation"
            );
            self.flush_segment(
                &mut op.segment,
                viewport_size,
                device,
                queue,
                pipelines,
                resources,
                encoder,
                target_view,
            );
            return;
        }

        // ── Step 2: acquire a 2× pooled texture ───────────────────────────────
        //
        // tile_w ≤ max_tile_half = max_tex_dim/2 (guaranteed by the fallback
        // above), so tile_w*2 ≤ max_tex_dim. No wgpu create_texture error.
        let supersample_w = tile_w * 2;
        let supersample_h = tile_h * 2;

        let super_tex = resources.layer_texture_pool_mut().acquire(
            supersample_w,
            supersample_h,
            surface_format,
        );
        let super_view = super_tex.view();

        // ── Step 3: clear + render into the 2× tile ───────────────────────────

        // Clear to transparent (R3 invariant from opacity_layer.rs).
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAA Path 2x Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: super_view,
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
        }

        // Remap vertices from full-frame device-pixel space into the coordinate
        // system the shape shader expects for the 2× tile render.
        //
        // The shape shader always computes:
        //   clip_x = (pos.x / viewport.size.x) * 2.0 - 1.0
        // where `viewport.size` is the value in `GpuReplay::viewport_buffer` —
        // the full-frame size `(vp_w, vp_h)`.  We cannot change that uniform
        // mid-encoder (`queue.write_buffer` takes effect at the next submit, not
        // mid-encoder), so we must pre-transform the vertex positions so that
        // dividing by `vp_w/vp_h` yields the correct NDC for the tile.
        //
        // Goal: a vertex at full-frame device pixel `(px, py)` should fill the
        // 2× tile as if the tile were the whole viewport.  The tile-local
        // coordinate is `(px - tile_x, py - tile_y)`.  We want:
        //   (pos.x / vp_w) * 2 - 1  ==  ((px - tile_x) / tile_w) * 2 - 1
        // Therefore: pos.x = (px - tile_x) * (vp_w / tile_w).
        //
        // `flush_segment` receives `viewport_size = (supersample_w, supersample_h)`
        // so the scissor rect it sets covers the full 2× render target; wgpu
        // clamps the scissor to the attachment dimensions automatically.
        let tile_origin_x = tile_x as f32;
        let tile_origin_y = tile_y as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "tile_w/tile_h are small u32 tile dims; f32 precision is \
                      sufficient for device-pixel coordinate remapping"
        )]
        let scale_x = vp_w as f32 / tile_w as f32;
        #[allow(
            clippy::cast_precision_loss,
            reason = "tile_h is a small u32 tile dim; f32 precision is sufficient"
        )]
        let scale_y = vp_h as f32 / tile_h as f32;

        let mut remapped_segment = op.segment.clone();
        for v in &mut remapped_segment.vertices {
            v.position[0] = (v.position[0] - tile_origin_x) * scale_x;
            v.position[1] = (v.position[1] - tile_origin_y) * scale_y;
        }

        // ── Scissor remap: full-frame → tile-local 2× space ─────────────────
        //
        // The `tess_batches` scissor was captured at record time in full-frame
        // device-pixel coordinates (via `state.current_scissor()`).  We are
        // now rendering into a 2× tile attachment whose top-left corresponds to
        // `(tile_x, tile_y)` in full-frame space.  Applying the full-frame
        // scissor verbatim against the tile attachment would clip to entirely
        // the wrong region or fully clip the path (BLOCKER finding).
        //
        // Algorithm per batch:
        //   1. Intersect the full-frame scissor with the tile rect.
        //      If the intersection is empty → the entire tile is clipped →
        //      set the batch's scissor to a zero-area rect (nothing drawn).
        //   2. Translate the intersected rect to be tile-relative, then
        //      scale both origin and extent by 2 (1× → 2× supersampled space).
        //   3. Store the result back; `flush_tessellated_geometry` will apply it
        //      against the (supersample_w × supersample_h) attachment.
        //
        // When the batch scissor is `None` (no clip), the geometry fills the
        // entire tile — leave it as `None` so the full 2× attachment is covered.
        for batch in &mut remapped_segment.tess_batches {
            if let Some((sx, sy, sw, sh)) = batch.scissor {
                // Tile rect in full-frame device pixels.
                let tile_right = tile_x + tile_w;
                let tile_bottom = tile_y + tile_h;

                // Full-frame scissor right/bottom edges.
                let scis_right = sx + sw;
                let scis_bottom = sy + sh;

                // Intersect: [max(left), max(top), min(right), min(bottom)].
                let inter_x = sx.max(tile_x);
                let inter_y = sy.max(tile_y);
                let inter_right = scis_right.min(tile_right);
                let inter_bottom = scis_bottom.min(tile_bottom);

                if inter_right <= inter_x || inter_bottom <= inter_y {
                    // Intersection is empty → tile is fully clipped → nothing
                    // should be drawn.  Use a 1×1 off-target rect as a sentinel
                    // (wgpu requires non-zero extent; clamping to attachment dims
                    // means it will simply not intersect any drawn pixels).
                    batch.scissor = Some((supersample_w, supersample_h, 1, 1));
                } else {
                    // Translate to tile-local coordinates and scale to 2× space.
                    let local_x = (inter_x - tile_x) * 2;
                    let local_y = (inter_y - tile_y) * 2;
                    let local_w = (inter_right - inter_x) * 2;
                    let local_h = (inter_bottom - inter_y) * 2;
                    batch.scissor = Some((local_x, local_y, local_w, local_h));
                }
            }
        }

        // Flush the remapped segment into the 2× texture.
        // `viewport_size = (supersample_w, supersample_h)` so the scissor covers
        // the full 2× render target.  The shape shader's static viewport uniform
        // `(vp_w, vp_h)` combined with the pre-scaled positions produces NDC that
        // fills the tile, rendering it into the 2× texture → supersampled.
        self.flush_segment(
            &mut remapped_segment,
            (supersample_w, supersample_h),
            device,
            queue,
            pipelines,
            resources,
            encoder,
            super_view,
        );

        // ── Step 4: box-downsample 2× → 1× premultiplied tile ────────────────

        let one_x_tile = self.downsample_ssaa_tile(
            &super_tex,
            tile_w,
            tile_h,
            surface_format,
            device,
            pipelines,
            resources,
            encoder,
        );

        // 2× tile is no longer needed — drop it back to the pool now, before
        // the composite pass, to minimise peak texture memory.
        drop(super_tex);

        // ── Step 5: composite the 1× tile onto the target ────────────────────
        //
        // PR-4 blend routing (see function doc):
        //   - tile-safe → fixed-function premul blend (SrcOver pipeline for now)
        //   - advanced   → flush_advanced_layer (dst-read W3C composite)
        //   - SrcOver (PR-3 baseline) → tile-safe path

        let composite_bounds = Rect::from_xywh(
            Pixels(tile_x as f32),
            Pixels(tile_y as f32),
            Pixels(tile_w as f32),
            Pixels(tile_h as f32),
        );

        if op.blend.is_advanced() {
            // Advanced (dst-read) composite: route through flush_advanced_layer,
            // same as AdvancedShape. The 1× SSAA tile is the AA'd foreground.
            if let Some(surf_tex) = surface_texture {
                let blend_op = AdvancedBlendOp {
                    foreground: one_x_tile,
                    mode: op.blend,
                    device_bounds: composite_bounds,
                    opacity: 1.0,
                    tint: [1.0, 1.0, 1.0],
                    // 1× tile exactly covers composite_bounds; UV is identity.
                    src_uv_min: [0.0, 0.0],
                    src_uv_max: [1.0, 1.0],
                };
                flush_advanced_layer(
                    blend_op,
                    surf_tex,
                    target_view,
                    surface_format,
                    viewport_size,
                    &pipelines.advanced_blend,
                    resources,
                    device,
                    encoder,
                );
                tracing::trace!(
                    mode = ?op.blend,
                    bounds = ?composite_bounds,
                    "GpuReplay: SSAA path tile → advanced composite"
                );
                // one_x_tile was moved into AdvancedBlendOp.foreground;
                // it returns to pool when AdvancedBlendOp is dropped inside
                // flush_advanced_layer.
            } else {
                // View-only target has no sampleable backdrop; fall back to SrcOver.
                // Same fallback as AdvancedShape in replay.rs.
                tracing::warn!(
                    mode = ?op.blend,
                    "SSAA path advanced blend reached a view_only target; \
                     falling back to SrcOver (caller must pass sampleable target)"
                );
                let instance = super::instancing::TextureInstance::new(
                    composite_bounds,
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
                    target_view,
                    one_x_tile.view(),
                    None,
                );
                // one_x_tile drops here → returns to pool.
            }
        } else {
            // Tile-safe: fixed-function premul composite with the exact blend mode.
            //
            // All tile-safe modes satisfy: blend(transparent_src, dst) == dst,
            // so the SSAA tile's transparent border pixels do not corrupt dst.
            //
            // `flush_texture_batch_premultiplied_with_mode` selects (or lazily
            // creates) a pipeline whose `wgpu::BlendState` matches `op.blend`
            // exactly, so DstOut, Plus, DstOver, Xor, SrcATop, Dst, and SrcOver
            // all composite the 1× tile with their correct factors.
            debug_assert!(
                is_tile_safe_for_ssaa(op.blend),
                "non-advanced, non-tile-safe mode {:?} reached SSAA tile composite — \
                 coverage-destructive modes must stay on the tessellated path",
                op.blend
            );
            let instance = super::instancing::TextureInstance::new(
                composite_bounds,
                flui_types::styling::Color::WHITE,
            );
            let _ = self.texture_batch.add(instance);
            self.flush_texture_batch_premultiplied_with_mode(
                op.blend,
                device,
                queue,
                pipelines,
                resources,
                viewport_size,
                encoder,
                target_view,
                one_x_tile.view(),
                None, // no scissor — tile exactly covers composite_bounds
            );
            // one_x_tile drops here → returns to pool (RAII).
        }
    }

    /// Box-downsample a 2× pooled `source_texture` into a fresh 1× tile.
    ///
    /// Returns the 1× [`PooledTexture`]; the caller is responsible for
    /// compositing it before dropping.
    fn downsample_ssaa_tile(
        &mut self,
        source_tex: &PooledTexture,
        output_w: u32,
        output_h: u32,
        output_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> PooledTexture {
        // Acquire the 1× output tile and clear it to transparent.
        let one_x_tile =
            resources
                .layer_texture_pool_mut()
                .acquire(output_w, output_h, output_format);
        let one_x_view = one_x_tile.view();

        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAA Downsample 1x Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: one_x_view,
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
        }

        // Build the bind group: source 2× texture + linear sampler.
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SSAA Downsample Bind Group"),
            layout: &pipelines.ssaa_downsample.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source_tex.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    // `default_sampler` uses Linear filtering — correct for the 4-tap box average.
                    resource: wgpu::BindingResource::Sampler(&self.default_sampler),
                },
            ],
        });

        // Fullscreen quad in NDC: two triangles covering [-1,1] × [-1,1].
        // UV (0,0) = top-left, (1,1) = bottom-right to match wgpu's top-left origin.
        #[rustfmt::skip]
        let quad_vertices: &[f32] = &[
            // position (x,y)   UV (u,v)
            -1.0,  1.0,         0.0, 0.0, // top-left
            -1.0, -1.0,         0.0, 1.0, // bottom-left
             1.0, -1.0,         1.0, 1.0, // bottom-right
            -1.0,  1.0,         0.0, 0.0, // top-left
             1.0, -1.0,         1.0, 1.0, // bottom-right
             1.0,  1.0,         1.0, 0.0, // top-right
        ];

        let vertex_buffer = {
            use wgpu::util::DeviceExt as _;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SSAA Downsample Quad VB"),
                contents: bytemuck::cast_slice(quad_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            })
        };

        // Run the downsample pass.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAA Box Downsample Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: one_x_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // preserve the clear-transparent baseline
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&pipelines.ssaa_downsample.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..6, 0..1);
        }

        one_x_tile
    }
}

// ─── unit tests (CPU-only) ────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::super::batches::DrawBatcher;
    use super::super::command_ir::{DrawItem, DrawSegment};
    use super::super::state_stack::GpuStateStack;
    use super::super::{command_ir::SsaaPathOp, vertex::Vertex};
    use flui_types::{Rect, geometry::Pixels};

    fn make_vertex(x: f32, y: f32) -> Vertex {
        Vertex {
            position: [x, y],
            color: [1.0, 0.0, 0.0, 1.0],
            tex_coord: [0.0, 0.0],
        }
    }

    // ── U1: SsaaPathOp is Clone (T11 purity witness) ─────────────────────────

    /// U1: `SsaaPathOp` must implement `Clone` (T11 IR-purity contract).
    ///
    /// Derivability of `Clone` proves no GPU handle is embedded in the record IR.
    #[test]
    fn ssaa_path_op_is_clone() {
        let mut seg = DrawSegment::new();
        seg.vertices.push(make_vertex(0.0, 0.0));
        seg.vertices.push(make_vertex(10.0, 0.0));
        seg.vertices.push(make_vertex(5.0, 10.0));

        let op = SsaaPathOp {
            segment: seg,
            device_bounds: Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(10.0), Pixels(10.0)),
            blend: flui_types::painting::BlendMode::SrcOver,
        };

        let cloned = op.clone();
        assert_eq!(
            cloned.segment.vertices.len(),
            3,
            "cloned SsaaPathOp must have 3 vertices"
        );
        assert!(
            (cloned.device_bounds.width().0 - 10.0).abs() < f32::EPSILON,
            "cloned device_bounds width must be 10"
        );
    }

    // ── U2: divert_path_to_ssaa produces DrawItem::SsaaPath ─────────────────

    /// U2: `divert_path_to_ssaa` must push `DrawItem::SsaaPath` to draw_order.
    #[test]
    fn divert_path_to_ssaa_produces_ssaa_path_item() {
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        let vertices = vec![
            make_vertex(0.0, 0.0),
            make_vertex(10.0, 0.0),
            make_vertex(5.0, 10.0),
        ];
        let indices = [0u32, 1, 2];

        DrawBatcher::divert_path_to_ssaa(
            &mut segment,
            &mut draw_order,
            &state,
            &vertices,
            &indices,
            flui_types::painting::BlendMode::SrcOver,
        );

        assert_eq!(draw_order.len(), 1, "one SsaaPath item must be pushed");
        assert!(
            matches!(draw_order[0], DrawItem::SsaaPath(_)),
            "draw_order[0] must be SsaaPath"
        );
    }

    // ── U3: divert seals prior content before pushing SsaaPath ──────────────

    /// U3: Prior SrcOver content in the main segment must be sealed into a
    /// `DrawItem::Segment` before the `DrawItem::SsaaPath` (Z-order correctness).
    #[test]
    fn divert_path_to_ssaa_seals_prior_content() {
        use super::super::pipeline::PipelineKey;

        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        // Add SrcOver content to the current segment.
        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            vec![
                make_vertex(0.0, 0.0),
                make_vertex(10.0, 0.0),
                make_vertex(5.0, 5.0),
            ],
            &[0, 1, 2],
            PipelineKey::alpha_blend(),
        );
        assert!(draw_order.is_empty(), "SrcOver stays in segment");

        // Divert a path to SSAA — must seal the prior content first.
        DrawBatcher::divert_path_to_ssaa(
            &mut segment,
            &mut draw_order,
            &state,
            &[
                make_vertex(5.0, 5.0),
                make_vertex(15.0, 5.0),
                make_vertex(10.0, 15.0),
            ],
            &[0, 1, 2],
            flui_types::painting::BlendMode::SrcOver,
        );

        assert_eq!(
            draw_order.len(),
            2,
            "draw_order must have [Segment (sealed prior), SsaaPath]"
        );
        assert!(
            matches!(draw_order[0], DrawItem::Segment(_)),
            "draw_order[0] must be the sealed prior Segment"
        );
        assert!(
            matches!(draw_order[1], DrawItem::SsaaPath(_)),
            "draw_order[1] must be SsaaPath"
        );
    }

    // ── U4: device_bounds AABB is correct ────────────────────────────────────

    /// U4: The `device_bounds` in `SsaaPathOp` must be the AABB of the
    /// input vertices.
    #[test]
    fn divert_path_to_ssaa_device_bounds_is_vertex_aabb() {
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        DrawBatcher::divert_path_to_ssaa(
            &mut segment,
            &mut draw_order,
            &state,
            &[
                make_vertex(5.0, 10.0),
                make_vertex(30.0, 10.0),
                make_vertex(17.5, 40.0),
            ],
            &[0, 1, 2],
            flui_types::painting::BlendMode::SrcOver,
        );

        let DrawItem::SsaaPath(ref op) = draw_order[0] else {
            panic!("expected SsaaPath");
        };

        assert!(
            (op.device_bounds.left().0 - 5.0).abs() < 0.01,
            "left must be 5.0; got {}",
            op.device_bounds.left().0
        );
        assert!(
            (op.device_bounds.top().0 - 10.0).abs() < 0.01,
            "top must be 10.0; got {}",
            op.device_bounds.top().0
        );
        assert!(
            (op.device_bounds.right().0 - 30.0).abs() < 0.01,
            "right must be 30.0; got {}",
            op.device_bounds.right().0
        );
        assert!(
            (op.device_bounds.bottom().0 - 40.0).abs() < 0.01,
            "bottom must be 40.0; got {}",
            op.device_bounds.bottom().0
        );
    }

    // ── U5: empty indices produce no item ────────────────────────────────────

    /// U5: `divert_path_to_ssaa` with empty indices must produce nothing.
    #[test]
    fn divert_path_to_ssaa_empty_indices_is_noop() {
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        DrawBatcher::divert_path_to_ssaa(
            &mut segment,
            &mut draw_order,
            &state,
            &[make_vertex(0.0, 0.0), make_vertex(10.0, 0.0)],
            &[], // empty indices
            flui_types::painting::BlendMode::SrcOver,
        );

        assert!(
            draw_order.is_empty(),
            "empty indices must produce no draw item"
        );
    }

    // ── U6: tile rect covers the full right/bottom sub-pixel edge ────────────

    /// U6: `tile_x + tile_w` must be ≥ `ceil(right)`, and
    /// `tile_y + tile_h` must be ≥ `ceil(bottom)`, for sub-pixel path bounds.
    ///
    /// The previous `tile_w = ceil(width)` scheme failed this:
    ///   left=5.6, right=25.4 → width=19.8 → tile_x=5, tile_w=ceil(19.8)=20
    ///   → right_edge=25 < ceil(right)=26  ← sub-pixel clip bug.
    ///
    /// The corrected scheme:
    ///   tile_x = floor(left) = 5
    ///   tile_w = ceil(right)+1 − tile_x = 27−5 = 22  (includes +1 AA fringe)
    ///   → right_edge = 27 ≥ ceil(right)=26  ✓
    ///
    /// This test asserts the arithmetic directly against the formula used in
    /// `render_ssaa_path` — it would have caught the pre-fix under-coverage.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    #[test]
    fn tile_rect_covers_full_subpixel_right_and_bottom_edge() {
        struct Case {
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            vp_w: u32,
            vp_h: u32,
        }

        let cases = [
            // Original bug: floor(left)=5, ceil(width=19.8)=20 → tile right=25 < ceil(right)=26.
            Case {
                left: 5.6,
                top: 10.3,
                right: 25.4,
                bottom: 40.7,
                vp_w: 1920,
                vp_h: 1080,
            },
            // Integer bounds: no sub-pixel, must still cover.
            Case {
                left: 0.0,
                top: 0.0,
                right: 100.0,
                bottom: 100.0,
                vp_w: 800,
                vp_h: 600,
            },
            // Near-zero width.
            Case {
                left: 10.1,
                top: 10.1,
                right: 10.9,
                bottom: 10.9,
                vp_w: 200,
                vp_h: 200,
            },
        ];

        for c in &cases {
            let tile_x = c.left.floor().max(0.0) as u32;
            let tile_y = c.top.floor().max(0.0) as u32;
            // Corrected formula: ceil each edge, derive extent, add +1 AA fringe.
            let tile_right_edge = ((c.right.ceil() as u32) + 1).min(c.vp_w);
            let tile_bottom_edge = ((c.bottom.ceil() as u32) + 1).min(c.vp_h);
            let tile_w = tile_right_edge.saturating_sub(tile_x).max(1);
            let tile_h = tile_bottom_edge.saturating_sub(tile_y).max(1);

            let tile_right = tile_x + tile_w;
            let tile_bottom = tile_y + tile_h;

            assert!(
                tile_right >= c.right.ceil() as u32,
                "tile right edge ({}) must cover ceil(right={})={}",
                tile_right,
                c.right,
                c.right.ceil() as u32,
            );
            assert!(
                tile_bottom >= c.bottom.ceil() as u32,
                "tile bottom edge ({}) must cover ceil(bottom={})={}",
                tile_bottom,
                c.bottom,
                c.bottom.ceil() as u32,
            );
        }
    }

    // ── U7: scissor remap translates to tile-local 2× space ─────────────────

    /// U7: A full-frame scissor must be translated into tile-local 2× space
    /// when `render_ssaa_path` remaps the vertex segment.
    ///
    /// Concrete: tile at (50, 50), size 100×100; full-frame scissor (60,70,40,30).
    /// Expected tile-local 2× scissor:
    ///   inter_x = max(60,50) = 60, inter_y = max(70,50) = 70
    ///   inter_right = min(100,150) = 100, inter_bottom = min(100,150) = 100
    ///   local_x = (60−50)*2 = 20, local_y = (70−50)*2 = 40
    ///   local_w = (100−60)*2 = 80, local_h = (100−70)*2 = 60
    ///
    /// Without the fix: full-frame scissor (60,70,40,30) is applied verbatim
    /// against the 200×200 tile attachment.  The tile starts at pixel (0,0)
    /// in attachment space, so x=60 is far inside the tile but refers to the
    /// wrong frame of reference → the clip rect covers the wrong region.
    #[test]
    fn scissor_remap_produces_correct_tile_local_coords() {
        // Tile parameters (matching render_ssaa_path Step 1 output).
        let tile_x: u32 = 50;
        let tile_y: u32 = 50;
        let tile_w: u32 = 100;
        let tile_h: u32 = 100;
        let supersample_w: u32 = tile_w * 2;
        let supersample_h: u32 = tile_h * 2;

        // Full-frame scissor from `state.current_scissor()` at record time.
        let scissor: (u32, u32, u32, u32) = (60, 70, 40, 30);
        let (sx, sy, sw, sh) = scissor;

        // Remap (mirrors render_ssaa_path scissor-remap logic verbatim).
        let tile_right = tile_x + tile_w;
        let tile_bottom = tile_y + tile_h;
        let scis_right = sx + sw;
        let scis_bottom = sy + sh;

        let inter_x = sx.max(tile_x);
        let inter_y = sy.max(tile_y);
        let inter_right = scis_right.min(tile_right);
        let inter_bottom = scis_bottom.min(tile_bottom);

        let remapped = if inter_right <= inter_x || inter_bottom <= inter_y {
            (supersample_w, supersample_h, 1, 1) // sentinel: fully clipped
        } else {
            let local_x = (inter_x - tile_x) * 2;
            let local_y = (inter_y - tile_y) * 2;
            let local_w = (inter_right - inter_x) * 2;
            let local_h = (inter_bottom - inter_y) * 2;
            (local_x, local_y, local_w, local_h)
        };

        assert_eq!(
            remapped,
            (20, 40, 80, 60),
            "tile-local 2× scissor must be (20, 40, 80, 60); got {remapped:?}"
        );
    }

    // ── U7b: scissor entirely outside tile → fully-clipped sentinel ─────────

    /// U7b: A full-frame scissor that does not intersect the tile must produce
    /// the fully-clipped sentinel, not a garbage rect that accidentally covers
    /// part of the tile.
    ///
    /// Without the fix this case is impossible to distinguish from correct
    /// rendering — the full-frame scissor would be applied as-is and might
    /// accidentally clip to a different but non-empty region.
    #[test]
    fn scissor_fully_outside_tile_produces_sentinel() {
        let tile_x: u32 = 300;
        let tile_y: u32 = 0;
        let tile_w: u32 = 100;
        let tile_h: u32 = 100;
        let supersample_w: u32 = tile_w * 2;
        let supersample_h: u32 = tile_h * 2;

        // Scissor entirely to the LEFT of the tile (x∈[0..99], tile x∈[300..399]).
        let (sx, sy, sw, sh): (u32, u32, u32, u32) = (0, 0, 100, 100);

        let tile_right = tile_x + tile_w;
        let tile_bottom = tile_y + tile_h;
        let scis_right = sx + sw;
        let scis_bottom = sy + sh;

        let inter_x = sx.max(tile_x);
        let inter_y = sy.max(tile_y);
        let inter_right = scis_right.min(tile_right);
        let inter_bottom = scis_bottom.min(tile_bottom);

        let remapped = if inter_right <= inter_x || inter_bottom <= inter_y {
            (supersample_w, supersample_h, 1, 1)
        } else {
            let local_x = (inter_x - tile_x) * 2;
            let local_y = (inter_y - tile_y) * 2;
            let local_w = (inter_right - inter_x) * 2;
            let local_h = (inter_bottom - inter_y) * 2;
            (local_x, local_y, local_w, local_h)
        };

        assert_eq!(
            remapped,
            (supersample_w, supersample_h, 1, 1),
            "scissor entirely outside tile must produce the fully-clipped sentinel; got {remapped:?}"
        );
    }
}

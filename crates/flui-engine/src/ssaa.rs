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
//!   a 1× tile, and composites with the requested blend. Plus also resolves an
//!   independent geometry tile before clamping and interpolating the result.
//!
//! ## Surface / sample-count invariant
//!
//! The SSAA tile is a plain normal (non-multisampled) texture, just twice the
//! logical resolution.  `sample_count` stays 1 everywhere.  No stencil, no
//! `resolve_target`. This is compatible with advanced blend and opacity layers,
//! which also uses `sample_count: 1` pooled textures.
//!
//! ## Premultiplied correctness
//!
//! `shape.wgsl` emits PREMULTIPLIED colour (`vec4(rgb*a, a)`, shape.wgsl:51-53),
//! and the SrcOver tessellated pipeline uses `PREMULTIPLIED_ALPHA_BLENDING`
//! (src factor `One`, pipeline.rs:133). The 2× tile starts clear-transparent, so
//! the path accumulates premultiplied values over transparent. The box downsample
//! averages premultiplied values, which is linear-correct (premultiplied colour is
//! linear in coverage). Linear tile-safe operators use fixed-function factors.
//! Saturating Plus resolves geometry independently of paint alpha and clamps
//! the full additive result before interpolating it with the destination.

use std::sync::Arc;

use flui_foundation::geometry::Rect;

use crate::{
    advanced_blend::{AdvancedBlendOp, flush_advanced_layer},
    command_ir::SsaaPathOp,
    pipeline_cache::is_tile_safe_for_ssaa,
    pipeline_set::PipelineSet,
    replay::GpuReplay,
    resources::GpuResources,
    texture_pool::PooledTexture,
};

// ─── Downsample pipeline ───────────────────────────────────────────────────────

/// Alignment multiple for SSAA source-texture bucket dimensions.
///
/// Bucket dimensions are rounded UP to the next multiple of this value so that
/// the texture pool can reuse the same allocation across tiles whose exact
/// supersample sizes differ only by a few pixels. When the bucket equals the
/// supersample exactly (no padding) `crop_uv` = (1, 1) and the shader output
/// is bit-identical to the unbucketed path.
///
/// 64 was chosen as the minimum power-of-two that amortises pool fragmentation
/// while keeping wasted texels below ~4× for typical (>16 px) tiles.
const SSAA_BUCKET_ALIGNMENT: u32 = 64;

/// Fullscreen quad for the 2×→1× downsample pass.
///
/// Two triangles covering NDC [-1,1]×[-1,1]. UV (0,0) = top-left, (1,1) =
/// bottom-right (wgpu top-left origin). The vertex shader scales UV by
/// `crop_uv` to address only the content region inside a bucket allocation.
///
/// Hoisted to a module-level const so the buffer is created ONCE in
/// `SsaaDownsamplePipeline::new` and reused for every frame — avoid per-draw
/// `create_buffer_init` (allocation + upload on every path AA draw call).
#[rustfmt::skip]
const SSAA_QUAD_VERTICES: &[f32] = &[
    // position (x,y)   UV (u,v)
    -1.0,  1.0,         0.0, 0.0, // top-left
    -1.0, -1.0,         0.0, 1.0, // bottom-left
     1.0, -1.0,         1.0, 1.0, // bottom-right
    -1.0,  1.0,         0.0, 0.0, // top-left
     1.0, -1.0,         1.0, 1.0, // bottom-right
     1.0,  1.0,         1.0, 0.0, // top-right
];

/// Pipeline for the 2×→1× box-filter downsample pass used by SSAA path AA.
///
/// The pipeline samples a 2× supersampled source texture (premultiplied RGBA)
/// and averages the four sub-texels for each output pixel, producing a
/// premultiplied 1× tile ready for compositing.
///
/// One `SsaaDownsamplePipeline` is created per `PipelineSet` (keyed to the
/// surface format) and reused for every SSAA path in the frame.
// wgpu handle types do not implement Debug.
pub(crate) struct SsaaDownsamplePipeline {
    /// Render pipeline for the 4-tap box downsample.
    pub(crate) pipeline: wgpu::RenderPipeline,
    /// Bind group layout: binding 0 = source texture, binding 1 = linear sampler,
    /// binding 2 = crop_uv uniform buffer.
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    /// Fullscreen quad vertex buffer, created once and reused every draw call.
    pub(crate) quad_vertex_buffer: wgpu::Buffer,
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
                // binding 2: crop_uv uniform — scales vertex UVs to the content
                // region of the (possibly padded) pool bucket texture.
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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
                buffers: &[Some(wgpu::VertexBufferLayout {
                    // Two f32 position + two f32 UV per vertex.
                    array_stride: 4 * std::mem::size_of::<f32>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                })],
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

        // Create the fullscreen quad vertex buffer ONCE — reused for every
        // SSAA downsample draw call within this pipeline's lifetime.
        let quad_vertex_buffer = {
            use wgpu::util::DeviceExt as _;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SSAA Downsample Quad VB"),
                contents: bytemuck::cast_slice(SSAA_QUAD_VERTICES),
                usage: wgpu::BufferUsages::VERTEX,
            })
        };

        Self {
            pipeline,
            bind_group_layout,
            quad_vertex_buffer,
        }
    }
}

// ─── Pool-bucketing arithmetic ─────────────────────────────────────────────────

/// Round `value` up to the next multiple of `alignment`.
///
/// `alignment` must be non-zero; panics in debug builds if it is.
/// Saturating addition prevents overflow for extreme values (though pool
/// dimensions are bounded by `max_tex_dim` before this function is called).
///
/// # Examples
///
/// `round_up_to_alignment` is private to this module, so the arithmetic is
/// shown rather than compiled:
///
/// ```text
/// assert_eq!(round_up_to_alignment(130, 64), 192);
/// assert_eq!(round_up_to_alignment(128, 64), 128); // already aligned
/// assert_eq!(round_up_to_alignment(1, 64), 64);
/// ```
fn round_up_to_alignment(value: u32, alignment: u32) -> u32 {
    debug_assert!(alignment > 0, "alignment must be non-zero");
    value.saturating_add(alignment - 1) / alignment * alignment
}

// ─── Replay helper ─────────────────────────────────────────────────────────────

#[expect(clippy::too_many_arguments)]
impl GpuReplay {
    /// Replay a `DrawItem::SsaaPath`: render the path into a 2× tile,
    /// box-downsample to a premultiplied 1× tile, then composite onto the target.
    ///
    /// ## Composite step (Step 5) — PR-4 blend routing
    ///
    /// After producing the AA'd 1× tile, the composite is selected by `op.blend`:
    ///
    /// - **Plus**: render and downsample a separate white geometry membership
    ///   plane, then clamp the full additive blend before interpolating coverage.
    ///   A sampleable destination is required.
    /// - **other tile-safe** (`is_tile_safe_for_ssaa(op.blend)` = true): composite via
    ///   `flush_texture_batch_premultiplied` with `blend_state_for(op.blend)`.
    ///   Transparent SSAA padding is a no-op for these modes (dst preserved).
    ///   Handles Dst, DstOver, DstOut, SrcATop and Xor alongside SrcOver.
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
    pub(crate) fn render_ssaa_path(
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
    ) -> crate::error::EngineResult<()> {
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

        let parent_origin = self.attachment_origin;
        let left = parent_origin.0 as f64;
        let top = parent_origin.1 as f64;
        let right = left + f64::from(vp_w);
        let bottom = top + f64::from(vp_h);
        let tile_x = op.device_bounds.left().floor().clamp(left, right) as i64;
        let tile_y = op.device_bounds.top().floor().clamp(top, bottom) as i64;
        let tile_right_edge = (op.device_bounds.right().ceil() + 1.0).clamp(left, right) as i64;
        let tile_bottom_edge = (op.device_bounds.bottom().ceil() + 1.0).clamp(top, bottom) as i64;

        // An invisible path is a no-op, before allocating or reading a backdrop.
        if tile_x >= tile_right_edge || tile_y >= tile_bottom_edge {
            return Ok(());
        }
        let tile_w = (tile_right_edge - tile_x) as u32;
        let tile_h = (tile_bottom_edge - tile_y) as u32;

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
                &op.segment,
                viewport_size,
                device,
                queue,
                pipelines,
                resources,
                encoder,
                crate::render_target::RenderTarget {
                    view: target_view,
                    texture: surface_texture,
                },
            )?;
            return Ok(());
        }

        // ── Step 2: acquire a 2× pooled texture (bucketed) ───────────────────
        //
        // tile_w ≤ max_tile_half = max_tex_dim/2 (guaranteed by the fallback
        // above), so tile_w*2 ≤ max_tex_dim. No wgpu create_texture error.
        let supersample_w = tile_w * 2;
        let supersample_h = tile_h * 2;

        // Pool bucketing: round supersample dims UP to the next multiple of
        // SSAA_BUCKET_ALIGNMENT, capped at max_tex_dim.  This promotes texture
        // reuse across paths with slightly different sizes (e.g. two tiles of
        // 130×132 and 126×128 both acquire the 192×192 bucket rather than two
        // distinct sizes).  When the bucket equals the supersample exactly,
        // crop_uv = (1,1) and the shader output is bit-identical.
        let bucket_w = round_up_to_alignment(supersample_w, SSAA_BUCKET_ALIGNMENT).min(max_tex_dim);
        let bucket_h = round_up_to_alignment(supersample_h, SSAA_BUCKET_ALIGNMENT).min(max_tex_dim);

        if self.filter_attachment_depth != 0 {
            resources.admit_foreground_target((bucket_w, bucket_h), surface_format, 1)?;
            resources.admit_foreground_target((tile_w, tile_h), surface_format, 1)?;
            let downsample_count = if op.blend == flui_painting::BlendMode::Plus {
                2
            } else {
                1
            };
            let work = (tile_w as usize)
                .checked_mul(tile_h as usize)
                .and_then(|pixels| pixels.checked_mul(4 * downsample_count))
                .ok_or(crate::error::EngineError::PreparedResourceOverflow)?;
            op.segment.budget.admit_effect_work(work)?;
            resources.reserve_prepared(crate::device_domain::PreparedCost {
                gpu_bytes: 80 * downsample_count,
                cpu_bytes: 80 * downsample_count,
                objects: 6 * downsample_count,
            })?;
        }

        let coverage_backdrop = if op.blend == flui_painting::BlendMode::Plus {
            if surface_texture.is_none() {
                return Err(crate::error::EngineError::CompositeBackdropUnavailable);
            }
            crate::portable_coverage::PortableCoveragePipeline::validate_device_limits(
                device,
                surface_format,
            )?;
            let bytes = (u64::from(bucket_w) * u64::from(bucket_h)
                + u64::from(tile_w) * u64::from(tile_h))
            .checked_mul(u64::from(surface_format.block_copy_size(None).unwrap_or(4)))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(crate::error::GeometryError::Unrepresentable {
                context: "SSAA coverage allocation",
            })?;
            resources.reserve_prepared(crate::device_domain::PreparedCost {
                gpu_bytes: bytes,
                cpu_bytes: 0,
                objects: 4,
            })?;
            Some(crate::portable_coverage::PreparedCoverageBackdrop::prepare(
                device,
                resources,
                crate::render_target::RenderTarget {
                    view: target_view,
                    texture: surface_texture,
                },
                (
                    (tile_x - parent_origin.0) as u32,
                    (tile_y - parent_origin.1) as u32,
                ),
                (tile_w, tile_h),
                surface_format,
                encoder,
            )?)
        } else {
            None
        };

        let super_tex =
            resources
                .layer_texture_pool_mut()
                .acquire(bucket_w, bucket_h, surface_format);
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
        let scale_x = vp_w as f32 / tile_w as f32;
        let scale_y = vp_h as f32 / tile_h as f32;

        let mut remapped_segment = op.segment.try_clone_for_remap()?;
        remapped_segment.rebase_attachment(
            f64::from(tile_origin_x),
            f64::from(tile_origin_y),
            0.5,
            0.5,
        );
        for v in &mut remapped_segment.vertices {
            v.position[0] = (v.position[0] - tile_origin_x) * scale_x;
            v.position[1] = (v.position[1] - tile_origin_y) * scale_y;
        }

        // The vertices above just became tile-local, so `world_pos` in the
        // fragment stage is tile-local too — and a tessellated batch's SDF clip
        // is in FULL-FRAME device space. Left alone it would be compared
        // against the wrong coordinates and displace or erase the path. Same
        // remap, same helper as the grown-offscreen path.
        Self::remap_segment_clips(
            &mut remapped_segment,
            (tile_origin_x, tile_origin_y),
            (scale_x, scale_y),
        );

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
                let tile_right = tile_x + i64::from(tile_w);
                let tile_bottom = tile_y + i64::from(tile_h);

                // Full-frame scissor right/bottom edges.
                let scis_right = sx.saturating_add(i64::from(sw));
                let scis_bottom = sy.saturating_add(i64::from(sh));

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
                    batch.scissor =
                        Some((i64::from(supersample_w), i64::from(supersample_h), 1, 1));
                } else {
                    // Translate to tile-local coordinates and scale to 2× space.
                    let local_x = (inter_x - tile_x) * 2;
                    let local_y = (inter_y - tile_y) * 2;
                    let local_w = (inter_right - inter_x) * 2;
                    let local_h = (inter_bottom - inter_y) * 2;
                    batch.scissor = Some((local_x, local_y, local_w as u32, local_h as u32));
                }
            }
        }

        // Flush the remapped segment into the 2× texture.
        // `viewport_size = (supersample_w, supersample_h)` so the scissor covers
        // the full 2× render target.  The shape shader's static viewport uniform
        // `(vp_w, vp_h)` combined with the pre-scaled positions produces NDC that
        // fills the tile, rendering it into the 2× texture → supersampled.
        self.attachment_origin = (0, 0);
        let tile_result = self.flush_segment(
            &remapped_segment,
            (supersample_w, supersample_h),
            device,
            queue,
            pipelines,
            resources,
            encoder,
            crate::render_target::RenderTarget::sampleable(super_view, super_tex.texture()),
        );
        self.attachment_origin = parent_origin;
        tile_result?;

        // ── Step 4: box-downsample 2× → 1× premultiplied tile ────────────────
        //
        // Pass the exact supersample dims (not the bucket) so the function can
        // compute crop_uv = supersample/bucket correctly.

        let one_x_tile = self.downsample_ssaa_tile(
            &super_tex,
            supersample_w,
            supersample_h,
            surface_format,
            device,
            pipelines,
            resources,
            encoder,
        );

        // 2× tile is no longer needed — drop it back to the pool now, before
        // the composite pass, to minimise peak texture memory.
        // Geometry coverage is independent of paint alpha, including alpha zero.
        let coverage_tile = if op.blend == flui_painting::BlendMode::Plus {
            let mut membership = remapped_segment.try_clone_for_remap()?;
            for vertex in &mut membership.vertices {
                vertex.color = [1.0; 4];
            }
            let supersampled =
                resources
                    .layer_texture_pool_mut()
                    .acquire(bucket_w, bucket_h, surface_format);
            {
                let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("SSAA independent coverage clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: supersampled.view(),
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
            }
            self.attachment_origin = (0, 0);
            let membership_result = self.flush_segment(
                &membership,
                (supersample_w, supersample_h),
                device,
                queue,
                pipelines,
                resources,
                encoder,
                crate::render_target::RenderTarget::sampleable(
                    supersampled.view(),
                    supersampled.texture(),
                ),
            );
            self.attachment_origin = parent_origin;
            membership_result?;
            Some(self.downsample_ssaa_tile(
                &supersampled,
                supersample_w,
                supersample_h,
                surface_format,
                device,
                pipelines,
                resources,
                encoder,
            ))
        } else {
            None
        };
        drop(super_tex);

        // ── Step 5: composite the 1× tile onto the target ────────────────────
        //
        // PR-4 blend routing (see function doc):
        //   - tile-safe → fixed-function premul blend (SrcOver pipeline for now)
        //   - advanced   → flush_advanced_layer (dst-read W3C composite)
        //   - SrcOver (PR-3 baseline) → tile-safe path

        let composite_bounds = Rect::from_xywh(
            f64::from(tile_x as f32),
            f64::from(tile_y as f32),
            f64::from(tile_w as f32),
            f64::from(tile_h as f32),
        );

        if let Some(coverage) = coverage_tile {
            coverage_backdrop
                .expect("BUG: Plus coverage admitted its backdrop before SSAA work")
                .composite(
                    pipelines.portable_coverage(device),
                    device,
                    encoder,
                    crate::render_target::RenderTarget {
                        view: target_view,
                        texture: surface_texture,
                    },
                    &one_x_tile,
                    &coverage,
                    op.blend,
                    None,
                )?;
        } else if op.blend.is_advanced() {
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
                    clip: None,
                };
                self.prepare_viewport_binding(device, pipelines, resources)?;
                self.admit_filter_composite(viewport_size, surface_format, resources)?;
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
                    Some(&self.viewport_bind_group),
                    self.attachment_origin,
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
                // Same fallback as AdvancedShape in replay.
                tracing::warn!(
                    mode = ?op.blend,
                    "SSAA path advanced blend reached a view_only target; \
                     falling back to SrcOver (caller must pass sampleable target)"
                );
                let instance = crate::instancing::TextureInstance::new(
                    composite_bounds,
                    flui_painting::styling::Color::WHITE,
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
            // exactly, so DstOut, DstOver, Xor, SrcATop, Dst, and SrcOver
            // all composite the 1× tile with their correct factors.
            debug_assert!(
                is_tile_safe_for_ssaa(op.blend),
                "non-advanced, non-tile-safe mode {:?} reached SSAA tile composite — \
                 coverage-destructive modes must stay on the tessellated path",
                op.blend
            );
            let instance = crate::instancing::TextureInstance::new(
                composite_bounds,
                flui_painting::styling::Color::WHITE,
            );
            let _ = self.texture_batch.add(instance);
            self.prepare_viewport_binding(device, pipelines, resources)?;
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
        Ok(())
    }

    /// Box-downsample a 2× pooled `source_texture` into a fresh 1× tile.
    ///
    /// ## Pool bucketing
    ///
    /// The `source_tex` was acquired at the exact supersample dimensions
    /// `(supersample_w, supersample_h)`, but the pool may have returned a
    /// larger bucket (next multiple of [`SSAA_BUCKET_ALIGNMENT`], capped at
    /// the device max texture dimension). The `crop_uv` uniform
    /// `(supersample_w/bucket_w, supersample_h/bucket_h)` scales vertex UVs
    /// so only the content region is sampled.
    ///
    /// When the bucket equals the supersample (no padding, or exact multiple),
    /// `crop_uv` = (1.0, 1.0) and the output is bit-identical to the
    /// pre-bucketing path.
    ///
    /// Returns the 1× [`PooledTexture`]; the caller composites then drops it.
    fn downsample_ssaa_tile(
        &mut self,
        source_tex: &PooledTexture,
        supersample_w: u32,
        supersample_h: u32,
        output_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> PooledTexture {
        // ── Output (1×) dimensions ─────────────────────────────────────────
        // The logical output is half the supersample size (which is always the
        // original tile_w/tile_h from render_ssaa_path step 2).
        let output_w = supersample_w / 2;
        let output_h = supersample_h / 2;

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

        // ── crop_uv: ratio of content to bucket ────────────────────────────
        //
        // The source texture returned by the pool has bucket dimensions
        // `(bucket_w, bucket_h)` — the actual allocated wgpu texture size.
        // The pool always returns a texture ≥ (supersample_w, supersample_h),
        // so the crop ratio is read from the handle's real dimensions rather
        // than assumed from the request.
        let bucket_w = source_tex.width();
        let bucket_h = source_tex.height();

        let crop_uv_x = supersample_w as f32 / bucket_w as f32;
        let crop_uv_y = supersample_h as f32 / bucket_h as f32;

        // crop_uv uniform: [x, y, pad, pad] — matches the WGSL struct layout.
        let crop_uv_data: [f32; 4] = [crop_uv_x, crop_uv_y, 0.0, 0.0];

        let crop_uv_buffer = {
            use wgpu::util::DeviceExt as _;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SSAA Crop UV Uniform"),
                contents: bytemuck::cast_slice(&crop_uv_data),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };

        // Build the bind group: source 2× texture + linear sampler + crop_uv.
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: crop_uv_buffer.as_entire_binding(),
                },
            ],
        });

        // Run the downsample pass using the cached quad vertex buffer.
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
            // Cached VB — no per-draw allocation.
            pass.set_vertex_buffer(0, pipelines.ssaa_downsample.quad_vertex_buffer.slice(..));
            pass.draw(0..6, 0..1);
        }

        one_x_tile
    }
}

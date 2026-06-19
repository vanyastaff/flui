//! Path, vertices, line, and shadow record methods: draw_path, draw_vertices, line, draw_shadow.

use flui_painting::{BlendMode, Paint, PaintStyle};
use flui_types::{
    Offset, Point,
    geometry::{Pixels, px},
    painting::path::Path,
    styling::Color,
};

use super::{
    super::{
        command_ir::{DrawItem, DrawSegment},
        path_cache::PathCache,
        pipeline::{self, PipelineKey},
        state_stack::GpuStateStack,
        vertex::Vertex,
    },
    DrawBatcher,
};

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl DrawBatcher {
    /// Record a stroked line segment.
    ///
    /// Always tessellated (no instanced stroke pipeline).
    /// `line` does not read opacity — no opacity baking is performed.
    pub(in super::super) fn line(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        paint: &Paint,
    ) {
        self.prime_tessellator_scale(state);
        match self.tessellator.tessellate_line(p1, p2, paint) {
            Ok((vertices, indices)) => {
                #[cfg(debug_assertions)]
                tracing::trace!(
                    "DrawBatcher::line: {} vertices, {} indices",
                    vertices.len(),
                    indices.len()
                );
                Self::submit_transformed_geometry(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
            Err(e) => {
                tracing::error!("DrawBatcher::line: tessellation failed — {}", e);
            }
        }
    }

    /// Record a multi-layer approximated drop shadow for `path`.
    ///
    /// # State mutation
    ///
    /// This method takes `state: &mut GpuStateStack` because each blur layer
    /// applies a per-layer translate via `state.save()` / `state.translate()`
    /// / `state.restore()`.  The save/restore balance is maintained strictly:
    /// every iteration pushes exactly one save and pops it before the next
    /// iteration.  The net depth change across the entire call is zero, so the
    /// T7 frame-boundary `debug_assert_balanced` remains satisfied.
    ///
    /// # Algorithm
    ///
    /// Material Design-style multi-pass approximation: the shadow path is
    /// tessellated `num_layers` times (≤ 8) with geometrically decreasing alpha
    /// to simulate radial blur.  The tessellator scale is primed **once** before
    /// the loop — the per-layer `translate` does not change scale, so the
    /// flatten tolerance captured before the loop is correct for every layer.
    ///
    /// `draw_shadow` does not read opacity — no opacity baking is performed.
    pub(in super::super) fn draw_shadow(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &mut GpuStateStack,
        path: &Path,
        color: Color,
        elevation: f32,
    ) {
        let blur_radius = elevation.max(0.0);
        let offset_y = elevation / 2.0;

        if blur_radius < 0.1 {
            return;
        }

        // Max 8 layers for performance.
        let num_layers = (blur_radius / 2.0).ceil().min(8.0) as usize;

        if num_layers == 0 {
            return;
        }

        let alpha_per_layer = f32::from(color.a) / num_layers as f32;

        // Prime the tessellator's flatten tolerance to the current CTM scale so
        // shadow curves don't facet on HiDPI / scaled frames. The per-layer
        // `translate` below only shifts the path (no scale change), so the scale
        // captured here is correct for every layer. Without this, the shadow
        // path would tessellate at whatever `max_scale` a previous draw left
        // behind (stale-scale hazard).
        self.prime_tessellator_scale(state);

        for i in 0..num_layers {
            let offset_scale = (i as f32 + 1.0) / num_layers as f32;
            let current_blur = blur_radius * offset_scale;

            let shadow_alpha = (alpha_per_layer * (1.0 - offset_scale * 0.5)) as u8;
            let shadow_color = Color::rgba(color.r, color.g, color.b, shadow_alpha);
            let shadow_paint = Paint::fill(shadow_color);

            // Push a per-layer translate so the tessellated geometry is baked
            // at the offset position (`submit_transformed_geometry` reads the
            // CTM at call time — shape.wgsl has no model matrix).
            state.save();
            state.translate(Offset::new(
                px(current_blur * 0.5),
                px(offset_y + current_blur * 0.5),
            ));

            match self
                .tessellator
                .tessellate_flui_path_fill(path, &shadow_paint)
            {
                Ok((vertices, indices)) => {
                    Self::submit_transformed_geometry(
                        segment,
                        draw_order,
                        state,
                        vertices,
                        &indices,
                        PipelineKey::alpha_blend(),
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to tessellate shadow path: {}", e);
                }
            }

            state.restore();
        }
    }

    /// Record a filled or stroked path, using the per-frame tessellation cache
    /// to avoid re-tessellating identical paths.
    ///
    /// # Branch ordering (must be preserved for byte-identical output)
    ///
    /// 1. Prime the tessellator flatten-tolerance from the current CTM scale.
    /// 2. **Dashed-stroke early return** — dashes are not cached (the pattern
    ///    affects geometry but is not part of `compute_path_hash`; caching
    ///    would collide a solid and a dashed stroke of the same path).
    /// 3. **Cache hit** — reconstruct `Vertex`s from UNTRANSFORMED cached
    ///    positions with the *current* `paint.color`, then submit.
    /// 4. **Cache miss** — tessellate fill or stroke, extract untransformed
    ///    positions, store in cache, then submit.
    ///
    /// `draw_path` does not read opacity — no opacity baking is performed,
    /// consistent with the other record methods in this module.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state are disjoint WgpuPainter fields \
                  passed as separate borrows; path geometry parameters are all necessary"
    )]
    pub(in super::super) fn draw_path(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        path: &Path,
        paint: &Paint,
    ) {
        // Snapshot world scale once: it drives flatten-tolerance in the tessellator
        // AND the cache-key bucket, so a single read guarantees they can never desync
        // (scale-1 geometry must not be reused at scale 8, which would facet).
        let max_scale = state.max_scale();
        self.tessellator.set_max_scale(max_scale);

        // Dashed strokes cannot use the path cache: the dash pattern affects
        // geometry but is not part of compute_path_hash, so caching would
        // collide a solid and a dashed stroke of the same path.
        if paint.style != PaintStyle::Fill
            && let Some(ref dash) = paint.dash_pattern
        {
            match self
                .tessellator
                .tessellate_flui_path_dashed_stroke(path, paint, dash)
            {
                Ok((vertices, indices)) => {
                    // Bake current_transform into vertices: shape.wgsl has no model matrix.
                    Self::submit_transformed_geometry(
                        segment,
                        draw_order,
                        state,
                        vertices,
                        &indices,
                        pipeline::pipeline_key_from_paint(paint),
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to tessellate dashed path stroke: {}", e);
                }
            }
            return;
        }

        // Determine whether this path fill should be SSAA-routed:
        //   - Fill style (not stroke).
        //   - SrcOver blend (non-SrcOver paths go to PR-4's path).
        //   - Path AABB area ≥ SSAA_AREA_THRESHOLD_PX²: tiny paths (icon
        //     sub-elements, micro-decorations) yield no visible AA benefit and
        //     pay 5 render passes + 2 texture acquisitions for nothing.  Below
        //     the threshold they join the batched tessellated draw call instead
        //     (same quality as pre-PR-3 for those sizes), preserving the engine's
        //     draw-batching design for the common small-path case.
        //
        // Closed-form shapes (rect/rrect/circle/oval/arc) never reach `draw_path`
        // — they go through `batches/shapes.rs` → instanced SDF.  So any Fill
        // arriving here IS an arbitrary path.
        //
        // The AABB is computed lazily (only when style==Fill && mode==SrcOver)
        // so the check adds no cost to the stroke / non-SrcOver paths.
        //
        let is_srcover_fill = paint.style == PaintStyle::Fill
            && paint.blend_mode == BlendMode::SrcOver
            && path_aabb_area_device_px_sq(path, state) >= SSAA_AREA_THRESHOLD_PX_SQ;

        // Compute cache key from path geometry + paint tessellation parameters
        // + the quantized world scale (so a scale-1 entry is not reused at a
        // larger scale with scale-1 chord density).
        let path_hash = PathCache::compute_path_hash(
            path,
            paint.style,
            paint.stroke_width,
            paint.stroke_cap,
            paint.stroke_join,
            max_scale,
        );

        // Check cache for previously tessellated geometry.
        if let Some((positions, cached_indices)) = self.path_cache.get(path_hash) {
            // Reconstruct full Vertex data with current paint color.
            // The cache stores UNTRANSFORMED positions; bake the current transform now.
            let rgba = paint.color.to_rgba_f32_array();
            let vertices: Vec<Vertex> = positions
                .iter()
                .map(|&pos| Vertex::new(pos, rgba, [0.0, 0.0]))
                .collect();
            let indices: Vec<u32> = cached_indices.to_vec();

            if is_srcover_fill {
                Self::submit_transformed_and_divert_to_ssaa(
                    segment, draw_order, state, vertices, &indices,
                );
            } else {
                // Bake current_transform into vertices: shape.wgsl has no model matrix.
                Self::submit_transformed_geometry(
                    segment,
                    draw_order,
                    state,
                    vertices,
                    &indices,
                    pipeline::pipeline_key_from_paint(paint),
                );
            }
            return;
        }

        // Cache miss — tessellate and store.
        let result = if paint.style == PaintStyle::Fill {
            self.tessellator.tessellate_flui_path_fill(path, paint)
        } else {
            self.tessellator.tessellate_flui_path_stroke(path, paint)
        };

        match result {
            Ok((vertices, indices)) => {
                // Extract position data for cache BEFORE baking the transform.
                // The cache stores local (untransformed) positions so that cached
                // geometry can be re-used across frames with different transforms.
                let positions: Vec<[f32; 2]> = vertices.iter().map(|v| v.position).collect();
                self.path_cache
                    .insert(path_hash, positions, indices.clone());

                if is_srcover_fill {
                    Self::submit_transformed_and_divert_to_ssaa(
                        segment, draw_order, state, vertices, &indices,
                    );
                } else {
                    // Bake current_transform into vertices: shape.wgsl has no model matrix.
                    Self::submit_transformed_geometry(
                        segment,
                        draw_order,
                        state,
                        vertices,
                        &indices,
                        pipeline::pipeline_key_from_paint(paint),
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to tessellate path: {}", e);
            }
        }
    }

    /// Transform vertices by the current CTM, then divert to SSAA.
    ///
    /// Factored out of `draw_path` so the transform+divert sequence is identical
    /// for cache-hit and cache-miss paths.
    fn submit_transformed_and_divert_to_ssaa(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        mut vertices: Vec<Vertex>,
        indices: &[u32],
    ) {
        let transform = state.current_transform();
        for v in &mut vertices {
            let transformed = transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
            v.position = [transformed.x, transformed.y];
        }
        Self::divert_path_to_ssaa(segment, draw_order, state, &vertices, indices);
    }

    /// Draw indexed triangle geometry with per-vertex color + uv.
    ///
    /// # Validation
    ///
    /// Returns early (silently) on empty input or a color-count mismatch in
    /// debug builds (the mismatch is logged via `tracing::error!`).
    ///
    /// # `tex_coords` parameter
    ///
    /// The per-vertex uv extraction IS implemented (the `tex_coords` slice is
    /// consumed at the per-vertex loop, copied into `Vertex::tex_coord`, and
    /// baked into the GPU vertex buffer).  What is NOT yet wired is the
    /// **texture-binding pipeline path**: `pipeline_key_from_paint(paint)`
    /// returns a solid-color pipeline today, so the uv values reach the vertex
    /// shader but the fragment shader has no texture to sample.  A textured
    /// pipeline-key variant is a follow-up audit item; until then `tex_coords`
    /// callers pre-populate the vertex stream for forward-compat.
    ///
    /// `draw_vertices` does not read opacity — no opacity baking is performed,
    /// consistent with the other record methods in this module.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state are disjoint WgpuPainter fields \
                  passed as separate borrows; vertex geometry slices are all necessary"
    )]
    pub(in super::super) fn draw_vertices(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: &[Point<Pixels>],
        colors: Option<&[Color]>,
        tex_coords: Option<&[Point<Pixels>]>,
        indices: &[u16],
        paint: &Paint,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "DrawBatcher::draw_vertices: vertices={}, indices={}",
            vertices.len(),
            indices.len()
        );

        // Validate input
        if vertices.is_empty() || indices.is_empty() {
            return;
        }

        if let Some(colors_arr) = colors
            && colors_arr.len() != vertices.len()
        {
            #[cfg(debug_assertions)]
            tracing::error!(
                "DrawVertices: color count ({}) doesn't match vertex count ({})",
                colors_arr.len(),
                vertices.len()
            );
            return;
        }

        // Convert to our Vertex format
        let default_color = paint.color;
        let gpu_vertices: Vec<Vertex> = vertices
            .iter()
            .enumerate()
            .map(|(i, pos)| {
                let color = colors
                    .and_then(|c| c.get(i))
                    .copied()
                    .unwrap_or(default_color);

                let uv = tex_coords
                    .and_then(|tc| tc.get(i))
                    .map_or([0.0, 0.0], |p| [p.x.0, p.y.0]);

                Vertex {
                    position: [pos.x.0, pos.y.0],
                    color: color.to_f32_array(),
                    tex_coord: uv,
                }
            })
            .collect();

        // Convert indices to u32
        let gpu_indices: Vec<u32> = indices.iter().map(|&i| u32::from(i)).collect();

        // Add to tessellated geometry (bypassing tessellator since we already have
        // triangles).  Bake current_transform into vertex positions: shape.wgsl has
        // no model-matrix uniform.
        Self::submit_transformed_geometry(
            segment,
            draw_order,
            state,
            gpu_vertices,
            &gpu_indices,
            pipeline::pipeline_key_from_paint(paint),
        );
    }
}

// ─── Module-level helpers ─────────────────────────────────────────────────────

/// Minimum path AABB area (in device-pixel²) that qualifies for SSAA routing.
///
/// Paths whose bounding-box area is below this threshold are rendered via the
/// normal batched tessellated draw call (shared GPU pass, zero offscreen
/// overhead).  Paths at or above the threshold are routed to SSAA (5 render
/// passes, 2 pooled-texture acquisitions) for proper sub-pixel AA on visible
/// diagonal edges.
///
/// 256 px² ≈ 16 × 16 device pixels.  Empirically, paths smaller than this
/// produce aliasing that is imperceptible at typical DPR and viewing distances;
/// the SSAA cost is not justified.
pub(in super::super) const SSAA_AREA_THRESHOLD_PX_SQ: f32 = 256.0;

/// Compute the device-pixel² area of a path's axis-aligned bounding box,
/// scaled by the current CTM (so the area reflects actual screen size).
///
/// Uses `path.compute_bounds()` — a pure computation over path commands with
/// no tessellation.  The CTM scale is applied as `area × max_scale²` because
/// the AABB in local space must be scaled to device pixels.
///
/// Used exclusively by `draw_path` to decide SSAA eligibility.
fn path_aabb_area_device_px_sq(path: &Path, state: &GpuStateStack) -> f32 {
    let local_bounds = path.compute_bounds();
    let w = local_bounds.width().0.max(0.0);
    let h = local_bounds.height().0.max(0.0);
    // Scale each edge by max_scale to get device-pixel extent.
    let scale = state.max_scale();
    let device_w = w * scale;
    let device_h = h * scale;
    device_w * device_h
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod threshold_tests {
    use flui_painting::{BlendMode, Paint, PaintStyle};
    use flui_types::{geometry::px, painting::path::Path};

    use super::{
        super::super::{
            command_ir::{DrawItem, DrawSegment},
            state_stack::GpuStateStack,
        },
        DrawBatcher, SSAA_AREA_THRESHOLD_PX_SQ,
    };

    fn make_batcher() -> DrawBatcher {
        DrawBatcher::new()
    }

    fn rect_path(w: f32, h: f32) -> Path {
        use flui_types::Rect;
        Path::rectangle(Rect::from_xywh(px(0.0), px(0.0), px(w), px(h)))
    }

    // ── T1: path below threshold → batched tessellated (no SsaaPath) ─────────

    /// T1: A SrcOver fill path whose AABB area is BELOW `SSAA_AREA_THRESHOLD_PX_SQ`
    /// must NOT produce `DrawItem::SsaaPath`.  It must join the batched tessellated
    /// path (zero offscreen overhead).
    ///
    /// Without the threshold gate, every SrcOver fill — including a 1px dot —
    /// issued 5 render passes, breaking the engine's draw-batching model.
    ///
    /// The path is a 15×15 square → area = 225 px² < 256 px² (threshold).
    #[test]
    fn small_path_below_threshold_does_not_produce_ssaa_path() {
        let mut batcher = make_batcher();
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test(); // identity CTM, scale=1

        let path = rect_path(15.0, 15.0); // area = 225 px² < 256 threshold
        const { assert!(15.0_f32 * 15.0 < SSAA_AREA_THRESHOLD_PX_SQ) };

        let paint = Paint {
            style: PaintStyle::Fill,
            blend_mode: BlendMode::SrcOver,
            ..Default::default()
        };

        batcher.draw_path(&mut segment, &mut draw_order, &state, &path, &paint);

        // No SsaaPath item must appear in draw_order.
        let ssaa_count = draw_order
            .iter()
            .filter(|item| matches!(item, DrawItem::SsaaPath(_)))
            .count();
        assert_eq!(
            ssaa_count, 0,
            "15×15 path (below threshold) must not produce SsaaPath; \
             got {ssaa_count} SsaaPath items"
        );
    }

    // ── T2: path at or above threshold → SsaaPath ────────────────────────────

    /// T2: A SrcOver fill path whose AABB area is AT OR ABOVE
    /// `SSAA_AREA_THRESHOLD_PX_SQ` must produce `DrawItem::SsaaPath`.
    ///
    /// The path is a 17×17 square → area = 289 px² ≥ 256 px² (threshold).
    ///
    /// Without the threshold gate all paths produced SsaaPath unconditionally,
    /// but with it, only sufficiently large paths do — this test ensures the
    /// threshold is not accidentally over-conservative.
    #[test]
    fn large_path_at_or_above_threshold_produces_ssaa_path() {
        let mut batcher = make_batcher();
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test(); // identity CTM, scale=1

        let path = rect_path(17.0, 17.0); // area = 289 px² ≥ 256 threshold
        const { assert!(17.0_f32 * 17.0 >= SSAA_AREA_THRESHOLD_PX_SQ) };

        let paint = Paint {
            style: PaintStyle::Fill,
            blend_mode: BlendMode::SrcOver,
            ..Default::default()
        };

        batcher.draw_path(&mut segment, &mut draw_order, &state, &path, &paint);

        let ssaa_count = draw_order
            .iter()
            .filter(|item| matches!(item, DrawItem::SsaaPath(_)))
            .count();
        assert_eq!(
            ssaa_count, 1,
            "17×17 path (at threshold) must produce exactly one SsaaPath; \
             got {ssaa_count}"
        );
    }

    // ── T3: stroke path is never SSAA-routed ─────────────────────────────────

    /// T3: A stroked path (PaintStyle::Stroke) must NEVER produce
    /// `DrawItem::SsaaPath`, regardless of size.  SSAA is only for fills.
    #[test]
    fn stroke_path_never_produces_ssaa_path() {
        let mut batcher = make_batcher();
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        // Large enough that it would be SSAA-routed if this were a fill.
        let path = rect_path(100.0, 100.0);

        let paint = Paint {
            style: PaintStyle::Stroke,
            blend_mode: BlendMode::SrcOver,
            stroke_width: 2.0,
            ..Default::default()
        };

        batcher.draw_path(&mut segment, &mut draw_order, &state, &path, &paint);

        let ssaa_count = draw_order
            .iter()
            .filter(|item| matches!(item, DrawItem::SsaaPath(_)))
            .count();
        assert_eq!(
            ssaa_count, 0,
            "stroke path must never produce SsaaPath; got {ssaa_count}"
        );
    }

    // ── T4: non-SrcOver fill is never SSAA-routed ────────────────────────────

    /// T4: A non-SrcOver fill (e.g. SrcOut) must NEVER produce
    /// `DrawItem::SsaaPath`.  SSAA is SrcOver-only in PR-3.
    #[test]
    fn non_srcover_fill_never_produces_ssaa_path() {
        let mut batcher = make_batcher();
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        let path = rect_path(200.0, 200.0); // large — would SSAA if SrcOver

        let paint = Paint {
            style: PaintStyle::Fill,
            blend_mode: BlendMode::SrcOut,
            ..Default::default()
        };

        batcher.draw_path(&mut segment, &mut draw_order, &state, &path, &paint);

        let ssaa_count = draw_order
            .iter()
            .filter(|item| matches!(item, DrawItem::SsaaPath(_)))
            .count();
        assert_eq!(
            ssaa_count, 0,
            "non-SrcOver fill must never produce SsaaPath; got {ssaa_count}"
        );
    }
}

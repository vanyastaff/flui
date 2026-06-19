//! Record-side draw accumulation helpers extracted from `WgpuPainter`.
//!
//! `DrawBatcher` owns the three mutable-but-non-GPU assets used only during
//! draw recording:
//! - `tessellator`        — Lyon-based path tessellator
//! - `path_cache`         — per-frame tessellation cache (keyed by path hash + scale)
//! - `superellipse_cache` — per-frame iOS-squircle path cache
//!
//! For each record call the caller (`WgpuPainter`) passes in the GPU draw-state
//! and the accumulation targets via **plain borrowed parameters**:
//! - `segment: &mut DrawSegment`      — current accumulation buffer
//! - `draw_order: &mut Vec<DrawItem>` — ordered list of sealed segments
//! - `state: &GpuStateStack`          — read-only transform/scissor queries
//! - `opacity: f32`                   — current opacity (Copy-read before call)
//!
//! This is the borrow seam described in the T9a chief-architect verdict.  Four
//! disjoint `WgpuPainter` fields are borrowed simultaneously; reading `opacity`
//! into a `f32` local before the call prevents `compositor` from being borrowed
//! across the batcher invocation.
//!
//! # Shader dispatch
//!
//! `dispatch_shader_rect` (gradient/shader fills for rect/rrect/circle) lives on
//! `DrawBatcher` (T9c).  The gradient methods (`gradient_rect`,
//! `radial_gradient_rect`, `sweep_gradient_rect`) and `shadow_rect` also live here,
//! taking `(&mut DrawSegment, &GpuStateStack, …)` via the same borrow seam.
//! Each painter shim (`rect`/`rrect`/`circle`) folds the shader pre-check into the
//! batcher call; the shim becomes a thin opacity-read + delegation.
//!
//! `draw_path` and `draw_vertices` also live here (T9d), using the same seam.
//! `draw_path` owns the tessellation cache hit/miss logic; `draw_vertices` owns
//! the per-vertex color/uv assembly and u16→u32 index conversion.
//!
//! # Invariants preserved
//!
//! - **Non-`SrcOver` segment seal** fires in `add_tessellated_with_key` at the
//!   identical point as the original painter code (immediately after appending a
//!   non-`SrcOver` batch entry), now threaded via `&mut draw_order`.
//! - **Scissor coalescing** reads `state.current_scissor()` as a `Copy` value at
//!   the same instant as the original code.
//! - **Opacity baked at record time**: the `opacity` value is read in the
//!   `WgpuPainter` shim before the batcher call, preserving the original
//!   read point relative to the compositor stack.
//! - **No new per-draw heap allocations** vs. the pre-extraction baseline.

use flui_painting::BlendMode;
use flui_types::{Rect, geometry::Pixels};

use super::{
    command_ir::{AdvancedShapeOp, DrawItem, DrawSegment, SsaaPathOp, TessellatedBatch},
    path_cache::PathCache,
    pipeline::PipelineKey,
    state_stack::GpuStateStack,
    superellipse_cache::SuperellipsePathCache,
    tessellator::Tessellator,
    vertex::Vertex,
};

mod gradients;
mod images;
mod paths;
mod shapes;

/// Owns the tessellator and per-frame geometry caches used during draw recording.
///
/// Separated from `WgpuPainter` so the record-side mutable state (`tessellator`,
/// `path_cache`, `superellipse_cache`) can be borrowed independently from the
/// flush-side state (`texture_batch`) and the draw accumulation targets
/// (`current_segment`, `draw_order`).  See the module-level doc for the borrow
/// seam contract.
pub(super) struct DrawBatcher {
    /// Lyon-based path tessellator for complex shapes.
    pub(super) tessellator: Tessellator,

    /// Per-frame tessellation cache: avoids re-tessellating identical paths within
    /// a frame.
    pub(super) path_cache: PathCache,

    /// Per-frame iOS-squircle path cache.
    ///
    /// Mirrors `PathCache` ownership and eviction semantics (`max_entries` +
    /// frame-based eviction).  Consulted by `WgpuPainter::superellipse_path` (the
    /// `Backend::superellipse_path` override).
    pub(super) superellipse_cache: SuperellipsePathCache,
}

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl DrawBatcher {
    /// Construct a `DrawBatcher` with the same cache capacities used by the
    /// original `WgpuPainter::with_shared_device`.
    pub(super) fn new() -> Self {
        Self {
            tessellator: Tessellator::new(),
            path_cache: PathCache::new(512),
            superellipse_cache: SuperellipsePathCache::new(256),
        }
    }

    // ===== Segment accumulation primitives =====

    /// Seal `segment` and push it onto `draw_order`, then start a fresh empty
    /// segment.  An empty segment is never pushed (avoids empty GPU passes).
    ///
    /// This is the **single place** that performs `current_segment → draw_order`
    /// promotion.  Every seal — whether triggered by an explicit Z-interleave
    /// (`WgpuPainter::queue_offscreen_result`), by the non-`SrcOver` draw-order
    /// contract in [`DrawBatcher::add_tessellated_with_key`], or by the final
    /// flush before GPU submission — routes through here.
    pub(super) fn finish_current_segment(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
    ) {
        let completed = std::mem::replace(segment, DrawSegment::new());
        if !completed.is_empty() {
            draw_order.push(DrawItem::Segment(completed));
        }
    }

    /// Append tessellated vertices/indices to `segment` under the given pipeline
    /// key, starting a new `TessellatedBatch` on a key or scissor change.
    ///
    /// # Draw-order contract for non-`SrcOver` blend modes
    ///
    /// After appending a non-`SrcOver` entry the segment is immediately sealed.
    /// This guarantees the blend-mode shape flushes **after** any instanced draws
    /// recorded into the same segment, which is required for destructive modes
    /// (Clear, DstOut, Src, SrcIn, DstIn, SrcOut, SrcATop, DstATop, Xor).
    /// `SrcOver` shapes do not trigger a split; the common path has zero overhead.
    ///
    /// # Advanced (dst-read) blend diversion — DECISION 2
    ///
    /// Advanced blend modes (W3C compositing modes: Multiply, Screen, Overlay, …)
    /// cannot be expressed as fixed-function blends and require a backdrop copy at
    /// replay time.  When `key.blend_mode().is_advanced()` is true:
    ///
    /// 1. The current `segment` (prior content) is sealed first so that earlier
    ///    draws flush to the surface before the backdrop is sampled — preserving
    ///    Z-order correctness.
    /// 2. The new shape's geometry is isolated into a fresh `DrawSegment` inside
    ///    an `AdvancedShapeOp` and pushed as `DrawItem::AdvancedShape`.
    /// 3. `pipeline_key_from_paint` now returns a `with_blend(mode)` key for
    ///    advanced modes (carrying the original mode), so `key.blend_mode().is_advanced()`
    ///    fires here and the key never reaches `PipelineCache::get_or_create`.
    ///
    /// Plus and Modulate are Porter-Duff (`is_advanced()` = false) and take the
    /// existing fixed-function path unchanged.
    pub(super) fn add_tessellated_with_key(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: Vec<Vertex>,
        indices: &[u32],
        key: PipelineKey,
    ) {
        if indices.is_empty() {
            return;
        }

        // ── Advanced (dst-read) diversion — ABOVE the Porter-Duff seal ───────
        //
        // Check before appending to `segment` so we can (a) seal prior content
        // cleanly and (b) build an isolated one-shape DrawSegment without having
        // to undo an append.
        if key.blend_mode().is_advanced() {
            // Step 1: seal whatever content preceded this shape so it lands on
            // the surface before the backdrop is copied.  Z-order guarantee:
            // flush_advanced_layer is called AFTER all prior draw_order items
            // are flushed in the submit loop.
            Self::finish_current_segment(segment, draw_order);

            // Step 2: compute device-space AABB from the already-baked vertices.
            // Vertices are in device-pixel coordinates (the CTM was applied by the
            // caller via apply_transform / submit_transformed_geometry).
            let device_bounds = vertices_aabb(&vertices);

            // Step 3: build an isolated DrawSegment containing only this shape.
            let mut shape_segment = DrawSegment::new();
            // Indices reference vertices[0..], so base_index = 0.
            shape_segment.vertices.extend_from_slice(&vertices);
            shape_segment.indices.extend(indices.iter().copied()); // already 0-based
            shape_segment.current_pipeline_key = Some(key);
            shape_segment.tess_batches.push(TessellatedBatch {
                // Use SrcOver alpha-blend pipeline for rendering the shape into
                // the offscreen foreground texture.  The advanced blend formula
                // is computed in the WGSL shader (backdrop copy path); the
                // fixed-function blend stage here just composites the shape over
                // the transparent offscreen background.
                pipeline_key: PipelineKey::alpha_blend(),
                scissor: state.current_scissor(),
                index_start: 0,
                index_count: indices.len() as u32,
            });

            draw_order.push(DrawItem::AdvancedShape(AdvancedShapeOp {
                segment: shape_segment,
                mode: key.blend_mode(),
                device_bounds,
            }));

            return;
        }

        // ── Normal path (SrcOver + Porter-Duff) ──────────────────────────────

        let base_index = segment.vertices.len() as u32;
        let index_start = segment.indices.len() as u32;

        segment.vertices.extend(vertices);
        segment
            .indices
            .extend(indices.iter().map(|&i| i + base_index));

        let index_count = indices.len() as u32;

        if let Some(last) = segment.tess_batches.last_mut()
            && last.pipeline_key == key
            && last.scissor == state.current_scissor()
        {
            last.index_count += index_count;
        } else {
            segment.current_pipeline_key = Some(key);
            segment.tess_batches.push(TessellatedBatch {
                pipeline_key: key,
                scissor: state.current_scissor(),
                index_start,
                index_count,
            });
        }

        // Draw-order contract: close the segment after any non-SrcOver blend.
        if key.blend_mode() != BlendMode::SrcOver {
            Self::finish_current_segment(segment, draw_order);
        }
    }

    /// Divert a geometry fill into a `DrawItem::SsaaPath` for SSAA-based
    /// anti-aliasing.
    ///
    /// Called from:
    /// - `DrawBatcher::draw_path` (in `batches/paths.rs`) for arbitrary path fills
    ///   whose blend mode is tile-safe or advanced.
    /// - `batches/shapes.rs` non-SrcOver branches for rect/rrect/circle/oval/arc
    ///   fills whose blend mode is tile-safe or advanced.
    ///
    /// Closed-form SrcOver shapes (rect/rrect/circle/oval/arc) route to the
    /// instanced affine-SDF path and must **not** call this method.
    ///
    /// Coverage-destructive Porter-Duff modes (Clear, Src, SrcIn, DstIn, SrcOut,
    /// DstATop, Modulate) must NOT call this method — they keep the tessellated
    /// path so transparent tile padding does not destructively write to the
    /// destination outside the geometry boundary.
    ///
    /// ## What this does
    ///
    /// 1. Seals the current `segment` so prior content flushes before the SSAA
    ///    tile (Z-order correctness, same as the advanced-shape diversion).
    /// 2. Builds an isolated `DrawSegment` containing only this path's geometry.
    /// 3. Computes the device-space AABB of the already-transformed vertices.
    /// 4. Pushes `DrawItem::SsaaPath(SsaaPathOp { segment, device_bounds, blend })`.
    ///
    /// All GPU work (2× render + box downsample + composite) happens at replay
    /// time in `GpuReplay::render_ssaa_path`.
    pub(super) fn divert_path_to_ssaa(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: &[Vertex],
        indices: &[u32],
        blend: BlendMode,
    ) {
        if indices.is_empty() {
            return;
        }

        // Step 1: seal prior content so it appears below the SSAA tile.
        Self::finish_current_segment(segment, draw_order);

        // Step 2: compute the device-space AABB from the baked (transformed) vertices.
        let device_bounds = vertices_aabb(vertices);

        // Step 3: build an isolated DrawSegment for this path only.
        // The internal pipeline is always SrcOver (alpha-blend): the geometry is
        // rendered into a transparent offscreen tile.  The SSAA blend mode is stored
        // in `SsaaPathOp::blend` and applied at composite time, not at raster time.
        let mut path_segment = DrawSegment::new();
        path_segment.vertices.extend_from_slice(vertices);
        path_segment.indices.extend(indices.iter().copied());
        path_segment.current_pipeline_key = Some(PipelineKey::alpha_blend());
        path_segment.tess_batches.push(TessellatedBatch {
            pipeline_key: PipelineKey::alpha_blend(),
            scissor: state.current_scissor(),
            index_start: 0,
            index_count: indices.len() as u32,
        });

        draw_order.push(DrawItem::SsaaPath(SsaaPathOp {
            segment: path_segment,
            device_bounds,
            blend,
        }));
    }

    /// Apply the current world transform to every vertex position in `vertices`
    /// (already tessellated in local space) and submit to the tessellated batch.
    ///
    /// `shape.wgsl` only converts px→clip via the viewport uniform; it has no
    /// model-matrix uniform, so the CPU must bake the transform at record time.
    pub(super) fn submit_transformed_geometry(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        mut vertices: Vec<Vertex>,
        indices: &[u32],
        key: PipelineKey,
    ) {
        let transform = state.current_transform();
        for v in &mut vertices {
            let transformed = transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
            v.position = [transformed.x, transformed.y];
        }
        Self::add_tessellated_with_key(segment, draw_order, state, vertices, indices, key);
    }

    /// Prime the tessellator's flatten tolerance from the current CTM max-basis
    /// length.  Must be called immediately before any `tessellate_*` invocation.
    pub(super) fn prime_tessellator_scale(&mut self, state: &GpuStateStack) {
        self.tessellator.set_max_scale(state.max_scale());
    }

    /// Convert a `Shader` into GPU `GradientStop`s (max 8 stops).
    ///
    /// Called by `DrawBatcher::dispatch_shader_rect` which lives in the same module.
    pub(super) fn shader_to_gradient_stops(
        shader: &flui_types::painting::Shader,
    ) -> Vec<super::effects::GradientStop> {
        let (colors, stops) = match shader {
            flui_types::painting::Shader::LinearGradient { colors, stops, .. }
            | flui_types::painting::Shader::RadialGradient { colors, stops, .. }
            | flui_types::painting::Shader::SweepGradient { colors, stops, .. } => {
                (colors.as_slice(), stops.as_deref())
            }
            flui_types::painting::Shader::Solid { color } => {
                return vec![
                    super::effects::GradientStop::new(*color, 0.0),
                    super::effects::GradientStop::new(*color, 1.0),
                ];
            }
            _ => return vec![],
        };

        let count = colors.len().min(8);
        (0..count)
            .map(|i| {
                let position = if let Some(s) = stops {
                    s.get(i)
                        .copied()
                        .unwrap_or(i as f32 / (count - 1).max(1) as f32)
                } else {
                    i as f32 / (count - 1).max(1) as f32
                };
                super::effects::GradientStop::new(colors[i], position)
            })
            .collect()
    }
}

// ─── Module-level helpers ─────────────────────────────────────────────────────

/// Compute the axis-aligned bounding box of a slice of device-space [`Vertex`]
/// positions.
///
/// Returns an empty rect at the origin if `vertices` is empty (cannot happen in
/// practice: `add_tessellated_with_key` returns early on empty indices before
/// this is called).
///
/// The AABB is used by `flush_advanced_layer` to determine `device_bounds` for
/// the backdrop-copy region and the `src_uv` remap.
fn vertices_aabb(vertices: &[Vertex]) -> Rect<Pixels> {
    if vertices.is_empty() {
        return Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(0.0), Pixels(0.0));
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for v in vertices {
        let x = v.position[0];
        let y = v.position[1];
        if x < min_x {
            min_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if x > max_x {
            max_x = x;
        }
        if y > max_y {
            max_y = y;
        }
    }

    Rect::from_ltrb(Pixels(min_x), Pixels(min_y), Pixels(max_x), Pixels(max_y))
}

// ─── Unit tests ───────────────────────────────────────────────────────────────
//
// Placed here because they require direct access to `DrawBatcher`,
// `GpuStateStack`, `Vertex`, and `vertices_aabb` which are `pub(super)` items
// visible only within the `wgpu` module and its direct children — this file is
// a direct child of `wgpu`, so all of those items are in scope.

#[cfg(test)]
mod unit_tests {
    use flui_painting::BlendMode;
    use flui_types::Rect;

    use super::super::{
        command_ir::{DrawItem, DrawSegment},
        state_stack::GpuStateStack,
    };
    use super::{DrawBatcher, PipelineKey, Vertex, vertices_aabb};

    /// All 15 W3C advanced blend modes — used by gradient diversion tests G1-G3.
    const ALL_ADVANCED_MODES: [BlendMode; 15] = [
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    // ── Helper: build the minimal geometry for a quad ─────────────────────────

    /// Build the four vertices for an axis-aligned rectangle in device pixels.
    ///
    /// The vertices are in the same order that `DrawBatcher::rect` would produce
    /// for a non-SrcOver fill: TL, TR, BR, BL in CCW winding with `tl.x/y` etc.
    fn rect_vertices(left: f32, top: f32, right: f32, bottom: f32) -> Vec<Vertex> {
        let rgba = [1.0_f32, 0.5, 0.2, 1.0]; // arbitrary colour
        vec![
            Vertex {
                position: [left, top],
                color: rgba,
                tex_coord: [0.0, 0.0],
            },
            Vertex {
                position: [right, top],
                color: rgba,
                tex_coord: [1.0, 0.0],
            },
            Vertex {
                position: [right, bottom],
                color: rgba,
                tex_coord: [1.0, 1.0],
            },
            Vertex {
                position: [left, bottom],
                color: rgba,
                tex_coord: [0.0, 1.0],
            },
        ]
    }

    fn rect_indices() -> Vec<u32> {
        vec![0, 1, 2, 0, 2, 3]
    }

    // ── S1: advanced rect → AdvancedShape ─────────────────────────────────────

    /// S1: `add_tessellated_with_key` must divert into `DrawItem::AdvancedShape`
    /// when the pipeline key carries an advanced blend mode (`is_advanced()` = true).
    ///
    /// **Proves:** the detection branch `if key.blend_mode().is_advanced()` fires
    /// and pushes `DrawItem::AdvancedShape` to `draw_order`.
    #[test]
    fn multiply_key_diverts_to_advanced_shape_draw_item() {
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();
        let key = PipelineKey::with_blend(BlendMode::Multiply);

        let vertices = rect_vertices(10.0, 10.0, 50.0, 50.0);
        let indices = rect_indices();

        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            vertices,
            &indices,
            key,
        );

        assert_eq!(
            draw_order.len(),
            1,
            "one AdvancedShape item must be pushed for a Multiply key"
        );
        assert!(
            matches!(draw_order[0], DrawItem::AdvancedShape(_)),
            "draw_order[0] must be AdvancedShape for Multiply key; \
             got a Segment or other variant instead"
        );
    }

    // ── S2: SrcOver key → stays in Segment ────────────────────────────────────

    /// S2: `add_tessellated_with_key` must NOT produce `DrawItem::AdvancedShape`
    /// for a `SrcOver` (alpha-blend) key — it must stay in `segment.tess_batches`.
    ///
    /// **Proves:** the advanced diversion only fires for `is_advanced()` modes;
    /// the SrcOver path is unchanged.
    #[test]
    fn srcover_key_stays_in_segment_not_advanced() {
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();
        let key = PipelineKey::alpha_blend(); // SrcOver

        let vertices = rect_vertices(10.0, 10.0, 50.0, 50.0);
        let indices = rect_indices();

        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            vertices,
            &indices,
            key,
        );

        assert!(
            draw_order.is_empty(),
            "SrcOver key must not push to draw_order (stays in segment)"
        );
        assert_eq!(
            segment.tess_batches.len(),
            1,
            "SrcOver key must add one TessellatedBatch to segment"
        );
    }

    // ── S3: Plus/Modulate → Segment (Porter-Duff, not advanced) ───────────────

    /// S3: Plus and Modulate are Porter-Duff; `is_advanced()` is false for them.
    /// `add_tessellated_with_key` must NOT produce `DrawItem::AdvancedShape`.
    ///
    /// **Proves:** Plus/Modulate bypass the advanced diversion correctly.
    #[test]
    fn plus_and_modulate_keys_are_not_advanced_shape() {
        for mode in [BlendMode::Plus, BlendMode::Modulate] {
            let mut segment = DrawSegment::new();
            let mut draw_order: Vec<DrawItem> = Vec::new();
            let state = GpuStateStack::new_for_test();
            let key = PipelineKey::with_blend(mode);

            assert!(
                !key.blend_mode().is_advanced(),
                "{mode:?}: is_advanced() must be false (Porter-Duff mode)"
            );

            let vertices = rect_vertices(10.0, 10.0, 50.0, 50.0);
            let indices = rect_indices();

            DrawBatcher::add_tessellated_with_key(
                &mut segment,
                &mut draw_order,
                &state,
                vertices,
                &indices,
                key,
            );

            assert!(
                !draw_order
                    .iter()
                    .any(|item| matches!(item, DrawItem::AdvancedShape(_))),
                "{mode:?}: must not produce DrawItem::AdvancedShape; \
                 Plus/Modulate are Porter-Duff (is_advanced() = false)"
            );
        }
    }

    // ── S4: device_bounds AABB ────────────────────────────────────────────────

    /// S4: The `device_bounds` on `AdvancedShapeOp` must be the AABB of the
    /// input vertices (baked device-space positions).
    ///
    /// **Proves:** `vertices_aabb` correctly computes the bounding box and the
    /// AABB is wired into `AdvancedShapeOp::device_bounds` at diversion time.
    #[test]
    fn advanced_shape_device_bounds_is_vertex_aabb() {
        let left = 20.0_f32;
        let top = 30.0_f32;
        let right = 80.0_f32;
        let bottom = 90.0_f32;

        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();
        let key = PipelineKey::with_blend(BlendMode::Screen);

        let vertices = rect_vertices(left, top, right, bottom);
        let indices = rect_indices();

        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            vertices,
            &indices,
            key,
        );

        let DrawItem::AdvancedShape(ref op) = draw_order[0] else {
            panic!("expected AdvancedShape");
        };

        assert!(
            (op.device_bounds.left().0 - left).abs() < 0.01,
            "device_bounds.left must match vertex left {left}; got {}",
            op.device_bounds.left().0
        );
        assert!(
            (op.device_bounds.top().0 - top).abs() < 0.01,
            "device_bounds.top must match vertex top {top}; got {}",
            op.device_bounds.top().0
        );
        assert!(
            (op.device_bounds.right().0 - right).abs() < 0.01,
            "device_bounds.right must match vertex right {right}; got {}",
            op.device_bounds.right().0
        );
        assert!(
            (op.device_bounds.bottom().0 - bottom).abs() < 0.01,
            "device_bounds.bottom must match vertex bottom {bottom}; got {}",
            op.device_bounds.bottom().0
        );
    }

    // ── S4b: shape segment batch uses alpha_blend (SrcOver) ───────────────────

    /// S4b: The `TessellatedBatch` inside the `AdvancedShapeOp` segment must use
    /// `PipelineKey::alpha_blend()` (SrcOver), not the original advanced key.
    ///
    /// **Proves:** the offscreen foreground renders via the SrcOver pipeline so
    /// the shape color/alpha composites correctly onto the transparent offscreen.
    #[test]
    fn advanced_shape_segment_batch_uses_alpha_blend_pipeline() {
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();
        let key = PipelineKey::with_blend(BlendMode::Overlay);

        let vertices = rect_vertices(10.0, 10.0, 50.0, 50.0);
        let indices = rect_indices();

        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            vertices,
            &indices,
            key,
        );

        let DrawItem::AdvancedShape(ref op) = draw_order[0] else {
            panic!("expected AdvancedShape");
        };

        assert_eq!(
            op.segment.tess_batches.len(),
            1,
            "shape segment must have exactly one TessellatedBatch"
        );
        let batch = &op.segment.tess_batches[0];
        assert!(
            batch.pipeline_key.is_alpha_blended(),
            "shape segment batch must use alpha-blend (SrcOver) pipeline; got non-blended key"
        );
        assert_eq!(
            batch.pipeline_key.blend_mode(),
            BlendMode::SrcOver,
            "shape segment batch blend mode must be SrcOver; got {:?}",
            batch.pipeline_key.blend_mode()
        );
    }

    // ── S4c: AdvancedShapeOp::mode carries original mode ─────────────────────

    /// S4c: `AdvancedShapeOp::mode` must carry the original advanced blend mode.
    #[test]
    fn advanced_shape_op_mode_carries_original_mode() {
        for mode in [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Luminosity,
        ] {
            let mut segment = DrawSegment::new();
            let mut draw_order: Vec<DrawItem> = Vec::new();
            let state = GpuStateStack::new_for_test();
            let key = PipelineKey::with_blend(mode);

            DrawBatcher::add_tessellated_with_key(
                &mut segment,
                &mut draw_order,
                &state,
                rect_vertices(0.0, 0.0, 64.0, 64.0),
                &rect_indices(),
                key,
            );

            let DrawItem::AdvancedShape(ref op) = draw_order[0] else {
                panic!("{mode:?}: expected AdvancedShape");
            };
            assert_eq!(
                op.mode, mode,
                "AdvancedShapeOp::mode must carry {mode:?}; got {:?}",
                op.mode
            );
        }
    }

    // ── S4d: all 15 advanced modes produce AdvancedShape ─────────────────────

    /// S4d: All 15 W3C advanced blend modes must divert into `DrawItem::AdvancedShape`.
    #[test]
    fn all_15_advanced_modes_produce_advanced_shape() {
        let advanced_modes = [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];
        for mode in advanced_modes {
            let mut segment = DrawSegment::new();
            let mut draw_order: Vec<DrawItem> = Vec::new();
            let state = GpuStateStack::new_for_test();
            let key = PipelineKey::with_blend(mode);

            DrawBatcher::add_tessellated_with_key(
                &mut segment,
                &mut draw_order,
                &state,
                rect_vertices(0.0, 0.0, 64.0, 64.0),
                &rect_indices(),
                key,
            );

            assert!(
                draw_order
                    .iter()
                    .any(|item| matches!(item, DrawItem::AdvancedShape(_))),
                "{mode:?}: expected DrawItem::AdvancedShape, got none in draw_order"
            );
        }
    }

    // ── S4e: vertices_aabb is correct ────────────────────────────────────────

    /// S4e: `vertices_aabb` correctly computes the bounding box of a set of vertices.
    #[test]
    fn vertices_aabb_is_correct_for_quad() {
        let vertices = rect_vertices(15.0, 25.0, 70.0, 85.0);
        let aabb = vertices_aabb(&vertices);
        assert!(
            (aabb.left().0 - 15.0).abs() < 0.01,
            "aabb.left: expected 15.0, got {}",
            aabb.left().0
        );
        assert!(
            (aabb.top().0 - 25.0).abs() < 0.01,
            "aabb.top: expected 25.0, got {}",
            aabb.top().0
        );
        assert!(
            (aabb.right().0 - 70.0).abs() < 0.01,
            "aabb.right: expected 70.0, got {}",
            aabb.right().0
        );
        assert!(
            (aabb.bottom().0 - 85.0).abs() < 0.01,
            "aabb.bottom: expected 85.0, got {}",
            aabb.bottom().0
        );
    }

    // ── S4f: empty vertices_aabb returns origin ───────────────────────────────

    /// S4f: `vertices_aabb` with an empty slice must return the zero-origin empty rect.
    #[test]
    fn vertices_aabb_empty_returns_zero_rect() {
        let aabb = vertices_aabb(&[]);
        assert!(
            aabb.left().0.abs() < f32::EPSILON,
            "empty vertices_aabb: left must be 0.0, got {}",
            aabb.left().0
        );
        assert!(
            aabb.top().0.abs() < f32::EPSILON,
            "empty vertices_aabb: top must be 0.0, got {}",
            aabb.top().0
        );
    }

    // ── G1: linear gradient advanced mode → AdvancedShape ───────────────────

    /// G1: `dispatch_shader_rect` with a linear gradient and an advanced blend mode
    /// must push `DrawItem::AdvancedShape` to `draw_order` and NOT accumulate
    /// gradient instances in the main segment.
    ///
    /// **Proves:** the advanced diversion branch in `dispatch_shader_rect` fires
    /// and produces an isolated `DrawItem::AdvancedShape`.  The gradient instance
    /// goes into `AdvancedShapeOp::segment`, not the main segment's batch.
    #[test]
    fn linear_gradient_advanced_mode_diverts_to_advanced_shape() {
        use flui_painting::Paint;
        use flui_types::{
            geometry::{Offset, px},
            painting::{Shader, TileMode},
        };

        for mode in ALL_ADVANCED_MODES {
            let mut segment = DrawSegment::new();
            let mut draw_order: Vec<DrawItem> = Vec::new();
            let state = GpuStateStack::new_for_test();

            let bounds = Rect::from_xywh(px(10.0), px(10.0), px(50.0), px(50.0));
            let paint = Paint {
                blend_mode: mode,
                shader: Some(Shader::LinearGradient {
                    from: Offset::new(px(10.0), px(10.0)),
                    to: Offset::new(px(60.0), px(10.0)),
                    colors: vec![
                        flui_types::Color::rgba(255, 0, 0, 255),
                        flui_types::Color::rgba(0, 0, 255, 255),
                    ],
                    stops: None,
                    tile_mode: TileMode::Clamp,
                }),
                ..Default::default()
            };

            let handled = DrawBatcher::dispatch_shader_rect(
                &mut segment,
                &mut draw_order,
                &state,
                bounds,
                &paint,
                [0.0; 4],
            );

            assert!(
                handled,
                "{mode:?}: dispatch_shader_rect must return true for linear gradient"
            );
            assert_eq!(
                draw_order.len(),
                1,
                "{mode:?}: exactly one AdvancedShape must be pushed; got {}",
                draw_order.len()
            );
            assert!(
                matches!(draw_order[0], DrawItem::AdvancedShape(_)),
                "{mode:?}: draw_order[0] must be AdvancedShape; got Segment or other"
            );
            // Main segment must hold no gradient instances — they went into the
            // isolated AdvancedShapeOp segment.
            assert_eq!(
                segment.linear_gradient_batch.len(),
                0,
                "{mode:?}: main segment must have 0 linear gradient instances; got {}",
                segment.linear_gradient_batch.len()
            );
        }
    }

    // ── G2: radial gradient advanced mode → AdvancedShape ────────────────────

    /// G2: `dispatch_shader_rect` with a radial gradient and an advanced blend
    /// mode must push `DrawItem::AdvancedShape`.  Mirror of G1 for the radial path.
    #[test]
    fn radial_gradient_advanced_mode_diverts_to_advanced_shape() {
        use flui_painting::Paint;
        use flui_types::{
            geometry::{Offset, px},
            painting::{Shader, TileMode},
        };

        for mode in ALL_ADVANCED_MODES {
            let mut segment = DrawSegment::new();
            let mut draw_order: Vec<DrawItem> = Vec::new();
            let state = GpuStateStack::new_for_test();

            let bounds = Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0));
            let paint = Paint {
                blend_mode: mode,
                shader: Some(Shader::RadialGradient {
                    center: Offset::new(px(32.0), px(32.0)),
                    radius: 32.0,
                    colors: vec![
                        flui_types::Color::rgba(255, 255, 0, 255),
                        flui_types::Color::rgba(0, 255, 0, 255),
                    ],
                    stops: None,
                    tile_mode: TileMode::Clamp,
                    focal: None,
                    focal_radius: None,
                }),
                ..Default::default()
            };

            let handled = DrawBatcher::dispatch_shader_rect(
                &mut segment,
                &mut draw_order,
                &state,
                bounds,
                &paint,
                [0.0; 4],
            );

            assert!(
                handled,
                "{mode:?}: dispatch_shader_rect must return true for radial gradient"
            );
            assert!(
                draw_order
                    .iter()
                    .any(|item| matches!(item, DrawItem::AdvancedShape(_))),
                "{mode:?}: draw_order must contain AdvancedShape for radial gradient"
            );
            assert_eq!(
                segment.radial_gradient_batch.len(),
                0,
                "{mode:?}: main segment must have 0 radial gradient instances"
            );
        }
    }

    // ── G3: sweep gradient advanced mode → AdvancedShape ─────────────────────

    /// G3: `dispatch_shader_rect` with a sweep gradient and an advanced blend
    /// mode must push `DrawItem::AdvancedShape`.  Mirror of G1/G2 for sweep.
    #[test]
    fn sweep_gradient_advanced_mode_diverts_to_advanced_shape() {
        use flui_painting::Paint;
        use flui_types::{
            geometry::{Offset, px},
            painting::{Shader, TileMode},
        };

        for mode in ALL_ADVANCED_MODES {
            let mut segment = DrawSegment::new();
            let mut draw_order: Vec<DrawItem> = Vec::new();
            let state = GpuStateStack::new_for_test();

            let bounds = Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0));
            let paint = Paint {
                blend_mode: mode,
                shader: Some(Shader::SweepGradient {
                    center: Offset::new(px(32.0), px(32.0)),
                    start_angle: 0.0,
                    end_angle: std::f32::consts::TAU,
                    colors: vec![
                        flui_types::Color::rgba(200, 100, 50, 255),
                        flui_types::Color::rgba(50, 100, 200, 255),
                    ],
                    stops: None,
                    tile_mode: TileMode::Clamp,
                }),
                ..Default::default()
            };

            let handled = DrawBatcher::dispatch_shader_rect(
                &mut segment,
                &mut draw_order,
                &state,
                bounds,
                &paint,
                [0.0; 4],
            );

            assert!(
                handled,
                "{mode:?}: dispatch_shader_rect must return true for sweep gradient"
            );
            assert!(
                draw_order
                    .iter()
                    .any(|item| matches!(item, DrawItem::AdvancedShape(_))),
                "{mode:?}: draw_order must contain AdvancedShape for sweep gradient"
            );
            assert_eq!(
                segment.sweep_gradient_batch.len(),
                0,
                "{mode:?}: main segment must have 0 sweep gradient instances"
            );
        }
    }

    // ── G4: SrcOver gradient stays in main segment ────────────────────────────

    /// G4: `dispatch_shader_rect` with a linear gradient and `SrcOver` must NOT
    /// produce `DrawItem::AdvancedShape`.  The SrcOver path is byte-identical to
    /// pre-PR-5.
    ///
    /// **Proves:** the advanced diversion gate (`is_advanced()`) correctly rejects
    /// SrcOver, leaving the gradient in the main segment's gradient batch.
    #[test]
    fn srcover_gradient_stays_in_main_segment() {
        use flui_painting::Paint;
        use flui_types::{
            geometry::{Offset, px},
            painting::{Shader, TileMode},
        };

        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0));
        let paint = Paint {
            blend_mode: BlendMode::SrcOver,
            shader: Some(Shader::LinearGradient {
                from: Offset::new(px(0.0), px(0.0)),
                to: Offset::new(px(64.0), px(0.0)),
                colors: vec![
                    flui_types::Color::rgba(255, 0, 0, 255),
                    flui_types::Color::rgba(0, 0, 255, 255),
                ],
                stops: None,
                tile_mode: TileMode::Clamp,
            }),
            ..Default::default()
        };

        let handled = DrawBatcher::dispatch_shader_rect(
            &mut segment,
            &mut draw_order,
            &state,
            bounds,
            &paint,
            [0.0; 4],
        );

        assert!(
            handled,
            "SrcOver linear gradient: dispatch_shader_rect must return true"
        );
        // SrcOver stays in main segment — no AdvancedShape pushed.
        assert!(
            draw_order.is_empty(),
            "SrcOver gradient must not push to draw_order; got {} items",
            draw_order.len()
        );
        assert_eq!(
            segment.linear_gradient_batch.len(),
            1,
            "SrcOver gradient must accumulate one instance in main segment; got {}",
            segment.linear_gradient_batch.len()
        );
    }

    // ── G5: isolated segment stop_offset is 0 ────────────────────────────────

    /// G5: The gradient instance inside an `AdvancedShapeOp` must use
    /// `stop_offset = 0`, relative to the isolated segment's own stop buffer.
    ///
    /// **Proves:** the isolated segment starts from zero prior stops, not from the
    /// main segment's cumulative stop count.  A non-zero stop_offset would index
    /// out-of-range stops in the isolated buffer and produce corrupt GPU output.
    #[test]
    fn advanced_gradient_isolated_segment_stop_offset_is_zero() {
        use flui_painting::Paint;
        use flui_types::{
            geometry::{Offset, px},
            painting::{Shader, TileMode},
        };

        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        // Pre-populate the main segment with stops so that a bug using the main
        // segment's stop count would produce stop_offset = 2 (wrong).
        let placeholder_stop = super::super::effects::GradientStop::new(
            flui_types::Color::rgba(128, 128, 128, 255),
            0.5,
        );
        segment.current_gradient_stops.push(placeholder_stop);
        segment.current_gradient_stops.push(placeholder_stop);
        // main segment now has 2 stops.

        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0));
        let paint = Paint {
            blend_mode: BlendMode::Multiply,
            shader: Some(Shader::LinearGradient {
                from: Offset::new(px(0.0), px(0.0)),
                to: Offset::new(px(64.0), px(0.0)),
                colors: vec![
                    flui_types::Color::rgba(255, 0, 0, 255),
                    flui_types::Color::rgba(0, 0, 255, 255),
                ],
                stops: None,
                tile_mode: TileMode::Clamp,
            }),
            ..Default::default()
        };

        DrawBatcher::dispatch_shader_rect(
            &mut segment,
            &mut draw_order,
            &state,
            bounds,
            &paint,
            [0.0; 4],
        );

        let DrawItem::AdvancedShape(ref isolated_op) = draw_order[draw_order.len() - 1] else {
            panic!("last draw_order item must be AdvancedShape (G5)");
        };

        // The isolated segment must hold exactly the gradient's 2 stops — NOT the
        // main segment's 2 placeholder stops plus 2 gradient stops (4 total).
        // stop_offset = 0 is the structural guarantee: the isolated segment always
        // starts empty, so the gradient's stops are at index 0.
        assert_eq!(
            isolated_op.segment.current_gradient_stops.len(),
            2,
            "isolated segment must have exactly 2 gradient stops (from the gradient itself); \
             got {} — stop_offset = 0 invariant may be broken",
            isolated_op.segment.current_gradient_stops.len()
        );
        assert_eq!(
            isolated_op.segment.linear_gradient_batch.len(),
            1,
            "isolated segment must have exactly 1 linear gradient instance"
        );
    }

    // ── G6: Z-seal fires for gradient advanced mode ───────────────────────────

    /// G6: Prior SrcOver content in the main segment must be sealed into
    /// `draw_order` BEFORE the `DrawItem::AdvancedShape` is pushed.
    ///
    /// Mirror of S4g for the gradient path: verifies `finish_current_segment`
    /// fires at the top of the advanced branch in `dispatch_shader_rect`.
    #[test]
    fn prior_content_sealed_before_gradient_advanced_shape() {
        use flui_painting::Paint;
        use flui_types::{
            geometry::{Offset, px},
            painting::{Shader, TileMode},
        };

        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        // Add SrcOver tessellated content to the main segment first.
        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            rect_vertices(0.0, 0.0, 32.0, 32.0),
            &rect_indices(),
            PipelineKey::alpha_blend(),
        );
        assert!(
            draw_order.is_empty(),
            "SrcOver must not push to draw_order yet"
        );

        // Dispatch a gradient with an advanced blend mode — this must seal the
        // prior SrcOver content before pushing the AdvancedShape.
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0));
        let paint = Paint {
            blend_mode: BlendMode::Multiply,
            shader: Some(Shader::LinearGradient {
                from: Offset::new(px(0.0), px(0.0)),
                to: Offset::new(px(64.0), px(0.0)),
                colors: vec![
                    flui_types::Color::rgba(200, 100, 50, 255),
                    flui_types::Color::rgba(50, 100, 200, 255),
                ],
                stops: None,
                tile_mode: TileMode::Clamp,
            }),
            ..Default::default()
        };
        DrawBatcher::dispatch_shader_rect(
            &mut segment,
            &mut draw_order,
            &state,
            bounds,
            &paint,
            [0.0; 4],
        );

        // draw_order must be: [0] sealed Segment (prior SrcOver) + [1] AdvancedShape.
        assert_eq!(
            draw_order.len(),
            2,
            "draw_order must contain a sealed Segment followed by AdvancedShape; got {}",
            draw_order.len()
        );
        assert!(
            matches!(draw_order[0], DrawItem::Segment(_)),
            "draw_order[0] must be the sealed SrcOver Segment (Z-seal before gradient advanced)"
        );
        assert!(
            matches!(draw_order[1], DrawItem::AdvancedShape(_)),
            "draw_order[1] must be the AdvancedShape (gradient advanced)"
        );
    }

    // ── S4g: seal fires before advancing — prior segment preserved ────────────

    /// S4g: When an advanced shape is drawn after prior SrcOver content, the seal
    /// step must preserve the SrcOver content as a `DrawItem::Segment` BEFORE
    /// the `DrawItem::AdvancedShape`.
    ///
    /// Z-order guarantee: prior content must appear before the advanced shape in
    /// `draw_order` so it flushes to the surface before the backdrop is copied.
    #[test]
    fn prior_srcover_content_sealed_before_advanced_shape() {
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        // Add SrcOver content first.
        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            rect_vertices(0.0, 0.0, 32.0, 32.0),
            &rect_indices(),
            PipelineKey::alpha_blend(),
        );
        // SrcOver stays in segment — draw_order still empty.
        assert!(
            draw_order.is_empty(),
            "SrcOver must not push to draw_order yet"
        );

        // Now add an advanced shape — this must seal the prior SrcOver content first.
        DrawBatcher::add_tessellated_with_key(
            &mut segment,
            &mut draw_order,
            &state,
            rect_vertices(0.0, 0.0, 64.0, 64.0),
            &rect_indices(),
            PipelineKey::with_blend(BlendMode::Multiply),
        );

        // draw_order must have exactly two items: sealed Segment + AdvancedShape.
        assert_eq!(
            draw_order.len(),
            2,
            "draw_order must have Segment (sealed prior) + AdvancedShape; got {} items",
            draw_order.len()
        );
        assert!(
            matches!(draw_order[0], DrawItem::Segment(_)),
            "draw_order[0] must be the sealed SrcOver Segment; got {:?}",
            std::mem::discriminant(&draw_order[0])
        );
        assert!(
            matches!(draw_order[1], DrawItem::AdvancedShape(_)),
            "draw_order[1] must be AdvancedShape; got {:?}",
            std::mem::discriminant(&draw_order[1])
        );
    }
}

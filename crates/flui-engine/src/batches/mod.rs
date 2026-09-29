//! Record-side draw accumulation helpers extracted from `WgpuPainter`.
//!
//! `DrawBatcher` owns the three mutable-but-non-GPU assets used only during
//! draw recording:
//! - `tessellator`        — Lyon-based path tessellator
//! - `path_cache`         — per-frame tessellation cache (keyed by path hash + scale)
//!
//! For each record call the caller (`WgpuPainter`) passes in the GPU draw-state
//! and the accumulation targets via **plain borrowed parameters**:
//! - `segment: &mut DrawSegment`      — current accumulation buffer
//! - `draw_order: &mut Vec<DrawItem>` — ordered list of sealed segments
//! - `state: &GpuStateStack`          — read-only transform/scissor queries
//! - `opacity: f32`                   — current opacity (Copy-read before call)
//!
//! This borrow seam permits four
//! disjoint `WgpuPainter` fields are borrowed simultaneously; reading `opacity`
//! into a `f32` local before the call prevents `compositor` from being borrowed
//! across the batcher invocation.
//!
//! # Shader dispatch
//!
//! `dispatch_shader_rect` (gradient/shader fills for rect/rrect/circle) lives on
//! `DrawBatcher`. The gradient methods (`gradient_rect`,
//! `radial_gradient_rect`, `sweep_gradient_rect`) and `shadow_rect` also live here,
//! taking `(&mut DrawSegment, &GpuStateStack, …)` via the same borrow seam.
//! Each painter shim (`rect`/`rrect`/`circle`) folds the shader pre-check into the
//! batcher call; the shim becomes a thin opacity-read + delegation.
//!
//! `draw_path` and `draw_vertices` also live here, using the same seam.
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

use flui_foundation::geometry::Rect;
use flui_painting::BlendMode;

use crate::{
    command_ir::{AdvancedShapeOp, DrawItem, DrawSegment, Phase, SsaaPathOp, TessellatedBatch},
    path_cache::PathCache,
    pipeline_cache::PipelineKey,
    state_stack::GpuStateStack,
    tessellator::Tessellator,
    vertex::Vertex,
};

mod gradients;
mod images;
mod paths;
mod shapes;
mod text;

/// Owns the tessellator and per-frame geometry caches used during draw recording.
///
/// Separated from `WgpuPainter` so the record-side mutable state (`tessellator`,
/// `path_cache`) can be borrowed independently from the
/// flush-side state (`texture_batch`) and the draw accumulation targets
/// (`current_segment`, `draw_order`).  See the module-level doc for the borrow
/// seam contract.
pub(super) struct DrawBatcher {
    /// Lyon-based path tessellator for complex shapes.
    pub(super) tessellator: Tessellator,

    /// Per-frame tessellation cache: avoids re-tessellating identical paths within
    /// a frame.
    pub(super) path_cache: PathCache,
}

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
impl DrawBatcher {
    /// Construct a `DrawBatcher` with the same cache capacities used by the
    /// original `WgpuPainter::with_shared_device`.
    pub(super) fn new() -> Self {
        Self {
            tessellator: Tessellator::new(),
            path_cache: PathCache::new(512),
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
    ///
    /// # Allocation strategy — `mem::take` over `mem::replace(…, DrawSegment::new())`
    ///
    /// The previous implementation called `mem::replace(segment, DrawSegment::new())`
    /// on every seal, which triggered 7 `InstanceBatch::new(1024)` allocation calls
    /// (7 × `Vec::with_capacity(1024 × sizeof(T))`) plus 11 more `Vec::new()` calls.
    /// `mem::take` leaves the slot as `DrawSegment::default()` (zero-capacity Vecs)
    /// so no heap allocation occurs at seal time — constituent `Vec`s grow lazily
    /// on first push in the next batch.
    pub(super) fn finish_current_segment(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
    ) {
        // `mem::take` moves completed data out in O(1); `segment` becomes a
        // zero-capacity `DrawSegment::default()` — no allocation at seal time.
        let completed = std::mem::take(segment);
        if !completed.is_empty() {
            draw_order.push(DrawItem::Segment(completed));
        }
        // If the segment was empty `completed` is dropped immediately (no data,
        // no capacity worth recycling). `segment` already holds the zero-cap
        // default from `take`.
    }

    /// Declare that the next primitive recorded into `segment` belongs to
    /// `phase`, sealing the segment first if the fixed replay order would put
    /// it BEFORE something already recorded.
    ///
    /// Call this immediately before every push into a `DrawSegment` batch —
    /// with one deliberate exception: the gradient recorders do NOT call it.
    /// See `DrawBatcher::gradient_rect` for why, and do not "fix" that by
    /// following this sentence literally.
    /// Together the calls make `flush_segment`'s fixed phase order equal record
    /// order within each segment, which is what gives the painter's algorithm
    /// back across primitive kinds — a circle followed by an overlapping rect
    /// previously drew the rect first and let the circle paint over it.
    ///
    /// A forward transition (rect then circle) never seals, so correctly
    /// ordered content costs nothing: it stays in one segment and one pass.
    /// Only genuinely interleaved content splits, and that is exactly the
    /// content whose output was wrong before.
    pub(super) fn begin_phase(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        phase: Phase,
    ) {
        // Never split gradient-bearing content. Gradients skip this seal
        // entirely (see `DrawBatcher::gradient_rect`), but skipping it is not
        // enough on its own: a backward transition between two OTHER kinds —
        // gradient, circle, rect — would still finalize a segment that carries
        // gradient stops, and every segment's table is uploaded to the same
        // buffer at offset 0, so the earlier gradient would sample the later
        // table. Holding the segment together keeps such content at exactly
        // today's ordering rather than making it newly wrong.
        //
        // This gives up kind-ordering for the rest of a segment once a
        // gradient is in it. That is the conservative half of the trade and it
        // disappears once stop tables are per-segment.
        if segment.would_reorder(phase) && segment.current_gradient_stops.is_empty() {
            Self::finish_current_segment(segment, draw_order);
        }
        segment.last_phase = Some(phase);
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
    /// # Advanced (dst-read) blend diversion
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
            let clip_for_isolated = state.active_clip();
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
                // The isolated shape carries the clip that was active when it
                // was recorded — it renders into an offscreen at full-frame
                // device coordinates, so the clip's mapping still applies
                // unchanged — there is no rebase to compose in.
                clip: clip_for_isolated,
            });

            draw_order.push(DrawItem::AdvancedShape(AdvancedShapeOp {
                segment: shape_segment,
                mode: key.blend_mode(),
                device_bounds,
            }));

            return;
        }

        // ── Normal path (SrcOver + Porter-Duff) ──────────────────────────────

        // Seal BEFORE reading the buffer lengths: `base_index`/`index_start`
        // are offsets into THIS segment's vertex/index buffers, so sealing
        // after reading them would rebase the appended geometry onto a fresh,
        // empty segment while the offsets still describe the sealed one.
        Self::begin_phase(segment, draw_order, Phase::Tess);

        let base_index = segment.vertices.len() as u32;
        let index_start = segment.indices.len() as u32;

        segment.vertices.extend(vertices);
        segment
            .indices
            .extend(indices.iter().map(|&i| i + base_index));

        let index_count = indices.len() as u32;

        let clip = state.active_clip();
        // Merging also requires the SAME clip: two different rounded clips can
        // share one bounding scissor (same rect, different radii), and the
        // batch's clip is what its draw binds.
        let mergeable = segment.tess_batches.last().is_some_and(|last| {
            last.pipeline_key == key && last.scissor == state.current_scissor() && last.clip == clip
        });
        if mergeable && let Some(last) = segment.tess_batches.last_mut() {
            last.index_count += index_count;
        } else {
            segment.current_pipeline_key = Some(key);
            segment.tess_batches.push(TessellatedBatch {
                pipeline_key: key,
                scissor: state.current_scissor(),
                index_start,
                index_count,
                clip,
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
        let clip_for_isolated = state.active_clip();
        let mut path_segment = DrawSegment::new();
        path_segment.vertices.extend_from_slice(vertices);
        path_segment.indices.extend(indices.iter().copied());
        path_segment.current_pipeline_key = Some(PipelineKey::alpha_blend());
        path_segment.tess_batches.push(TessellatedBatch {
            pipeline_key: PipelineKey::alpha_blend(),
            scissor: state.current_scissor(),
            index_start: 0,
            index_count: indices.len() as u32,
            clip: clip_for_isolated,
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
        shader: &flui_painting::paint::Shader,
    ) -> Vec<crate::effects::GradientStop> {
        let (colors, stops) = match shader {
            flui_painting::paint::Shader::LinearGradient { colors, stops, .. }
            | flui_painting::paint::Shader::RadialGradient { colors, stops, .. }
            | flui_painting::paint::Shader::SweepGradient { colors, stops, .. } => {
                (colors.as_slice(), stops.as_deref())
            }
            flui_painting::paint::Shader::Solid { color } => {
                return vec![
                    crate::effects::GradientStop::new(*color, 0.0),
                    crate::effects::GradientStop::new(*color, 1.0),
                ];
            }
            _ => return vec![],
        };

        let count = colors.len().min(8);
        (0..count)
            .map(|i| {
                let even = i as f32 / (count - 1).max(1) as f32;
                // Stop positions are logical f64; the GPU stop is f32.
                let position = stops
                    .and_then(|s| s.get(i).copied())
                    .map_or(even, |p| p as f32);
                crate::effects::GradientStop::new(colors[i], position)
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
fn vertices_aabb(vertices: &[Vertex]) -> Rect<f64> {
    if vertices.is_empty() {
        return Rect::from_xywh(0.0, 0.0, 0.0, 0.0);
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

    Rect::from_ltrb(
        f64::from(min_x),
        f64::from(min_y),
        f64::from(max_x),
        f64::from(max_y),
    )
}

// ─── Unit tests ───────────────────────────────────────────────────────────────
//
// Placed here because they require direct access to `DrawBatcher`,
// `GpuStateStack`, `Vertex`, and `vertices_aabb` which are `pub(super)` items
// visible only within the `wgpu` module and its direct children — this file is
// a direct child of `wgpu`, so all of those items are in scope.

#[cfg(test)]
mod unit_tests {
    use flui_foundation::geometry::Rect;
    use flui_painting::BlendMode;

    use super::{DrawBatcher, PipelineKey, Vertex};
    use crate::{
        command_ir::{DrawItem, DrawSegment},
        state_stack::GpuStateStack,
    };

    // ── Helper: build the minimal geometry for a quad ─────────────────────────

    /// No kind seal may finalize a segment that carries gradient stops.
    ///
    /// Gradients skip `begin_phase`, but that alone does not protect them: a
    /// backward transition between two OTHER kinds still seals the segment they
    /// live in. Every segment's stop table is uploaded to the same
    /// `gradient_stops_buffer` at offset 0 and all passes are recorded into one
    /// `CommandEncoder` before submission, so the last write wins — two
    /// gradient-bearing segments in a frame cannot both be correct.
    ///
    /// Sequence: gradient, then circle, then rect. The circle -> rect step is
    /// the backward transition; without the guard it seals a segment holding
    /// the gradient's stops.
    ///
    /// If reverted (drop `&& segment.current_gradient_stops.is_empty()` from
    /// `begin_phase`): one sealed segment carries stops and this fails.
    #[test]
    fn a_kind_seal_never_splits_gradient_bearing_content() {
        use crate::command_ir::Phase;
        use crate::effects::GradientStop;

        let state = GpuStateStack::new();
        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let stops = [
            GradientStop::from_rgba([1.0, 0.0, 0.0, 1.0], 0.0),
            GradientStop::from_rgba([1.0, 0.0, 0.0, 1.0], 1.0),
        ];

        DrawBatcher::draw_gradient_rect(
            &mut segment,
            &state,
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
            glam::Vec2::new(0.0, 0.0),
            glam::Vec2::new(10.0, 0.0),
            &stops,
            [0.0; 4],
            BlendMode::SrcOver,
        );
        assert!(
            !segment.current_gradient_stops.is_empty(),
            "precondition: the segment carries gradient stops"
        );

        DrawBatcher::begin_phase(&mut segment, &mut draw_order, Phase::Circle);
        DrawBatcher::begin_phase(&mut segment, &mut draw_order, Phase::Rect);

        let sealed_with_stops = draw_order
            .iter()
            .filter(
                |it| matches!(it, DrawItem::Segment(seg) if !seg.current_gradient_stops.is_empty()),
            )
            .count();
        assert_eq!(
            sealed_with_stops, 0,
            "a kind seal finalized a gradient-bearing segment; its stop table and the \
             next one both upload to the same buffer, so the earlier gradient renders \
             with the later colours"
        );
    }

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

    // ── S2: SrcOver key → stays in Segment ────────────────────────────────────

    // ── S3: Plus/Modulate → Segment (Porter-Duff, not advanced) ───────────────

    // ── S4: device_bounds AABB ────────────────────────────────────────────────

    // ── S4b: shape segment batch uses alpha_blend (SrcOver) ───────────────────

    // ── S4c: AdvancedShapeOp::mode carries original mode ─────────────────────

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

    // ── S4f: empty vertices_aabb returns origin ───────────────────────────────

    // ── linear gradient advanced mode → AdvancedShape ───────────────────────

    // ── radial gradient advanced mode → AdvancedShape ────────────────────────

    // ── sweep gradient advanced mode → AdvancedShape ─────────────────────────

    // ── SrcOver gradient stays in main segment ────────────────────────────────

    /// `dispatch_shader_rect` with a linear gradient and `SrcOver` must NOT
    /// produce `DrawItem::AdvancedShape`.  The SrcOver path is byte-identical to
    /// what it was before advanced-blend support existed.
    ///
    /// **Proves:** the advanced diversion gate (`is_advanced()`) correctly rejects
    /// SrcOver, leaving the gradient in the main segment's gradient batch.
    #[test]
    fn srcover_gradient_stays_in_main_segment() {
        use flui_foundation::geometry::Offset;
        use flui_painting::Paint;
        use flui_painting::paint::{Shader, TileMode};

        let mut segment = DrawSegment::new();
        let mut draw_order: Vec<DrawItem> = Vec::new();
        let state = GpuStateStack::new_for_test();

        let bounds = Rect::from_xywh(0.0, 0.0, 64.0, 64.0);
        let paint = Paint {
            blend_mode: BlendMode::SrcOver,
            shader: Some(Shader::LinearGradient {
                from: Offset::new(0.0, 0.0),
                to: Offset::new(64.0, 0.0),
                colors: vec![
                    flui_painting::styling::Color::rgba(255, 0, 0, 255),
                    flui_painting::styling::Color::rgba(0, 0, 255, 255),
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

    // ── isolated segment stop_offset is 0 ────────────────────────────────────

    // ── Z-seal fires for gradient advanced mode ───────────────────────────────

    // ── S4g: seal fires before advancing — prior segment preserved ────────────
}

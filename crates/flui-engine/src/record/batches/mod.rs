//! Record-side draw accumulation: `DrawBatcher` owns the two mutable, non-GPU
//! assets used only while recording, the `tessellator` (lyon) and the
//! `path_cache` (tessellations keyed by path and scale).
//!
//! For each record call the caller (`WgpuPainter`) passes in the draw state
//! and the accumulation targets as plain borrowed parameters:
//! - `segment: &mut DrawSegment`      — current accumulation buffer
//! - `draw_order: &mut Vec<DrawItem>` — ordered list of sealed segments
//! - `state: &GpuStateStack`          — read-only transform/scissor queries
//! - `opacity: f32`                   — current opacity, read before the call
//!
//! This borrow seam lets the painter borrow its batcher, segment, draw order
//! and state stack at once; reading `opacity` into an `f32` local before the
//! call keeps the layer compositor from being borrowed across the batcher
//! invocation.
//!
//! # Shader dispatch
//!
//! `dispatch_shader` (gradient and shader fills for rect, rrect and circle, and
//! the tessellated outline of a shaded path) lives on `DrawBatcher`, as do the
//! gradient methods (`draw_gradient_rect`, `draw_sweep_gradient_rect`) and
//! `draw_shadow_rect`. Each takes `(&mut DrawSegment, &GpuStateStack, …)` through
//! the same seam. `draw_path` owns the tessellation-cache hit and miss logic;
//! `draw_vertices` owns the per-vertex colour and uv assembly and the u16→u32
//! index conversion.
//!
//! # Invariants
//!
//! - A non-`SrcOver` segment is sealed in `add_tessellated_with_key`, right
//!   after its batch entry is appended, through `&mut draw_order`.
//! - Scissor coalescing reads `state.current_scissor()` as a `Copy` value at
//!   the moment of the draw.
//! - Opacity is baked at record time: the `WgpuPainter` shim reads it before
//!   the batcher call, ahead of any change to the compositor stack.
//! - Recording allocates nothing per draw beyond the segment's own growth.

use flui_foundation::geometry::Rect;
use flui_painting::paint::BlendMode;

use crate::{
    pipelines::variants::PipelineKey,
    record::command_ir::{
        AdvancedShapeOp, DrawItem, DrawRun, DrawSegment, SsaaPathOp, TessellatedBatch,
    },
    record::path_cache::PathCache,
    record::state_stack::GpuStateStack,
    record::tessellator::Tessellator,
    record::vertex::Vertex,
};

mod gradients;
pub(crate) use gradients::MAX_GRADIENT_STOPS;
use gradients::ShaderGeometry;
pub(crate) use text::{GlyphCull, GlyphInk, outside_scissor};
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
pub(crate) struct DrawBatcher {
    /// Lyon-based path tessellator for complex shapes.
    pub(crate) tessellator: Tessellator,

    /// Per-frame tessellation cache: avoids re-tessellating identical paths within
    /// a frame.
    pub(crate) path_cache: PathCache,
}

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
impl DrawBatcher {
    /// Construct a `DrawBatcher` with the same cache capacities used by the
    /// original `WgpuPainter::with_shared_device`.
    pub(crate) fn new() -> Self {
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
    /// (an ordered backdrop), by the non-`SrcOver` draw-order
    /// contract in [`DrawBatcher::add_tessellated_with_key`], or by the final
    /// flush before GPU submission — routes through here.
    ///
    /// # Allocation strategy — `mem::take` over `mem::replace(…, segment.empty_sibling())`
    ///
    /// The previous implementation called `mem::replace(segment, segment.empty_sibling())`
    /// on every seal, which triggered 7 `InstanceBatch::new(1024)` allocation calls
    /// (7 × `Vec::with_capacity(1024 × sizeof(T))`) plus 11 more `Vec::new()` calls.
    /// `mem::take` leaves the slot as `DrawSegment::default()` (zero-capacity Vecs)
    /// so no heap allocation occurs at seal time — constituent `Vec`s grow lazily
    /// on first push in the next batch.
    pub(crate) fn finish_current_segment(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
    ) {
        // `mem::take` moves completed data out in O(1); `segment` becomes a
        // zero-capacity `DrawSegment::default()` — no allocation at seal time.
        let replacement = segment.empty_sibling();
        let completed = std::mem::replace(segment, replacement);
        if !completed.is_empty() {
            draw_order.push(DrawItem::Segment(completed.seal()));
        }
        // If the segment was empty `completed` is dropped immediately (no data,
        // no capacity worth recycling). `segment` already holds the zero-cap
        // default from `take`.
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
    /// 3. `pipeline_key_from_paint` returns a `with_blend(mode)` key for
    ///    advanced modes (carrying the original mode), so `key.blend_mode().is_advanced()`
    ///    fires here and the key never reaches `PipelineVariants::get_or_create`.
    ///
    /// Plus and Modulate are Porter-Duff (`is_advanced()` = false) and take the
    /// existing fixed-function path unchanged.
    pub(crate) fn add_tessellated_with_key(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: impl IntoIterator<Item = Vertex>,
        indices: &[u32],
        key: PipelineKey,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

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

            // Build an isolated DrawSegment directly from the device-space
            // vertices, without an intermediate copy of a cached path.
            let clip_for_isolated = state.active_clip();
            let mut shape_segment = segment.empty_sibling();
            // Indices reference vertices[0..], so base_index = 0.
            shape_segment.vertices.extend(vertices);
            let device_bounds = vertices_aabb(&shape_segment.vertices);
            shape_segment.indices.extend(indices.iter().copied()); // already 0-based
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
            shape_segment.record_run(DrawRun::Tess(0..1), state.clip_chain());

            draw_order.push(DrawItem::AdvancedShape(AdvancedShapeOp {
                segment: shape_segment.seal(),
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

        let base_index = segment.vertices.len() as u32;
        let index_start = segment.indices.len() as u32;

        segment.vertices.extend(vertices);
        segment
            .indices
            .extend(indices.iter().map(|&i| i + base_index));

        let index_count = indices.len() as u32;

        segment.push_tess_batch(
            TessellatedBatch {
                pipeline_key: key,
                scissor: state.current_scissor(),
                index_start,
                index_count,
                clip: state.active_clip(),
            },
            state.clip_chain(),
        );

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
    pub(crate) fn divert_path_to_ssaa(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: impl IntoIterator<Item = Vertex>,
        indices: &[u32],
        blend: BlendMode,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

        if indices.is_empty() {
            return;
        }

        // Step 1: seal prior content so it appears below the SSAA tile.
        Self::finish_current_segment(segment, draw_order);

        // Build an isolated DrawSegment for this path only.
        // The internal pipeline is always SrcOver (alpha-blend): the geometry is
        // rendered into a transparent offscreen tile.  The SSAA blend mode is stored
        // in `SsaaPathOp::blend` and applied at composite time, not at raster time.
        let clip_for_isolated = state.active_clip();
        let mut path_segment = segment.empty_sibling();
        path_segment.vertices.extend(vertices);
        let device_bounds = vertices_aabb(&path_segment.vertices);
        path_segment.indices.extend(indices.iter().copied());
        path_segment.tess_batches.push(TessellatedBatch {
            pipeline_key: PipelineKey::alpha_blend(),
            scissor: state.current_scissor(),
            index_start: 0,
            index_count: indices.len() as u32,
            clip: clip_for_isolated,
        });
        path_segment.record_run(DrawRun::Tess(0..1), state.clip_chain());

        draw_order.push(DrawItem::SsaaPath(SsaaPathOp {
            segment: path_segment.seal(),
            device_bounds,
            blend,
        }));
    }

    /// Apply the current world transform to every vertex position in `vertices`
    /// (already tessellated in local space) and submit to the tessellated batch.
    ///
    /// `shape.wgsl` only converts px→clip via the viewport uniform; it has no
    /// model-matrix uniform, so the CPU must bake the transform at record time.
    pub(crate) fn submit_transformed_geometry(
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        vertices: impl IntoIterator<Item = Vertex>,
        indices: &[u32],
        key: PipelineKey,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

        let transform = state.current_transform();
        let vertices = vertices.into_iter().map(|mut v| {
            let transformed = transform * glam::vec4(v.position[0], v.position[1], 0.0, 1.0);
            v.position = [transformed.x, transformed.y];
            v
        });
        Self::add_tessellated_with_key(segment, draw_order, state, vertices, indices, key);
    }

    /// Prime the tessellator's flatten tolerance from the current CTM max-basis
    /// length.  Must be called immediately before any `tessellate_*` invocation.
    pub(crate) fn prime_tessellator_scale(&mut self, state: &GpuStateStack) {
        self.tessellator.set_max_scale(state.max_scale());
    }

    /// Convert a `Shader` into GPU `GradientStop`s (max 8 stops).
    ///
    /// Called by `DrawBatcher::dispatch_shader` which lives in the same module.
    pub(crate) fn shader_to_gradient_stops(
        shader: &flui_painting::paint::Shader,
        budget: &std::sync::Arc<crate::record::budget::RecordingBudget>,
    ) -> crate::record::budget::BudgetVec<crate::record::effects::GradientStop> {
        let (colors, stops) = match shader {
            flui_painting::paint::Shader::LinearGradient { colors, stops, .. }
            | flui_painting::paint::Shader::RadialGradient { colors, stops, .. }
            | flui_painting::paint::Shader::SweepGradient { colors, stops, .. } => {
                (colors.as_slice(), stops.as_deref())
            }
            flui_painting::paint::Shader::Solid { color } => {
                let mut values = crate::record::budget::BudgetVec::with_budget(budget);
                values.extend_from_slice(&[
                    crate::record::effects::GradientStop::new(*color, 0.0),
                    crate::record::effects::GradientStop::new(*color, 1.0),
                ]);
                return values;
            }
            _ => return crate::record::budget::BudgetVec::with_budget(budget),
        };

        let count = colors.len();
        let mut values = crate::record::budget::BudgetVec::with_budget(budget);
        values.extend((0..count).map(|i| {
            let even = i as f32 / (count - 1).max(1) as f32;
            // Stop positions are logical f64; the GPU stop is f32.
            let position = stops
                .and_then(|s| s.get(i).copied())
                .map_or(even, |p| p as f32);
            crate::record::effects::GradientStop::new(colors[i], position)
        }));
        values
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

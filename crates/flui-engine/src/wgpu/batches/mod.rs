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

use super::{
    command_ir::{DrawItem, DrawSegment, TessellatedBatch},
    path_cache::PathCache,
    pipeline::PipelineKey,
    state_stack::GpuStateStack,
    superellipse_cache::SuperellipsePathCache,
    tessellator::Tessellator,
    vertex::Vertex,
};

mod gradients;
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

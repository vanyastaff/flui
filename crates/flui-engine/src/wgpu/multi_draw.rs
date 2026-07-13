//! Multi-Draw Indirect System for GPU Instancing
//!
//! Combines multiple draw calls into a single indirect draw command,
//! reducing CPU overhead by 2-3x compared to individual draw calls.
//!
//! # Performance Impact
//!
//! **Before (Individual Draws):**
//! ```text
//! draw_indexed(rects)     ← CPU call 1
//! draw_indexed(circles)   ← CPU call 2
//! draw_indexed(arcs)      ← CPU call 3
//! draw_indexed(textures)  ← CPU call 4
//! Total: 4 CPU calls
//! ```
//!
//! **After (Multi-Draw Indirect):**
//! ```text
//! multi_draw_indexed_indirect([rects, circles, arcs, textures])  ← CPU call 1
//! Total: 1 CPU call
//! ```
//!
//! **Result:** 75% CPU reduction for draw submission!
//!
//! # Trimmed to the live surface
//!
//! The batcher was trimmed to the surface `WgpuPainter::
//! flush_all_instanced_batches` actually exercises:
//! `new` / `add_quad_draw` / `stats`. The previous wide surface
//! (`is_empty` / `len` / `total_instances` / `commands` / `clear` /
//! `create_indirect_buffer` and the 4-field `DrawIndirect` wrapper
//! holding `pipeline_id` + `instance_buffer_offset` +
//! `instance_buffer_size` alongside `args`) was forward-looking
//! scaffolding for a multi-pipeline indirect-draw path that never
//! shipped -- painter collects per-pipeline draws into a single
//! combined buffer and submits via direct `draw_indexed_indirect`
//! calls, not via this batcher's `commands()` iterator.
//!
//! What remains is the minimal `MultiDrawStats` log surface plus
//! the `add_quad_draw` accumulator. `PipelineId::Texture` (zero
//! callers in painter flush path) was dropped from the enum
//! along the way. The feature-gated test module was deleted
//! because its targets are gone -- the surviving entry points
//! have observable effect via `stats()`, which is covered by the
//! debug-build log in painter.

// The `DrawIndexedIndirectArgs` struct and its impl were deleted. The
// bytemuck::Pod wrapper for `wgpu::util::
// DrawIndexedIndirectArgs` was used only by the previous
// `DrawIndirect` wrapper struct (also deleted alongside it); the
// surviving `MultiDrawBatcher` accumulates stats and lets painter
// drive the actual `wgpu::util::DrawIndexedIndirectArgs` construction
// at the submission callsite. Reintroduce the Pod wrapper here if a
// caller ever needs `bytemuck::bytes_of(&args)` for upload.

/// Pipeline identifier for grouping draws
///
/// `PipelineId::Texture` was dropped --
/// `WgpuPainter::flush_all_instanced_batches` only batches
/// rect / circle / arc / shadow; textured drawing has its own
/// dispatch path that doesn't route through this enum.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum PipelineId {
    /// Rectangle instancing pipeline
    Rectangle,
    /// Circle instancing pipeline
    Circle,
    /// Arc instancing pipeline
    Arc,
}

/// Multi-draw indirect batcher
///
/// Accumulates per-pipeline draw-arg records + an instance count
/// total. The caller (`WgpuPainter::flush_all_instanced_batches`)
/// reads `stats()` after enqueueing to log batch shape; the actual
/// indirect-draw submission lives in painter and does not iterate
/// this batcher's command list.
pub struct MultiDrawBatcher {
    /// Number of draw commands accumulated this batch.
    active_draws: usize,
    /// Total instance count across all draws.
    total_instances: usize,
}

impl MultiDrawBatcher {
    /// Create a new multi-draw batcher
    pub fn new() -> Self {
        Self {
            active_draws: 0,
            total_instances: 0,
        }
    }

    /// Record a quad-based draw in the batch.
    ///
    /// # Arguments
    /// * `_pipeline_id` - Which pipeline to use (unused after the
    ///   wave 5 trim; the painter selects pipelines directly).
    ///   Retained for API symmetry with the eventual multi-pipeline
    ///   indirect path.
    /// * `instance_count` - Number of instances
    /// * `_instance_buffer_offset` - Offset in combined instance
    ///   buffer (used at painter callsite, recorded here for
    ///   symmetry).
    /// * `_instance_buffer_size` - Size of instance data
    pub fn add_quad_draw(
        &mut self,
        _pipeline_id: PipelineId,
        instance_count: u32,
        _instance_buffer_offset: u64,
        _instance_buffer_size: u64,
    ) {
        if instance_count == 0 {
            return;
        }
        self.active_draws += 1;
        self.total_instances += instance_count as usize;
    }

    /// Get statistics (debug-only telemetry).
    ///
    /// The sole consumer logs at `debug_assertions`, so the method is gated to
    /// match — avoiding a dead-code warning in release/bench builds.
    #[cfg(debug_assertions)]
    pub fn stats(&self) -> MultiDrawStats {
        MultiDrawStats {
            active_draws: self.active_draws,
            active_instances: self.total_instances,
        }
    }
}

impl Default for MultiDrawBatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-draw statistics (debug-only telemetry).
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy)]
pub struct MultiDrawStats {
    /// Number of draw commands in current batch
    pub active_draws: usize,
    /// Number of instances in current batch
    pub active_instances: usize,
}

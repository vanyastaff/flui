//! Command IR data types for the `WgpuPainter` draw-record pipeline.
//!
//! This module owns the type definitions for the per-frame record-side IR:
//! the batching/segment/layer types that accumulate draw commands before
//! the painter flushes them to wgpu.
//!
//! **Scope:** pure type definitions + their inherent impls.  The painter's
//! record methods (rect/rrect/circle/…) and flush/replay methods remain in
//! `painter.rs`.  These types are re-exported `pub(crate)` so `painter.rs`
//! and future batcher/compositor modules can import from one place.

use flui_types::{Rect, geometry::Pixels};

use super::{
    effects::GradientStop,
    instancing::{
        ArcInstance, CircleInstance, InstanceBatch, LinearGradientInstance, RadialGradientInstance,
        RectInstance, ShadowInstance, SweepGradientInstance, TextureInstance,
    },
    pipeline::PipelineKey,
    texture_cache::TextureId,
    texture_pool::PooledTexture,
    vertex::Vertex,
};

// ─── Primitive helpers ────────────────────────────────────────────────────────

/// Scissor rect type (x, y, width, height) in physical pixels.
pub(crate) type ScissorRect = Option<(u32, u32, u32, u32)>;

/// Tracks a sub-range of instances that share the same scissor state.
/// Used to split instanced draw calls when clipping changes.
#[derive(Debug, Clone)]
pub(crate) struct ScissorRegion {
    pub(crate) scissor: ScissorRect,
    pub(crate) start: u32,
    pub(crate) count: u32,
}

/// A recorded batch of tessellated geometry sharing the same pipeline key.
///
/// During a frame, each call to
/// [`super::painter::WgpuPainter::add_tessellated_with_key`] appends
/// vertices/indices to the global buffers.  When the pipeline key changes a
/// new batch is started so that the render pass can switch pipelines at the
/// correct index boundary.
#[derive(Debug, Clone)]
pub(crate) struct TessellatedBatch {
    /// Pipeline variant to use for this batch
    pub(crate) pipeline_key: PipelineKey,
    /// Scissor rect active when this batch was recorded
    pub(crate) scissor: ScissorRect,
    /// First index (inclusive) into the shared index buffer
    pub(crate) index_start: u32,
    /// Number of indices in this batch
    pub(crate) index_count: u32,
}

// ─── Offscreen / layer snapshots ─────────────────────────────────────────────

/// A pending offscreen texture waiting to be composited into the main render target.
///
/// Created by [`super::painter::WgpuPainter::queue_offscreen_result`] and consumed
/// during [`super::painter::WgpuPainter::render`] after all other drawing is complete.
// `wgpu::TextureView` and `PooledTexture` are not `Debug`; no derive possible.
#[allow(missing_debug_implementations)]
pub(crate) struct PendingOffscreenTexture {
    pub(crate) texture: PooledTexture,
    pub(crate) bounds: Rect<Pixels>,
}

/// Saved render state for `save_layer`/`restore_layer` offscreen compositing.
///
/// When `save_layer` is called, the current draw state is captured into this
/// struct and a fresh segment begins. All subsequent drawing goes into the new
/// segment. On `restore_layer`, the offscreen content is composited back onto
/// the parent surface with the layer's opacity applied as a group.
// `DrawSegment` contains `wgpu::TextureView` fields that are not `Debug`.
#[allow(missing_debug_implementations)]
pub(crate) struct SavedLayer {
    /// Previous draw order (restored on pop)
    pub(crate) saved_draw_order: Vec<DrawItem>,
    /// Previous segment (restored on pop)
    pub(crate) saved_segment: DrawSegment,
    /// Previous opacity stack (restored on pop)
    pub(crate) saved_opacity_stack: Vec<f32>,
    /// Previous accumulated opacity (restored on pop)
    pub(crate) saved_opacity: f32,
    /// Opacity to apply when compositing the offscreen layer
    pub(crate) layer_opacity: f32,
    /// Per-channel tint applied when compositing the offscreen layer.
    ///
    /// White (`(1.0, 1.0, 1.0)`) for a plain opacity layer; a non-white value
    /// carries the ColorFilter chroma (`filter.apply([1,1,1,1])` RGB) so hue
    /// shifts survive compositing. Captured from `paint.color` in `save_layer`.
    pub(crate) layer_tint_rgb: [f32; 3],
    /// Bounds of the layer in screen space [x, y, w, h], or `None` for full viewport
    pub(crate) bounds: Option<[f32; 4]>,
}

// ─── Draw segment ─────────────────────────────────────────────────────────────

/// A segment of draw commands that share the same rendering phase ordering.
///
/// When an offscreen texture is queued, the current segment is finalized and
/// a new one starts. This ensures that content drawn before the offscreen
/// texture renders before it, and content drawn after renders after it,
/// preserving correct Z-order.
// `wgpu::TextureView` (inside `external_images`) is not `Debug`.
#[allow(missing_debug_implementations)]
pub(crate) struct DrawSegment {
    /// Rectangle instance batch
    pub(crate) rect_batch: InstanceBatch<RectInstance>,
    /// Circle instance batch
    pub(crate) circle_batch: InstanceBatch<CircleInstance>,
    /// Arc instance batch
    pub(crate) arc_batch: InstanceBatch<ArcInstance>,
    /// Shadow instance batch
    pub(crate) shadow_batch: InstanceBatch<ShadowInstance>,
    /// Linear gradient instance batch
    pub(crate) linear_gradient_batch: InstanceBatch<LinearGradientInstance>,
    /// Radial gradient instance batch
    pub(crate) radial_gradient_batch: InstanceBatch<RadialGradientInstance>,
    /// Sweep gradient instance batch
    pub(crate) sweep_gradient_batch: InstanceBatch<SweepGradientInstance>,
    /// Accumulated gradient stops for this segment
    pub(crate) current_gradient_stops: Vec<GradientStop>,
    /// Batched vertices for tessellation path
    pub(crate) vertices: Vec<Vertex>,
    /// Batched indices for tessellation path
    pub(crate) indices: Vec<u32>,
    /// Recorded tessellated batches for this segment
    pub(crate) tess_batches: Vec<TessellatedBatch>,
    /// Current pipeline key (for batching draws with same pipeline)
    pub(crate) current_pipeline_key: Option<PipelineKey>,
    /// Scissor regions for rect instanced batch
    pub(crate) rect_scissors: Vec<ScissorRegion>,
    /// Scissor regions for circle instanced batch
    pub(crate) circle_scissors: Vec<ScissorRegion>,
    /// Scissor regions for arc instanced batch
    pub(crate) arc_scissors: Vec<ScissorRegion>,
    /// Scissor regions for linear gradient batch
    pub(crate) linear_grad_scissors: Vec<ScissorRegion>,
    /// Scissor regions for radial gradient batch
    pub(crate) radial_grad_scissors: Vec<ScissorRegion>,
    /// Scissor regions for sweep gradient batch
    pub(crate) sweep_grad_scissors: Vec<ScissorRegion>,
    /// Cached image draws queued for this segment.
    ///
    /// The third element is the scissor rect active at draw time, forwarded to
    /// `flush_texture_batch` so clipped images don't spill outside their clip region.
    pub(crate) cached_images: Vec<(TextureId, TextureInstance, ScissorRect)>,
    /// External-texture draws queued for this segment.
    ///
    /// Each entry carries the registry's `wgpu::TextureView` alongside the
    /// instance data so `flush_segment_external_images` can bind each view
    /// independently — identical to how `flush_segment_cached_images` binds
    /// per-texture views for the atlas cache.
    ///
    /// The third element is the scissor rect active at draw time.
    pub(crate) external_images: Vec<(wgpu::TextureView, TextureInstance, ScissorRect)>,
}

impl DrawSegment {
    /// Create an empty draw segment with pre-allocated batch capacities.
    pub(crate) fn new() -> Self {
        Self {
            rect_batch: InstanceBatch::new(1024),
            circle_batch: InstanceBatch::new(1024),
            arc_batch: InstanceBatch::new(1024),
            shadow_batch: InstanceBatch::new(1024),
            linear_gradient_batch: InstanceBatch::new(512),
            radial_gradient_batch: InstanceBatch::new(512),
            sweep_gradient_batch: InstanceBatch::new(512),
            current_gradient_stops: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            tess_batches: Vec::new(),
            current_pipeline_key: None,
            rect_scissors: Vec::new(),
            circle_scissors: Vec::new(),
            arc_scissors: Vec::new(),
            linear_grad_scissors: Vec::new(),
            radial_grad_scissors: Vec::new(),
            sweep_grad_scissors: Vec::new(),
            cached_images: Vec::new(),
            external_images: Vec::new(),
        }
    }

    /// Record an instance addition for a given scissor region tracker.
    /// Extends the last region if the scissor matches, or creates a new one.
    pub(crate) fn push_scissor_region(regions: &mut Vec<ScissorRegion>, scissor: ScissorRect) {
        if let Some(last) = regions.last_mut()
            && last.scissor == scissor
        {
            last.count += 1;
            return;
        }
        regions.push(ScissorRegion {
            scissor,
            start: regions.last().map_or(0, |r| r.start + r.count),
            count: 1,
        });
    }

    /// Returns `true` if this segment has no drawing commands.
    pub(crate) fn is_empty(&self) -> bool {
        self.rect_batch.is_empty()
            && self.circle_batch.is_empty()
            && self.arc_batch.is_empty()
            && self.shadow_batch.is_empty()
            && self.linear_gradient_batch.is_empty()
            && self.radial_gradient_batch.is_empty()
            && self.sweep_gradient_batch.is_empty()
            && self.vertices.is_empty()
            && self.tess_batches.is_empty()
            && self.cached_images.is_empty()
            && self.external_images.is_empty()
    }
}

// ─── Draw item (top-level ordering enum) ─────────────────────────────────────

/// An item in the draw order list: either a segment of batched commands,
/// an offscreen texture to composite, or an opacity layer.
// `DrawSegment` and `PendingOffscreenTexture` are not `Debug`.
#[allow(missing_debug_implementations)]
pub(crate) enum DrawItem {
    /// A segment of instanced/tessellated/gradient draw commands.
    Segment(DrawSegment),
    /// An offscreen texture to composite at its bounds.
    OffscreenTexture(PendingOffscreenTexture),
    /// An opacity layer: a group of draw items to render offscreen and composite
    /// with the given alpha. Created by `save_layer`/`restore_layer`.
    OpacityLayer(PendingOpacityLayer),
}

// ─── Opacity layer ────────────────────────────────────────────────────────────

/// A pending opacity layer waiting to be rendered offscreen and composited.
///
/// Created by [`super::painter::WgpuPainter::restore_layer`] when opacity < 1.0.
/// During [`super::painter::WgpuPainter::render`], the contained segments are
/// flushed to a pooled offscreen texture, then that texture is composited onto
/// the main surface with the layer opacity applied as tint alpha.
// `DrawSegment` (via `DrawItem`) is not `Debug`.
#[allow(missing_debug_implementations)]
pub(crate) struct PendingOpacityLayer {
    /// Draw items accumulated between save_layer and restore_layer
    pub(crate) items: Vec<DrawItem>,
    /// Final segment at the time of restore_layer (may have content)
    pub(crate) final_segment: DrawSegment,
    /// Group opacity to apply during compositing (0.0–1.0)
    pub(crate) opacity: f32,
    /// Per-channel chroma tint for ColorFilter layers; white for plain opacity.
    /// See [`SavedLayer::layer_tint_rgb`].
    pub(crate) tint_rgb: [f32; 3],
    /// Compositing bounds in screen coordinates
    pub(crate) bounds: Rect<Pixels>,
}

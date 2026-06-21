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

use flui_types::{
    Rect, geometry::Pixels, painting::BlendMode, painting::TextureId as ExternalTextureId,
};

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

// ─── Layer filter ────────────────────────────────────────────────────────────

/// A pixel-level filter applied to the rendered content of an offscreen layer
/// before it is composited onto its parent surface.
///
/// The filter is applied during `flush_opacity_layer`, immediately after
/// `render_layer_to_offscreen` completes, by a full-screen quad pass that reads
/// the layer offscreen and writes into a separate pooled texture (ping-pong
/// avoiding read/write aliasing).  The filtered texture is then forwarded into
/// both composite arms in place of the raw offscreen.
///
/// ## Correctness invariant — premultiplied alpha
///
/// The layer offscreen is premultiplied RGBA.  The `ColorMatrix` variant stores
/// values that operate on **straight** (un-premultiplied) RGBA, matching
/// [`flui_types::painting::ColorMatrix::apply`].  The GPU shader MUST
/// unpremultiply before the matrix, clamp each output channel to `[0, 1]`,
/// and repremultiply before writing, producing the identical result the CPU oracle
/// would return for each pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LayerFilter {
    /// A 5×4 row-major color matrix applied per-pixel on un-premultiplied color.
    ///
    /// Layout mirrors [`flui_types::painting::ColorMatrix::values`]:
    /// rows R/G/B/A × columns `[m0..m3, offset]`.
    ColorMatrix([f32; 20]),
}

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
/// [`super::batches::DrawBatcher::add_tessellated_with_key`] appends
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
// `saved_draw_order: Vec<DrawItem>` is not `Debug` because `DrawItem::OffscreenTexture`
// wraps `PooledTexture` which wraps a non-`Debug` `wgpu::Texture`.
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
    /// Blend mode to apply when compositing this layer onto its parent.
    ///
    /// Stored on the record side so the compositor dispatch can read it without
    /// coupling the flush path to the record path.  Defaults to `SrcOver`.
    pub(crate) layer_blend: BlendMode,
    /// Optional per-pixel filter applied to the rendered layer before compositing.
    ///
    /// `None` = today's behavior (premultiplied tint-only composite).
    /// `Some(LayerFilter::ColorMatrix(_))` routes the rendered offscreen through
    /// the color-matrix GPU pass before the composite step.
    pub(crate) filter: Option<LayerFilter>,
}

// ─── Draw segment ─────────────────────────────────────────────────────────────

/// A segment of draw commands that share the same rendering phase ordering.
///
/// When an offscreen texture is queued, the current segment is finalized and
/// a new one starts. This ensures that content drawn before the offscreen
/// texture renders before it, and content drawn after renders after it,
/// preserving correct Z-order.
///
/// `Clone` is derived because all fields are plain CPU data (no GPU handles).
/// This is the compile-time purity witness: `DrawSegment` can be cloned without
/// touching any `wgpu::Device`, `wgpu::Queue`, `wgpu::Encoder`, or `wgpu::TextureView`.
/// The deterministic-replay test (T11) snapshots a `DrawSegment` before replay to assert
/// the IR is unchanged by `GpuReplay::submit`.
#[derive(Debug, Clone)]
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
    /// Each entry carries the `ExternalTextureId` (a `flui_types::painting::TextureId`)
    /// so the IR is comparable by value and free of non-`PartialEq` wgpu handles.
    /// Resolution from ID to `wgpu::TextureView` happens at replay time in
    /// `flush_segment_external_images`, which calls
    /// `ExternalTextureRegistry::get(id)` immediately before the draw call.
    ///
    /// This means a texture `update()`d between the `draw_texture`/`texture`
    /// call and the frame flush resolves to the newer view — the latest-frame
    /// semantics documented in [`super::external_texture_registry`].
    ///
    /// The third element is the scissor rect active at draw time.
    pub(crate) external_images: Vec<(ExternalTextureId, TextureInstance, ScissorRect)>,
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

// ─── Advanced-shape op ────────────────────────────────────────────────────────

/// A single tessellated shape that requires a dst-read (advanced) blend.
///
/// Created by [`super::batches::DrawBatcher::add_tessellated_with_key`] when
/// the pipeline key carries an advanced (W3C composite) blend mode.  Instead
/// of batching the shape into the current `DrawSegment` (which would use
/// fixed-function blending), the shape's geometry is isolated here and rendered
/// offscreen at replay time so `flush_advanced_layer` can read the backdrop and
/// compute the correct non-separable blend.
///
/// ## T11 purity contract
///
/// `DrawSegment` derives `Clone` as the compile-time IR-purity witness.
/// `AdvancedShapeOp` also derives `Clone` for the same reason — it must be
/// cloneable without touching any GPU handle.  The embedded `DrawSegment` is
/// handle-free by the same invariant.
///
/// ## AA note
///
/// The tessellated geometry is rendered at `sample_count=1` with no SDF
/// anti-aliasing, so shape edges are aliased.  This is consistent with the
/// Phase-A quality note at `batches/shapes.rs`.  Phase B will add SDF AA.
#[derive(Clone)]
pub(crate) struct AdvancedShapeOp {
    /// Tessellated geometry for this shape (vertices already baked to device
    /// space; indices relative to segment-local base).
    pub(crate) segment: DrawSegment,
    /// Advanced blend mode to apply when compositing the shape onto the surface.
    pub(crate) mode: BlendMode,
    /// Device-space AABB of the producer's coverage in device pixels.
    ///
    /// For tessellated shapes: computed as the AABB of `segment.vertices[*].position`
    /// (baked to device space at record time).
    ///
    /// For gradient and image producers: the transformed quad/rect or union of
    /// tile/sprite destination rects in device space (no vertex array is stored for
    /// these instanced producers).
    ///
    /// Used by `flush_advanced_layer` for the backdrop-copy region, the `src_uv`
    /// remap, and the damage-straddle guard.
    pub(crate) device_bounds: Rect<Pixels>,
}

// ─── SSAA-path op ────────────────────────────────────────────────────────────

/// A single geometry fill routed to the SSAA offscreen tile for anti-aliased
/// compositing.
///
/// Created by:
/// - `DrawBatcher::draw_path` (in `batches/paths.rs`) for arbitrary path fills
///   (SrcOver, tile-safe Porter-Duff, and advanced blend modes).
/// - `batches/shapes.rs` non-SrcOver branches for rect/rrect/circle/oval/arc fills
///   when the blend mode is tile-safe or advanced.
///
/// Instead of batching the tessellated geometry directly into the main
/// `DrawSegment` (which renders aliased at `sample_count=1`), the geometry is
/// isolated here and rendered at replay time into a 2× supersampled pooled
/// offscreen, box-downsampled to a premultiplied 1× tile, and composited via
/// one of three paths depending on `blend`:
///
/// - **SrcOver / tile-safe Porter-Duff** (`is_tile_safe_for_ssaa(blend)` = true):
///   premultiplied texture composite with `blend_state_for(blend)` fixed-function
///   blend.  Transparent-padding pixels are a no-op on the destination.
/// - **Advanced (dst-read)** (`blend.is_advanced()` = true):
///   `flush_advanced_layer` with the 1× tile as the foreground texture.
///   W3C composite with correct coverage.
/// - **Coverage-destructive Porter-Duff** (all other non-advanced modes):
///   NOT routed here — these keep the existing tessellated (aliased) path because
///   the transparent tile padding would destructively modify the destination
///   outside the geometry boundary.  See the coverage-destructive exception list
///   in `batches/shapes.rs` and `batches/paths.rs`.
///
/// ## T11 purity contract
///
/// `SsaaPathOp` derives `Clone` — it is handle-free (no `wgpu::*` fields),
/// matching the [`AdvancedShapeOp`] invariant.  `BlendMode` is `Copy`.
/// The embedded [`DrawSegment`] carries only CPU data.  All GPU work happens at
/// replay time in `GpuReplay::submit`.
#[derive(Clone)]
pub(crate) struct SsaaPathOp {
    /// Tessellated geometry for this path (vertices already baked to device
    /// space; indices relative to a fresh segment starting at base 0).
    pub(crate) segment: DrawSegment,
    /// Device-space AABB of the path's coverage in device pixels.
    ///
    /// Used to size the SSAA tile: `ceil(device_bounds.width) × ceil(device_bounds.height)`,
    /// clamped to `[1, viewport]`.  Computed as the AABB of
    /// `segment.vertices[*].position` at record time.
    pub(crate) device_bounds: Rect<Pixels>,
    /// Blend mode to use when compositing the SSAA 1× tile onto the surface.
    ///
    /// Determines the composite strategy in `GpuReplay::render_ssaa_path`:
    /// tile-safe → fixed-function premul blend; advanced → `flush_advanced_layer`.
    /// Coverage-destructive modes never reach this struct (they stay tessellated).
    pub(crate) blend: BlendMode,
}

// ─── Draw item (top-level ordering enum) ─────────────────────────────────────

/// An item in the draw order list: either a segment of batched commands,
/// an offscreen texture to composite, an opacity layer, or an advanced shape,
/// or an SSAA-supersampled path tile.
// `PendingOffscreenTexture` (via `OffscreenTexture` variant) is not `Debug`
// because `PooledTexture` wraps a `wgpu::Texture`.
#[allow(missing_debug_implementations)]
pub(crate) enum DrawItem {
    /// A segment of instanced/tessellated/gradient draw commands.
    Segment(DrawSegment),
    /// An offscreen texture to composite at its bounds.
    OffscreenTexture(PendingOffscreenTexture),
    /// An opacity layer: a group of draw items to render offscreen and composite
    /// with the given alpha. Created by `save_layer`/`restore_layer`.
    OpacityLayer(PendingOpacityLayer),
    /// A single tessellated shape drawn with an advanced (dst-read) blend mode.
    ///
    /// Isolated from its surrounding `DrawSegment` at record time so that
    /// `GpuReplay::submit` can render it to an offscreen foreground and call
    /// `flush_advanced_layer` for the backdrop-compositing pass.
    AdvancedShape(AdvancedShapeOp),
    /// An arbitrary SrcOver path fill rendered via SSAA (2× supersample →
    /// box-downsample → premultiplied tile composite).
    ///
    /// Isolated from the surrounding `DrawSegment` at record time so that
    /// `GpuReplay::submit` can execute the 2× render + downsample + composite
    /// sequence at replay time.  Z-order is the insertion position in
    /// `draw_order` (R1 arm order).
    SsaaPath(SsaaPathOp),
}

// ─── Opacity layer ────────────────────────────────────────────────────────────

/// A pending opacity layer waiting to be rendered offscreen and composited.
///
/// Created by [`super::painter::WgpuPainter::restore_layer`] when opacity < 1.0.
/// During [`super::painter::WgpuPainter::render`], the contained segments are
/// flushed to a pooled offscreen texture, then that texture is composited onto
/// the main surface with the layer opacity applied as tint alpha.
// `DrawItem` (via the `OffscreenTexture` variant containing `PooledTexture`) is not `Debug`.
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
    /// Blend mode to apply when compositing this layer onto its parent.
    ///
    /// Stored on the pending layer so the flush path can read it without
    /// coupling it to the record path.  `SrcOver` for plain opacity layers;
    /// an advanced mode for `saveLayer` with an explicit blend mode.
    pub(crate) blend: BlendMode,
    /// Optional per-pixel filter applied to the rendered layer before compositing.
    ///
    /// Forwarded from [`SavedLayer::filter`] at restore time.  `None` = plain
    /// tint-only composite (existing behavior).
    pub(crate) filter: Option<LayerFilter>,
}

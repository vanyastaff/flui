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
use smallvec::SmallVec;

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

/// Direction of the sRGB gamma transfer function.
///
/// Used by [`LayerFilter::Gamma`] to select which transfer direction the GPU
/// shader applies per RGB channel (alpha is always passed through unchanged).
///
/// The underlying transfer functions are the `pub` helpers
/// [`flui_types::styling::color::srgb_to_linear`] and
/// [`flui_types::styling::color::linear_to_srgb`] — the same functions used by
/// the CPU oracle in the GPU readback tests, ensuring one authoritative home for
/// the IEC 61966-2-1 piecewise formula.
///
/// Under **Scope A** this type is constructed only from `cfg(test)` GPU readback
/// tests (no production `push_color_filter` → `LayerFilter::Gamma` wiring yet).
/// The `#[cfg_attr]` gates dead_code away in non-test builds only; under `cfg(test)`
/// the lint stays live so regressions surface immediately.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GammaDirection {
    /// sRGB → linear light: apply the electro-optical transfer function
    /// (gamma-decode each channel).  Corresponds to `srgb_to_linear`.
    SrgbToLinear,
    /// Linear light → sRGB: apply the opto-electronic transfer function
    /// (gamma-encode each channel).  Corresponds to `linear_to_srgb`.
    LinearToSrgb,
}

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
/// The layer offscreen is premultiplied RGBA.  The `ColorMatrix` and `Mode`
/// variants operate on **straight** (un-premultiplied) RGBA: the GPU shader
/// MUST unpremultiply before the operation, clamp each output channel to
/// `[0, 1]`, and repremultiply before writing, producing a result that matches
/// the CPU oracle applied to the straight color.
///
/// The `Gamma` variant also unpremultiplies first, applies the transfer function
/// per RGB channel (alpha is untouched), clamps, and repremultiplies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LayerFilter {
    /// A 5×4 row-major color matrix applied per-pixel on un-premultiplied color.
    ///
    /// Layout mirrors [`flui_types::painting::ColorMatrix::values`]:
    /// rows R/G/B/A × columns `[m0..m3, offset]`.
    ColorMatrix([f32; 20]),

    /// Porter-Duff / W3C blend of a solid filter color over each layer pixel,
    /// applied in straight sRGB space.
    ///
    /// `color` is the **filter** color in straight sRGB `[f32; 4]` (pre-converted
    /// from [`flui_types::Color`] via [`flui_types::Color::to_f32_array`] at the
    /// call site).  The GPU shader unpremultiplies the layer pixel, computes
    /// `blend(src=color, dst=straight_pixel, mode)`, clamps to `[0, 1]`, and
    /// repremultiplies.
    ///
    /// Mirrors [`flui_types::Color::blend`] (`self` = filter color = src,
    /// `dst` = layer pixel): the CPU oracle for the GPU readback tests.
    ///
    /// Constructed only from `#[cfg(test)]` paths under **Scope A** (no
    /// production `push_color_filter` → `LayerFilter::Mode` wiring yet).
    // Scope A: no production producer of this variant yet — it is constructed
    // only in `cfg(test)` GPU readback tests. The `apply_mode` function and
    // `ModePipeline` that consume it are live (referenced in the fold arm),
    // so only the *variant construction sites* need the dead_code gate.
    #[cfg_attr(not(test), allow(dead_code))]
    Mode {
        /// Filter color in straight sRGB `[r, g, b, a]` (values in `[0, 1]`).
        color: [f32; 4],
        /// Blend mode — selects the Porter-Duff or W3C blend function.
        blend_mode: flui_types::painting::BlendMode,
    },

    /// Per-channel sRGB ↔ linear-light transfer function, applied per RGB channel
    /// with alpha untouched.
    ///
    /// The direction is selected by [`GammaDirection`]; the underlying formula is
    /// the IEC 61966-2-1 piecewise function implemented in
    /// [`flui_types::styling::color::srgb_to_linear`] /
    /// [`flui_types::styling::color::linear_to_srgb`].
    ///
    /// The GPU shader unpremultiplies, applies the transfer per R/G/B, clamps to
    /// `[0, 1]`, and repremultiplies; alpha is left unchanged.
    ///
    /// Constructed only from `#[cfg(test)]` paths under **Scope A**.
    // Scope A: same dead_code rationale as `Mode` above.
    #[cfg_attr(not(test), allow(dead_code))]
    Gamma(GammaDirection),
}

/// An inline-storage chain of [`LayerFilter`]s folded in `flush_opacity_layer`.
///
/// Inline capacity N=2 covers the overwhelmingly common cases:
/// - 1 filter (a single `ColorMatrix`), and
/// - 2 filters (a Mode+Gamma pair).
///
/// Image-filter Compose depth (where 4 is realistic) rides `FilterOp::passes`
/// (a different chain). `LayerFilter` stays `Copy`, so push/iterate are cheap.
///
/// `Default` = empty chain = the no-filter fast-path state.
pub(crate) type LayerFilterChain = SmallVec<[LayerFilter; 2]>;

// ─── Image-filter IR ─────────────────────────────────────────────────────────

/// Morphological operation: dilate (max) or erode (min).
///
/// Determines the per-channel accumulation init value and reduction function
/// in the morphology GPU shader:
/// - `Dilate`: init `vec4(0)`, accumulate `max` — expands bright/opaque areas.
/// - `Erode`:  init `vec4(1)`, accumulate `min` — contracts bright/opaque areas.
///
/// ## Premultiplied-direct invariant (PINNED #1)
///
/// The shader applies max/min directly to premultiplied RGBA. No unpremultiply
/// step is performed — this is the correct semantics for morphological filters
/// per Impeller `morphology_filter.frag`. The CPU oracle in the test module
/// follows the same premultiplied-direct contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MorphOp {
    /// Dilate: per-channel maximum — expands bright/opaque regions.
    Dilate,
    /// Erode: per-channel minimum — contracts bright/opaque regions.
    Erode,
}

/// Specification of the image-filter to emit at `save_layer`/`restore_layer`
/// record time.
///
/// Stored in `SavedLayer::image_filter` so `restore_layer` can choose between
/// the `OpacityLayer` and `DrawItem::Filter` paths.
///
/// `Copy` has been intentionally removed: the `Chain` variant holds a
/// `SmallVec<[ImageFilterPass; 4]>` which is `Clone` but not `Copy`.  All
/// eight production call sites pass the spec by value or match by reference, so
/// removing `Copy` has no impact on them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ImageFilterSpec {
    /// Morphological dilate or erode with the given per-axis radius in physical
    /// pixels.
    Morph {
        /// Kernel half-radius in physical pixels: the shader samples
        /// `[-ceil(radius)..=ceil(radius)]` texels in each direction.
        radius: f32,
        /// Whether to accumulate the per-channel maximum (dilate) or minimum
        /// (erode).
        op: MorphOp,
    },
    /// Separable Gaussian blur with independent horizontal and vertical sigma.
    ///
    /// ## PINNED #2 — premultiplied-direct, sRGB-encoded
    ///
    /// The Gaussian kernel operates on premultiplied RGBA in sRGB-encoded space.
    /// NO unpremultiply step, NO linearise. Matching Impeller
    /// `gaussian_blur_filter_contents.cc:935` (`apply_unpremultiply=false`).
    ///
    /// ## √3·sigma kernel extent
    ///
    /// Half-radius = `ceil(sigma × √3)` per [`super::effects::kernel_radius`].
    /// The `grown_bounds` expansion in `restore_layer` uses
    /// `kernel_radius(max(sigma_x, sigma_y))` as a conservative per-axis pad.
    Blur {
        /// Gaussian sigma for the horizontal sub-pass.
        sigma_x: f32,
        /// Gaussian sigma for the vertical sub-pass.
        sigma_y: f32,
    },
    /// A pre-flattened ordered chain of [`ImageFilterPass`]es produced by
    /// `flatten_compose` in `backend.rs` for `ImageFilter::Compose`.
    ///
    /// The passes are already in execution order (index 0 = innermost = applied
    /// first), and `restore_layer` emits a single `DrawItem::Filter` carrying
    /// the full chain.  The `cumulative_growth` helper in `painter.rs` sums the
    /// per-pass radius contributions to produce the expanded `grown_bounds`.
    ///
    /// Inline capacity 4 covers realistic `Compose` depth; heap-spills beyond 4
    /// are correct.
    Chain(SmallVec<[ImageFilterPass; 4]>),
}

/// A lowered, flattened image-filter pass.
///
/// Passes are either bounds-GROWING (Morph, Blur) or bounds-PRESERVING
/// (ColorMatrix, Identity).  The `cumulative_growth` helper in `painter.rs`
/// returns the correct growth for each variant; `ColorMatrix` and `Identity`
/// contribute 0.
///
/// Adding a new variant requires adding a match arm in
/// `apply_image_filter_passes` — the compiler enforces this (no `_` catch-all).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ImageFilterPass {
    /// Passthrough: render the input segment and copy it through unchanged.
    ///
    /// Exercises the `DrawItem::Filter` seam end-to-end with zero filter math
    /// (Task 0). Grows `FilterOp::grown_bounds` by 0 pixels.
    // Constructed by the CI-visible `task0_ir_witnesses` tests below; there is no
    // production producer until Slice 1 wires a public painter API. So the variant
    // is genuinely unconstructed only in NON-test builds — scope the allow to those
    // (the lint stays live under `cfg(test)`, where the witnesses construct it).
    #[cfg_attr(not(test), allow(dead_code))]
    Identity,
    /// Morphological filter: separable H then V pass with the given radius and op.
    ///
    /// The H and V sub-passes are internal to `apply_morphology` — callers see a
    /// single `Morph` pass. `radius` is in physical pixels; `op` selects
    /// dilate/erode. Grows `FilterOp::grown_bounds` by `ceil(radius)` pixels on
    /// each side before clipping to the viewport.
    Morph {
        /// Kernel half-radius in physical pixels.
        radius: f32,
        /// Dilate (max) or erode (min).
        op: MorphOp,
    },
    /// Separable Gaussian blur: H pass (sigma_x) then V pass (sigma_y).
    ///
    /// The two sub-passes are internal to `apply_blur` — callers see a single
    /// `Blur` pass.  `sigma_x` / `sigma_y` are in physical pixels.
    ///
    /// ## PINNED #2 — premultiplied-direct, sRGB-encoded
    ///
    /// The Gaussian kernel operates on premultiplied RGBA in sRGB-encoded space.
    /// NO unpremultiply step, NO linearise.
    ///
    /// ## Kernel extent
    ///
    /// Half-radius = `ceil(sigma × √3)` per [`super::effects::kernel_radius`].
    /// Grows `FilterOp::grown_bounds` by `kernel_radius(max(sigma_x, sigma_y))`
    /// pixels on each side (conservative per-axis pad).
    Blur {
        /// Gaussian sigma for the horizontal sub-pass.
        sigma_x: f32,
        /// Gaussian sigma for the vertical sub-pass.
        sigma_y: f32,
    },
    /// 5×4 row-major color matrix applied per-pixel on un-premultiplied color.
    ///
    /// Bounds-PRESERVING: grows `FilterOp::grown_bounds` by **0** pixels.
    ///
    /// Reuses `apply_color_matrix` in `opacity_layer.rs` — the same function
    /// the `LayerFilter::ColorMatrix` fold arm uses.  The matrix is applied
    /// full-viewport (REPLACE semantics, `LoadOp::Clear(TRANSPARENT)`).
    ///
    /// Layout mirrors [`flui_types::painting::ColorMatrix::values`]:
    /// rows R/G/B/A × columns `[m0..m3, offset]`.
    ///
    /// ## Two-route rule
    ///
    /// A standalone `ImageFilter::Matrix`/`ColorAdjust` (not inside a `Compose`)
    /// still routes to the `LayerFilter::ColorMatrix` seam.  Only
    /// `ImageFilter::Compose` produces this variant — the flatten at record time
    /// (`flatten_compose` in `backend.rs`) promotes Matrix/ColorAdjust inside a
    /// Compose to `ImageFilterPass::ColorMatrix` so the ordered fold can interleave
    /// them with growing passes.  Both routes call `apply_color_matrix` underneath,
    /// so the pixel output is identical.
    ColorMatrix([f32; 20]),
}

/// A bounds-GROWING image-filter operation, isolated at record time.
///
/// `Clone` + GPU-resource-free (T11 purity witness): `input` is a `DrawSegment`
/// (already witnessed `Clone`), `passes` are POD, bounds are `Copy`. The repo
/// represents an owned GPU texture as `PooledTexture`, which is `!Clone` (it
/// reclaims its pool slot on `Drop`); adding such a field would break the
/// `const _FILTER_OP_IS_CLONE` witness — enforcing "acquire textures at replay,
/// never store them in the IR". (Raw `wgpu::Texture`/`TextureView` are `Clone`
/// in wgpu 29, so `Clone` alone does not bar them — the discipline is to use
/// `PooledTexture` for all owned GPU textures, which the witness then catches.)
///
/// Textures are acquired at REPLAY time (never held in the IR), matching the
/// discipline of `AdvancedShapeOp` and `SsaaPathOp`.
#[derive(Debug, Clone)]
pub(crate) struct FilterOp {
    /// Foreground content the filter consumes, rendered to an offscreen
    /// intermediate at replay time.
    pub(crate) input: DrawSegment,
    /// Flattened pass chain, applied left-to-right (index 0 = innermost pass).
    ///
    /// Inline capacity 4 covers realistic Compose depth
    /// (e.g. Blur∘Mode∘Morph∘Identity). Heap-spills beyond 4 are correct.
    pub(crate) passes: SmallVec<[ImageFilterPass; 4]>,
    /// Pre-filter content AABB in physical pixels (record-time geometry bound).
    pub(crate) content_bounds: Rect<Pixels>,
    /// `content_bounds` expanded by the accumulated pass radius, clipped to
    /// the layer bounds. For Task 0 this equals `content_bounds` (Identity
    /// grows bounds by 0 pixels). Slice 4 computes the real growth via
    /// `kernel_radius(sigma)`.
    pub(crate) grown_bounds: Rect<Pixels>,
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
    /// Color-filter chain applied to the rendered layer before compositing.
    ///
    /// Empty chain = premultiplied tint-only composite (the common fast-path).
    /// A non-empty chain is folded left-to-right in `flush_opacity_layer`:
    /// each filter reads the previous output and writes into a fresh pooled
    /// texture (ping-pong, ≤2 live textures regardless of chain length N).
    /// `LayerFilter::ColorMatrix(_)` routes through the color-matrix GPU pass.
    pub(crate) filters: LayerFilterChain,
    /// Image filter (bounds-GROWING) to apply via `DrawItem::Filter` instead of
    /// the normal `DrawItem::OpacityLayer` path.
    ///
    /// When `Some`, `restore_layer` emits a `DrawItem::Filter` carrying the
    /// isolated offscreen content and the pass chain derived from this spec.
    /// The `Reintegrate` fast-path is gated on `image_filter.is_none()` — a
    /// filter layer always routes through the offscreen composite path (G3).
    pub(crate) image_filter: Option<ImageFilterSpec>,
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
    /// A bounds-GROWING image filter over an isolated content segment.
    ///
    /// The content segment is rendered to a full-viewport pooled offscreen at
    /// replay time, the pass chain is applied (ping-pong, ≤2 live textures),
    /// and the filtered result is composited at `grown_bounds` via the existing
    /// premultiplied offscreen composite seam (`flush_texture_batch_premultiplied`).
    ///
    /// Z-order is the insertion position in `draw_order` (R1 arm order). This
    /// arm is placed LAST in `GpuReplay::submit` so all prior draw-order items
    /// are flushed to the target before the filter result is composited on top.
    // Constructed by the CI-visible `task0_ir_witnesses` tests below; there is no
    // production producer until Slice 1 wires a public painter API (e.g.
    // `push_image_filter`). The variant is genuinely unconstructed only in
    // NON-test builds, so scope the allow there (the lint stays live under
    // `cfg(test)`, where the witnesses construct + match it).
    #[cfg_attr(not(test), allow(dead_code))]
    Filter(FilterOp),
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
    /// Color-filter chain applied to the rendered layer before compositing.
    ///
    /// Forwarded from [`SavedLayer::filters`] at restore time. Empty chain =
    /// plain tint-only composite (the common fast-path). Folded left-to-right
    /// in `flush_opacity_layer` via ping-pong texture acquire/drop.
    pub(crate) filters: LayerFilterChain,
}

// ─── Task 0 IR-purity witnesses (CI-visible) ──────────────────────────────────
//
// These run in the standard CI `cargo nextest --lib` pass — a PLAIN `#[cfg(test)]`
// module in a non-feature-gated file (NOT under `feature = "enable-wgpu-tests"`),
// unlike the GPU readback A/B test. They:
//   1. construct `FilterOp` / `DrawItem::Filter` from CPU data only — exercising
//      the new variants under `cfg(test)` so `dead_code` stays satisfied there
//      (the `#[cfg_attr(not(test), allow(dead_code))]` on the variants covers only
//      the non-test build, where no production producer exists until Slice 1); and
//   2. assert the new IR is `Clone` + handle-free (T11 purity), so any future field
//      holding a live GPU handle fails to compile — guarding IR purity in CI.
#[cfg(test)]
mod task0_ir_witnesses {
    use flui_types::{Rect, geometry::px};
    use smallvec::smallvec;

    use flui_types::painting::BlendMode;

    use super::{
        DrawItem, DrawSegment, FilterOp, GammaDirection, ImageFilterPass, ImageFilterSpec,
        LayerFilter, LayerFilterChain, MorphOp,
    };

    /// Compile-time proof that `FilterOp` is `Clone`.
    ///
    /// Fields: `input: DrawSegment` (Clone-witnessed), `passes` (POD),
    /// `content_bounds`/`grown_bounds: Rect<Pixels>` (Copy). The guard catches a
    /// future `!Clone` field — notably `PooledTexture` (the repo's owned GPU-texture
    /// handle, `!Clone` by Drop-returns-to-pool). Storing one in the IR would make
    /// this fail to compile, enforcing "textures acquired at replay, never in the IR".
    const _FILTER_OP_IS_CLONE: fn(FilterOp) -> FilterOp = |op| op.clone();

    /// Compile-time proof that `ImageFilterPass` is `Clone` (Blur/Morph variants
    /// from later slices must keep deriving it; this catches a regression).
    const _IMAGE_FILTER_PASS_IS_CLONE: fn(ImageFilterPass) -> ImageFilterPass = |p| p.clone();

    fn identity_op() -> FilterOp {
        let bounds = Rect::from_ltrb(px(0.0), px(0.0), px(64.0), px(64.0));
        FilterOp {
            input: DrawSegment::new(),
            passes: smallvec![ImageFilterPass::Identity],
            content_bounds: bounds,
            grown_bounds: bounds,
        }
    }

    /// Runtime purity witness: a `FilterOp` is constructible + cloneable with no GPU
    /// context. Constructing the value also exercises the variant under `cfg(test)`.
    #[test]
    fn filter_op_is_pure_cpu_data() {
        let op = identity_op();
        let cloned = op.clone();
        assert_eq!(cloned.passes.len(), 1);
    }

    /// The `DrawItem::Filter` variant is constructable + pattern-matchable in plain
    /// CPU code (no GPU feature) — the CI-visible construction that keeps the
    /// variant non-dead under `cfg(test)`.
    #[test]
    fn draw_item_filter_variant_is_reachable() {
        match DrawItem::Filter(identity_op()) {
            DrawItem::Filter(inner) => {
                assert_eq!(inner.passes.len(), 1);
                assert!(matches!(inner.passes[0], ImageFilterPass::Identity));
            }
            _ => panic!("constructed DrawItem::Filter must match its own variant"),
        }
    }

    /// `LayerFilterChain::default()` is empty — the no-filter fast-path state.
    #[test]
    fn layer_filter_chain_default_is_empty() {
        assert!(LayerFilterChain::new().is_empty());
    }

    /// `MorphOp` is `Copy`/`Clone`/`PartialEq`/`Debug` — exercised here so it is
    /// never dead under `cfg(test)`.
    #[test]
    fn morph_op_is_copy_and_clone() {
        let dilate = MorphOp::Dilate;
        let erode = MorphOp::Erode;
        assert_ne!(dilate, erode);
        assert_eq!(dilate, dilate.clone());
    }

    /// `ImageFilterSpec::Morph` and `ImageFilterPass::Morph` are constructable and
    /// comparable — exercises both under `cfg(test)`.
    #[test]
    fn morph_ir_variants_are_constructable() {
        let spec = ImageFilterSpec::Morph {
            radius: 3.0,
            op: MorphOp::Dilate,
        };
        assert!(matches!(spec, ImageFilterSpec::Morph { .. }));

        let pass = ImageFilterPass::Morph {
            radius: 3.0,
            op: MorphOp::Dilate,
        };
        assert!(matches!(pass, ImageFilterPass::Morph { .. }));
    }

    /// `LayerFilter::Mode` is constructable + pattern-matchable without GPU context.
    ///
    /// Exercises the `Mode` variant under `cfg(test)` — the
    /// `#[cfg_attr(not(test), allow(dead_code))]` on the variant is satisfied by
    /// this construction, keeping the lint live in test builds.
    #[test]
    fn layer_filter_mode_variant_is_constructable() {
        let filter = LayerFilter::Mode {
            color: [1.0, 0.0, 0.0, 1.0],
            blend_mode: BlendMode::Multiply,
        };
        assert!(matches!(filter, LayerFilter::Mode { .. }));
    }

    /// `LayerFilter::Gamma` is constructable + pattern-matchable without GPU context.
    ///
    /// Same dead_code rationale as `Mode` above.
    #[test]
    fn layer_filter_gamma_variant_is_constructable() {
        let srgb_to_lin = LayerFilter::Gamma(GammaDirection::SrgbToLinear);
        let lin_to_srgb = LayerFilter::Gamma(GammaDirection::LinearToSrgb);
        assert!(matches!(
            srgb_to_lin,
            LayerFilter::Gamma(GammaDirection::SrgbToLinear)
        ));
        assert!(matches!(
            lin_to_srgb,
            LayerFilter::Gamma(GammaDirection::LinearToSrgb)
        ));
        // GammaDirection is Copy + PartialEq.
        assert_ne!(GammaDirection::SrgbToLinear, GammaDirection::LinearToSrgb);
    }

    /// `FilterOp` carrying a `Morph` pass is still `Clone` (T11 purity witness).
    #[test]
    fn filter_op_with_morph_pass_is_pure_cpu_data() {
        let bounds = Rect::from_ltrb(px(0.0), px(0.0), px(64.0), px(64.0));
        let op = FilterOp {
            input: DrawSegment::new(),
            passes: smallvec![ImageFilterPass::Morph {
                radius: 4.0,
                op: MorphOp::Erode,
            }],
            content_bounds: bounds,
            grown_bounds: bounds,
        };
        let cloned = op.clone();
        assert_eq!(cloned.passes.len(), 1);
        assert!(matches!(
            cloned.passes[0],
            ImageFilterPass::Morph {
                radius: _,
                op: MorphOp::Erode
            }
        ));
    }

    /// `ImageFilterSpec::Blur` and `ImageFilterPass::Blur` are constructable and
    /// comparable — exercises both under `cfg(test)` so the variants are not
    /// dead code in the test cfg.
    #[test]
    fn blur_ir_variants_are_constructable() {
        let spec = ImageFilterSpec::Blur {
            sigma_x: 4.0,
            sigma_y: 2.0,
        };
        assert!(matches!(spec, ImageFilterSpec::Blur { .. }));

        let pass = ImageFilterPass::Blur {
            sigma_x: 4.0,
            sigma_y: 2.0,
        };
        assert!(matches!(pass, ImageFilterPass::Blur { .. }));
    }

    /// `FilterOp` carrying a `Blur` pass is still `Clone` (T11 purity witness).
    #[test]
    fn filter_op_with_blur_pass_is_pure_cpu_data() {
        let bounds = Rect::from_ltrb(px(0.0), px(0.0), px(64.0), px(64.0));
        let op = FilterOp {
            input: DrawSegment::new(),
            passes: smallvec![ImageFilterPass::Blur {
                sigma_x: 4.0,
                sigma_y: 2.0,
            }],
            content_bounds: bounds,
            grown_bounds: bounds,
        };
        let cloned = op.clone();
        assert_eq!(cloned.passes.len(), 1);
        assert!(matches!(
            cloned.passes[0],
            ImageFilterPass::Blur {
                sigma_x: _,
                sigma_y: _
            }
        ));
    }
}

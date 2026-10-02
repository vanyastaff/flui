//! Command IR data types for the `WgpuPainter` draw-record pipeline.
//!
//! This module owns the type definitions for the per-frame record-side IR:
//! the batching/segment/layer types that accumulate draw commands before
//! the painter flushes them to wgpu.
//!
//! **Scope:** pure type definitions + their inherent impls.  The painter's
//! record methods (rect/rrect/circle/…) and flush/replay methods remain in
//! `painter`.  These types are re-exported `pub(crate)` so `painter`
//! and future batcher/compositor modules can import from one place.

use flui_foundation::geometry::Rect;
use flui_painting::paint::BlendMode;
use smallvec::SmallVec;

use crate::{
    effects::GradientStop,
    instancing::{
        ArcInstance, CircleInstance, GlyphInstance, InstanceBatch, LinearGradientInstance,
        RadialGradientInstance, RectInstance, ShadowInstance, SweepGradientInstance,
        TextureInstance,
    },
    pipeline_cache::PipelineKey,
    texture_cache::TextureKey,
    vertex::Vertex,
};

// ─── Layer filter ────────────────────────────────────────────────────────────

/// Direction of the sRGB gamma transfer function.
///
/// Used by [`LayerFilter::Gamma`] to select which transfer direction the GPU
/// shader applies per RGB channel (alpha is always passed through unchanged).
///
/// The underlying transfer functions are the `pub` helpers
/// [`flui_painting::styling::color::srgb_to_linear`] and
/// [`flui_painting::styling::color::linear_to_srgb`] — the same functions used by
/// the CPU oracle in the GPU readback tests, ensuring one authoritative home for
/// the IEC 61966-2-1 piecewise formula.
///
/// Constructed from the production `push_color_filter` path via
/// `ColorFilter::LinearToSrgbGamma` / `ColorFilter::SrgbToLinearGamma`.
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
    /// Layout mirrors [`flui_painting::paint::ColorMatrix::values`]:
    /// rows R/G/B/A × columns `[m0..m3, offset]`.
    ColorMatrix([f32; 20]),

    /// Porter-Duff / W3C blend of a solid filter color over each layer pixel,
    /// applied in straight sRGB space.
    ///
    /// `color` is the **filter** color in straight sRGB `[f32; 4]` (pre-converted
    /// from [`flui_painting::styling::Color`] via [`flui_painting::styling::Color::to_f32_array`] at the
    /// call site).  The GPU shader unpremultiplies the layer pixel, computes
    /// `blend(src=color, dst=straight_pixel, mode)`, clamps to `[0, 1]`, and
    /// repremultiplies.
    ///
    /// Mirrors [`flui_painting::styling::Color::blend`] (`self` = filter color = src,
    /// `dst` = layer pixel): the CPU oracle for the GPU readback tests.
    ///
    /// Constructed from the production `push_color_filter` path via
    /// `ColorFilter::Mode { color, blend_mode }`.
    Mode {
        /// Filter color in straight sRGB `[r, g, b, a]` (values in `[0, 1]`).
        color: [f32; 4],
        /// Blend mode — selects the Porter-Duff or W3C blend function.
        blend_mode: flui_painting::paint::BlendMode,
    },

    /// Per-channel sRGB ↔ linear-light transfer function, applied per RGB channel
    /// with alpha untouched.
    ///
    /// The direction is selected by [`GammaDirection`]; the underlying formula is
    /// the IEC 61966-2-1 piecewise function implemented in
    /// [`flui_painting::styling::color::srgb_to_linear`] /
    /// [`flui_painting::styling::color::linear_to_srgb`].
    ///
    /// The GPU shader unpremultiplies, applies the transfer per R/G/B, clamps to
    /// `[0, 1]`, and repremultiplies; alpha is left unchanged.
    ///
    /// Constructed from the production `push_color_filter` path via
    /// `ColorFilter::LinearToSrgbGamma` / `ColorFilter::SrgbToLinearGamma`.
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
    /// Half-radius = `ceil(sigma × √3)` per [`crate::effects::kernel_radius`].
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
    /// the full chain.  The `cumulative_growth` helper in the `painter` module sums the
    /// per-pass radius contributions to produce the expanded `grown_bounds`.
    ///
    /// Inline capacity 4 covers realistic `Compose` depth; heap-spills beyond 4
    /// are correct.
    Chain(SmallVec<[ImageFilterPass; 4]>),
}

/// A lowered, flattened image-filter pass.
///
/// Passes are either bounds-GROWING (Morph, Blur) or bounds-PRESERVING
/// (ColorMatrix, Identity).  The `cumulative_growth` helper in the `painter` module
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
    /// and grows `FilterOp::grown_bounds` by 0 pixels.
    // No production producer until a public painter API is wired; only test
    // builds construct it.
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
    /// Half-radius = `ceil(sigma × √3)` per [`crate::effects::kernel_radius`].
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
    /// Layout mirrors [`flui_painting::paint::ColorMatrix::values`]:
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
/// `Clone`: `input` is a `DrawSegment`, `passes` are POD, bounds are `Copy`. The repo
/// represents an owned GPU texture as `PooledTexture`, which is `!Clone` (it
/// reclaims its pool slot on `Drop`); adding such a field would break the
/// `const _FILTER_OP_IS_CLONE` witness — enforcing "acquire textures at replay,
/// never store them in the IR". (Raw `wgpu::Texture`/`TextureView` are `Clone`
/// in wgpu 30, so `Clone` alone does not bar them — the discipline is to use
/// `PooledTexture` for all owned GPU textures, which the witness then catches.)
///
/// Textures are acquired at REPLAY time (never held in the IR), matching the
/// discipline of `AdvancedShapeOp` and `SsaaPathOp`.
///
/// ## Integer-grid composite
///
/// The intermediate offscreen is sized to the integer-aligned bounding box of
/// `grown_bounds` (floor origin, ceil far corner, clamped to viewport) rather
/// than the full viewport. This reduces VRAM peak from `vp_area` to `grown_area`
/// × nesting depth.
///
/// The integer alignment is REQUIRED because the texture-batch composite sampler
/// is bilinear (`default_sampler` Linear in `replay`): compositing a
/// full-viewport intermediate at a fractional `grown_bounds` is self-consistent
/// (texel grid == device-pixel grid → aligned blit), but compositing an
/// integer-origin `fb`-sized intermediate at a fractional dst_rect offsets the
/// two grids by `frac(grown_left)`, shifting every pixel by a sub-texel.
///
/// `fb_origin` and `fb_dim` are computed at **record time** in `painter::layer`'s
/// `restore_layer` and stored here so BOTH composite arms (top-level in
/// `replay` + nested in `opacity_layer.rs`) re-read ONE source — eliminating
/// drift between the two composite arms.
pub(crate) struct FilterOp {
    pub(crate) composite_clip: Option<GroupClip>,
    /// Foreground content the filter consumes, rendered to an offscreen
    /// intermediate at replay time.
    pub(crate) input: SealedSegment,
    /// Ordered draws flushed before the final input segment.
    pub(crate) items: Vec<DrawItem>,
    /// Flattened pass chain, applied left-to-right (index 0 = innermost pass).
    ///
    /// Inline capacity 4 covers realistic Compose depth
    /// (e.g. Blur∘Mode∘Morph∘Identity). Heap-spills beyond 4 are correct.
    pub(crate) passes: SmallVec<[ImageFilterPass; 4]>,
    /// Pre-filter content AABB in physical pixels (record-time geometry bound).
    pub(crate) content_bounds: Rect<f64>,
    /// `content_bounds` expanded by the accumulated pass radius, clipped to
    /// the layer bounds. For Identity this equals `content_bounds` because the
    /// pass grows bounds by 0 pixels. Growing filters compute their pad via
    /// `kernel_radius(sigma)`.
    ///
    /// The composite uses `fb_origin`/`fb_dim` (integer-aligned) rather than
    /// `grown_bounds` directly. `grown_bounds` is
    /// retained for diagnostics, tracing, and future tooling (e.g. damage-region
    /// tracking or spec-verify audits that check halo extent in floating-point).
    // Retained for diagnostics: the composite arms now use fb_origin/fb_dim but
    // grown_bounds documents the fractional halo extent pre-quantisation and will
    // be needed by damage-tracking or future floating-point halo assertions.
    #[expect(dead_code)]
    pub(crate) grown_bounds: Rect<f64>,
    /// Integer-grid top-left of the offscreen intermediate in device pixels.
    ///
    /// Computed as `(floor(grown_bounds.left), floor(grown_bounds.top))`.
    /// Integer-aligned so the bilinear composite produces an aligned texel blit
    /// (no sub-pixel shift). Stored on the IR so both composite arms share one
    /// authoritative value.
    pub(crate) fb_origin: (u32, u32),
    /// Integer-aligned dimensions of the offscreen intermediate in device pixels.
    ///
    /// Computed as `(ceil(far.x) - fb_origin.x, ceil(far.y) - fb_origin.y)`,
    /// clamped so `fb_origin + fb_dim ≤ viewport`. This is the exact size passed
    /// to `pool.acquire` and used as `texture_size` in all filter sub-passes.
    pub(crate) fb_dim: (u32, u32),
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

/// Tracks a sub-range of gradient instances that share both a scissor state and
/// a blend mode.
///
/// A gradient carries its paint's blend mode all the way to the GPU, and the
/// blend mode is pipeline state, so a run ends when EITHER changes. The
/// non-gradient instanced batches keep [`ScissorRegion`]: their pipelines are
/// fixed `ALPHA_BLENDING` and a blend mode on their runs would be a field
/// nothing reads, claiming a capability those pipelines do not have.
#[derive(Debug, Clone)]
pub(crate) struct GradientRun {
    pub(crate) scissor: ScissorRect,
    /// The fixed-function blend mode this run's pipeline is keyed by.
    ///
    /// Never advanced: `dispatch_shader_rect` diverts those into
    /// `DrawItem::AdvancedShape` before a run is recorded.
    pub(crate) blend: BlendMode,
    pub(crate) start: u32,
    pub(crate) count: u32,
}

/// The per-batch SDF clip, in the layout `shape.wgsl`'s `ClipUniform` expects.
///
/// `vec4` members because WGSL uniform layout aligns them to 16 bytes; the
/// three arrays are the same values `TessellatedBatch` stores, regrouped.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ClipUniform {
    /// Clip-local `[x, y, w, h]`.
    pub(crate) bounds: [f32; 4],
    /// `[tl, tr, br, bl]`, in the same space as `bounds`.
    pub(crate) radii: [f32; 4],
    /// `[kind, _, _, _]`: 0 = none, 1 = rrect, 2 = rounded superellipse.
    pub(crate) kind: [u32; 4],
    /// Device-to-clip-local linear part `[a, b, c, d]`, columns first.
    pub(crate) device_to_local: [f32; 4],
    /// Device-to-clip-local translation `[tx, ty, 0, 0]`.
    pub(crate) local_origin: [f32; 4],
}

impl ClipUniform {
    /// Regroup a [`ResolvedClip`](crate::state_stack::ResolvedClip) into the
    /// `vec4` members WGSL uniform layout wants.
    pub(crate) fn from_resolved(clip: crate::state_stack::ResolvedClip) -> Self {
        let m = clip.device_to_local;
        Self {
            bounds: [clip.rrect[0], clip.rrect[1], clip.rrect[2], clip.rrect[3]],
            radii: [clip.rrect[4], clip.rrect[5], clip.rrect[6], clip.rrect[7]],
            kind: clip.kind,
            device_to_local: [m[0], m[1], m[2], m[3]],
            local_origin: [m[4], m[5], 0.0, 0.0],
        }
    }
}

/// A recorded batch of tessellated geometry sharing the same pipeline key.
///
/// During a frame, each call to
/// [`crate::batches::DrawBatcher::add_tessellated_with_key`] appends
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
    /// The SDF clip active when this batch was recorded.
    ///
    /// Tessellated geometry carries no per-instance slot — there are no
    /// instances, only vertices — so the clip lives on the batch and is bound
    /// as a uniform for its draw. The batch is the right granularity because
    /// `add_tessellated_with_key` only merges into the previous batch when the
    /// pipeline key AND the clip both match.
    pub(crate) clip: crate::state_stack::ResolvedClip,
}

// ─── Offscreen / layer snapshots ─────────────────────────────────────────────

/// Saved render state for `save_layer`/`restore_layer` offscreen compositing.
///
/// When `save_layer` is called, the current draw state is captured into this
/// struct and a fresh segment begins. All subsequent drawing goes into the new
/// segment. On `restore_layer`, the offscreen content is composited back onto
/// the parent surface with the layer's opacity applied as a group.
// Saved draw-order owns nested effect payloads.
pub(crate) struct SavedLayer {
    pub(crate) force_isolation: bool,
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
    /// The layer's region in device space `[x, y, w, h]`: its bounds mapped
    /// through the transform and already cut by the clip, which a composite
    /// changes in full (mapping decision 19). `None` falls back to the full
    /// viewport, which only filter layers ask for.
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
    /// The clip a layer applies to its COMPOSITE rather than to the draws
    /// inside it: a `Clip::AntiAliasWithSaveLayer` layer's own clip, or, for a
    /// layer whose mode replaces the destination, the ambient rounded clip or
    /// its rotated bounds as a hard clip, without which the composite would
    /// change pixels outside its region.
    ///
    /// `Some` marks the layer as one that must composite, and forces the
    /// composite path: for a clip-opened layer the whole point of the mode is
    /// that the offscreen exists, so the `Reintegrate` fast-path — which
    /// splices the children straight back into the parent draw order — must
    /// not swallow it.
    ///
    /// `Some(ResolvedClip::NONE)` is a real value, not an empty one: a
    /// rectangular clip is the hardware scissor, which already applied to every
    /// draw inside the offscreen and is binary, so the composite carries no SDF
    /// of its own. `None` means the composite needs no clip beyond its region.
    pub(crate) composite_clip: Option<GroupClip>,
}

// ─── Draw segment ─────────────────────────────────────────────────────────────

/// A segment of draw commands that share the same rendering phase ordering.
///
/// When an offscreen texture is queued, the current segment is finalized and
/// a new one starts. This ensures that content drawn before the offscreen
/// texture renders before it, and content drawn after renders after it,
/// preserving correct Z-order.
///
/// `Clone` is derived because every field is plain CPU data. What the derive
/// enforces is narrower than "no GPU handle": raw `wgpu::Texture` /
/// `TextureView` / `Buffer` are `Clone` in wgpu 30 (ref-counted handles), so
/// only a `PooledTexture` — this crate's owned texture, `!Clone` because it
/// returns its slot on `Drop` — is barred by the derive. Textures are acquired
/// at replay, never stored here. No test checks at run time that replay
/// leaves the IR unchanged.
#[derive(Debug, Clone)]
pub(crate) struct DrawSegment {
    pub(crate) budget: std::sync::Arc<crate::recording_budget::RecordingBudget>,
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
    pub(crate) current_gradient_stops: crate::recording_budget::BudgetVec<GradientStop>,
    /// Batched vertices for tessellation path
    pub(crate) vertices: crate::recording_budget::BudgetVec<Vertex>,
    /// Batched indices for tessellation path
    pub(crate) indices: crate::recording_budget::BudgetVec<u32>,
    /// Recorded tessellated batches for this segment
    pub(crate) tess_batches: crate::recording_budget::BudgetVec<TessellatedBatch>,
    /// Glyph quads, sampled from the painter's glyph atlas. Text is a batch
    /// like any other, so it takes its place in the segment's phase order
    /// and in every offscreen path a segment can replay through.
    pub(crate) glyph_batch: InstanceBatch<GlyphInstance>,
    /// Scissor regions for the glyph batch.
    pub(crate) glyph_scissors: crate::recording_budget::BudgetVec<ScissorRegion>,
    /// Current pipeline key (for batching draws with same pipeline)
    pub(crate) current_pipeline_key: Option<PipelineKey>,
    /// Scissor regions for rect instanced batch
    pub(crate) rect_scissors: crate::recording_budget::BudgetVec<ScissorRegion>,
    /// Scissor regions for circle instanced batch
    pub(crate) circle_scissors: crate::recording_budget::BudgetVec<ScissorRegion>,
    /// Scissor regions for arc instanced batch
    pub(crate) arc_scissors: crate::recording_budget::BudgetVec<ScissorRegion>,
    /// Draw runs for the linear gradient batch, split by scissor and blend mode.
    pub(crate) linear_gradient_runs: crate::recording_budget::BudgetVec<GradientRun>,
    /// Draw runs for the radial gradient batch, split by scissor and blend mode.
    pub(crate) radial_gradient_runs: crate::recording_budget::BudgetVec<GradientRun>,
    /// Draw runs for the sweep gradient batch, split by scissor and blend mode.
    pub(crate) sweep_gradient_runs: crate::recording_budget::BudgetVec<GradientRun>,
    /// Cached image draws queued for this segment.
    ///
    /// The third element is the scissor rect active at draw time, forwarded to
    /// `flush_texture_batch` so clipped images don't spill outside their clip region.
    pub(crate) cached_images:
        crate::recording_budget::BudgetVec<(TextureKey, TextureInstance, ScissorRect)>,
    /// Allocation, alpha and sampling are frozen at recording. Texel contents
    /// remain live: writes into the same allocation are visible at submission.
    pub(crate) external_images: crate::recording_budget::BudgetVec<(
        crate::external_texture_registry::ExternalAllocationLease,
        TextureInstance,
        ScissorRect,
        crate::external_texture_registry::ExternalSampling,
    )>,

    /// Actual attachment samples map back to immutable root/device clip coordinates.
    pub(crate) attachment_to_root: [f64; 6],
    pub(crate) runs: crate::recording_budget::BudgetVec<RecordedRun>,
    pub(crate) record_error: Option<RecordError>,
}

#[derive(Debug, Clone)]
pub(crate) enum RecordError {
    External(crate::error::ExternalTextureError),
    Geometry(crate::error::GeometryError),
    Backdrop(&'static str),
    Limit {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
}

/// Ownership handoff from mutable recording to immutable replay.
#[derive(Debug, Clone)]
pub(crate) struct SealedSegment(DrawSegment);
impl std::ops::Deref for SealedSegment {
    type Target = DrawSegment;
    fn deref(&self) -> &DrawSegment {
        &self.0
    }
}

/// Geometric coverage owned by an effect/group composite, never by its children.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupClip {
    pub(crate) legacy: crate::state_stack::ResolvedClip,
    pub(crate) chain: crate::clip_chain::ClipChain,
}

/// A compatible ordered run owns the exact clip snapshot active at recording.
#[derive(Debug, Clone)]
pub(crate) struct RecordedRun {
    pub(crate) kind: DrawRun,
    pub(crate) clip: crate::clip_chain::ClipChain,
}

/// Painter-order typed ranges, each indexing only its corresponding arena.
#[derive(Debug, Clone)]
pub(crate) enum DrawRun {
    Rect(std::ops::Range<usize>),
    Circle(std::ops::Range<usize>),
    Arc(std::ops::Range<usize>),
    Shadow(std::ops::Range<usize>),
    LinearGradient(std::ops::Range<usize>),
    RadialGradient(std::ops::Range<usize>),
    SweepGradient(std::ops::Range<usize>),
    Tess(std::ops::Range<usize>),
    CachedImage(std::ops::Range<usize>),
    ExternalImage(std::ops::Range<usize>),
    Glyph(std::ops::Range<usize>),
}

impl DrawSegment {
    /// Create an empty draw segment with pre-allocated batch capacities.
    pub(crate) fn new() -> Self {
        Self::with_budget(crate::recording_budget::RecordingBudget::default_frame())
    }
    pub(crate) fn empty_sibling(&self) -> Self {
        Self::with_budget(std::sync::Arc::clone(&self.budget))
    }
    pub(crate) fn with_budget(
        budget: std::sync::Arc<crate::recording_budget::RecordingBudget>,
    ) -> Self {
        let mut segment = Self {
            budget,
            rect_batch: InstanceBatch::new(0),
            circle_batch: InstanceBatch::new(0),
            arc_batch: InstanceBatch::new(0),
            shadow_batch: InstanceBatch::new(0),
            linear_gradient_batch: InstanceBatch::new(0),
            radial_gradient_batch: InstanceBatch::new(0),
            sweep_gradient_batch: InstanceBatch::new(0),
            current_gradient_stops: crate::recording_budget::BudgetVec::new(),
            vertices: crate::recording_budget::BudgetVec::new(),
            indices: crate::recording_budget::BudgetVec::new(),
            tess_batches: crate::recording_budget::BudgetVec::new(),
            current_pipeline_key: None,
            rect_scissors: crate::recording_budget::BudgetVec::new(),
            circle_scissors: crate::recording_budget::BudgetVec::new(),
            arc_scissors: crate::recording_budget::BudgetVec::new(),
            linear_gradient_runs: crate::recording_budget::BudgetVec::new(),
            radial_gradient_runs: crate::recording_budget::BudgetVec::new(),
            sweep_gradient_runs: crate::recording_budget::BudgetVec::new(),
            cached_images: crate::recording_budget::BudgetVec::new(),
            external_images: crate::recording_budget::BudgetVec::new(),
            glyph_batch: InstanceBatch::new(0),
            glyph_scissors: crate::recording_budget::BudgetVec::new(),
            attachment_to_root: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            runs: crate::recording_budget::BudgetVec::new(),
            record_error: None,
        };
        segment.attach_budget();
        segment
    }
    fn attach_budget(&mut self) {
        self.current_gradient_stops = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.vertices = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.indices = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.tess_batches = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.glyph_scissors = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.rect_scissors = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.circle_scissors = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.arc_scissors = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.linear_gradient_runs = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.radial_gradient_runs = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.sweep_gradient_runs = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.cached_images = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.external_images = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.runs = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.rect_batch.instances = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.circle_batch.instances = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.arc_batch.instances = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.shadow_batch.instances = crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.linear_gradient_batch.instances =
            crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.radial_gradient_batch.instances =
            crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.sweep_gradient_batch.instances =
            crate::recording_budget::BudgetVec::with_budget(&self.budget);
        self.glyph_batch.instances = crate::recording_budget::BudgetVec::with_budget(&self.budget);
    }
    pub(crate) fn rebase_attachment(&mut self, x: f64, y: f64, sx: f64, sy: f64) {
        let [xx, yx, xy, yy, tx, ty] = self.attachment_to_root;
        self.attachment_to_root = [
            xx * sx,
            yx * sx,
            xy * sy,
            yy * sy,
            xx * x + xy * y + tx,
            yx * x + yy * y + ty,
        ];
    }
    pub(crate) fn recording_result(&self) -> crate::error::EngineResult<()> {
        match self.record_error.clone().or_else(|| self.budget.error()) {
            Some(RecordError::External(error)) => Err(error.into()),
            Some(RecordError::Geometry(error)) => Err(error.into()),
            Some(RecordError::Backdrop(reason)) => {
                Err(crate::error::EngineError::UnsupportedBackdropFilter { reason })
            }
            Some(RecordError::Limit {
                resource,
                requested,
                limit,
            }) => Err(crate::error::EngineError::PreparedResourceLimit {
                resource,
                requested,
                limit,
            }),
            None => Ok(()),
        }
    }
    pub(crate) fn record_external_error(&mut self, error: crate::error::ExternalTextureError) {
        self.budget.record_error(RecordError::External(error));
        if self.record_error.is_none() {
            self.record_error = self.budget.error();
        }
    }
    pub(crate) fn try_clone_for_remap(&self) -> crate::error::EngineResult<Self> {
        self.recording_result()?;
        let cloned = self.clone();
        cloned.recording_result()?;
        Ok(cloned)
    }
    pub(crate) fn seal(mut self) -> SealedSegment {
        if self.record_error.is_none() {
            self.record_error = self.budget.error();
        }
        SealedSegment(self)
    }

    pub(crate) fn record_run(&mut self, run: DrawRun, clip: crate::clip_chain::ClipChain) {
        if self.budget.error().is_some() {
            return;
        }
        use DrawRun::{
            Arc, CachedImage, Circle, ExternalImage, Glyph, LinearGradient, RadialGradient, Rect,
            Shadow, SweepGradient, Tess,
        };
        let previous = self
            .runs
            .last_mut()
            .filter(|previous| previous.clip == clip);
        let merge = match (previous.map(|previous| &mut previous.kind), &run) {
            (Some(Rect(a)), Rect(b))
            | (Some(Circle(a)), Circle(b))
            | (Some(Arc(a)), Arc(b))
            | (Some(Shadow(a)), Shadow(b))
            | (Some(LinearGradient(a)), LinearGradient(b))
            | (Some(RadialGradient(a)), RadialGradient(b))
            | (Some(SweepGradient(a)), SweepGradient(b))
            | (Some(Tess(a)), Tess(b))
            | (Some(CachedImage(a)), CachedImage(b))
            | (Some(ExternalImage(a)), ExternalImage(b))
            | (Some(Glyph(a)), Glyph(b))
                if a.end == b.start =>
            {
                a.end = b.end;
                true
            }
            _ => false,
        };
        if !merge {
            self.runs.push(RecordedRun { kind: run, clip });
        }
    }

    pub(crate) fn record_limit(&mut self, resource: &'static str, requested: usize, limit: usize) {
        self.budget.record_error(RecordError::Limit {
            resource,
            requested,
            limit,
        });
        if self.record_error.is_none() {
            self.record_error = self.budget.error();
        }
    }

    /// Record an instance addition for a given scissor region tracker.
    /// Extends the last region if the scissor matches, or creates a new one.
    pub(crate) fn push_scissor_region(
        regions: &mut crate::recording_budget::BudgetVec<ScissorRegion>,
        scissor: ScissorRect,
    ) {
        if regions.failed() {
            return;
        }
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

    /// Record a gradient instance addition against its run tracker.
    ///
    /// Extends the last run when both the scissor AND the blend mode match, and
    /// starts a new one otherwise. Same rule as [`Self::push_scissor_region`]
    /// with one more component in the key, because a run is exactly the span of
    /// instances one `set_scissor_rect` + `set_pipeline` pair can draw.
    pub(crate) fn push_gradient_run(
        runs: &mut crate::recording_budget::BudgetVec<GradientRun>,
        scissor: ScissorRect,
        blend: BlendMode,
    ) {
        if runs.failed() {
            return;
        }
        if let Some(last) = runs.last_mut()
            && last.scissor == scissor
            && last.blend == blend
        {
            last.count += 1;
            return;
        }
        runs.push(GradientRun {
            scissor,
            blend,
            start: runs.last().map_or(0, |r| r.start + r.count),
            count: 1,
        });
    }

    /// Whether this segment records nothing at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.record_error.is_none()
            && self.budget.error().is_none()
            && self.rect_batch.is_empty()
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
            && self.glyph_batch.is_empty()
    }
}

impl Default for DrawSegment {
    fn default() -> Self {
        Self::with_budget(crate::recording_budget::RecordingBudget::default_frame())
    }
}

/// A single tessellated shape that requires a dst-read (advanced) blend.
///
/// Created by [`crate::batches::DrawBatcher::add_tessellated_with_key`] when
/// the pipeline key carries an advanced (W3C composite) blend mode.  Instead
/// of batching the shape into the current `DrawSegment` (which would use
/// fixed-function blending), the shape's geometry is isolated here and rendered
/// offscreen at replay time so `flush_advanced_layer` can read the backdrop and
/// compute the correct non-separable blend.
///
/// ## Record-side contract
///
/// `AdvancedShapeOp` derives `Clone` like [`DrawSegment`]: it holds CPU data
/// only, and a `PooledTexture` field (the one `!Clone` texture type here)
/// cannot be added without the derive failing. See `DrawSegment`'s doc for
/// what that does and does not prove.
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
    pub(crate) segment: SealedSegment,
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
    pub(crate) device_bounds: Rect<f64>,
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
/// ## Record-side contract
///
/// `SsaaPathOp` derives `Clone` like [`AdvancedShapeOp`]: CPU data only
/// (`BlendMode` is `Copy`, the embedded [`DrawSegment`] is plain data). All
/// GPU work happens at replay time in `GpuReplay::submit`.
#[derive(Clone)]
pub(crate) struct SsaaPathOp {
    /// Tessellated geometry for this path (vertices already baked to device
    /// space; indices relative to a fresh segment starting at base 0).
    pub(crate) segment: SealedSegment,
    /// Device-space AABB of the path's coverage in device pixels.
    ///
    /// Used to size the SSAA tile: `ceil(device_bounds.width) × ceil(device_bounds.height)`,
    /// clamped to `[1, viewport]`.  Computed as the AABB of
    /// `segment.vertices[*].position` at record time.
    pub(crate) device_bounds: Rect<f64>,
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
pub(crate) enum DrawItem {
    /// An ordered read/filter/write of the current attachment, before subsequent children.
    Backdrop(BackdropOp),
    /// A segment of instanced/tessellated/gradient draw commands.
    Segment(SealedSegment),
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
    // No production producer until a public painter API (for example,
    // `push_image_filter`) is wired; only test builds construct it.
    #[cfg_attr(not(test), allow(dead_code))]
    Filter(FilterOp),
}

/// Backdrop input is independent of the output clip and damage write region.
pub(crate) struct BackdropOp {
    pub(crate) context: SealedSegment,
    pub(crate) clip: GroupClip,
    pub(crate) scissor: ScissorRect,
    pub(crate) output_bounds: Rect<f64>,
    pub(crate) sigma: [f32; 2],
    pub(crate) blend: BlendMode,
}

impl Drop for BackdropOp {
    fn drop(&mut self) {
        self.context.budget.release(std::mem::size_of::<Self>(), 1);
    }
}

// ─── Opacity layer ────────────────────────────────────────────────────────────

/// A pending opacity layer waiting to be rendered offscreen and composited.
///
/// Created by [`crate::painter::WgpuPainter::restore_layer`] when opacity < 1.0.
/// During [`crate::painter::WgpuPainter::render`], the contained segments are
/// flushed to a pooled offscreen texture, then that texture is composited onto
/// the main surface with the layer opacity applied as tint alpha.
// Nested draw payloads are not debug-derived.
pub(crate) struct PendingOpacityLayer {
    /// Draw items accumulated between save_layer and restore_layer
    pub(crate) items: Vec<DrawItem>,
    /// Final segment at the time of restore_layer (may have content)
    pub(crate) final_segment: SealedSegment,
    /// Group opacity to apply during compositing (0.0–1.0)
    pub(crate) opacity: f32,
    /// Per-channel chroma tint for ColorFilter layers; white for plain opacity.
    /// See [`SavedLayer::layer_tint_rgb`].
    pub(crate) tint_rgb: [f32; 3],
    /// The layer's region in device coordinates, already cut by the clip:
    /// the composite quad, every pixel of which the composite writes with
    /// [`Self::blend`].
    pub(crate) bounds: Rect<f64>,
    /// Blend mode to apply when compositing this layer onto its parent.
    ///
    /// Stored on the pending layer so the flush path can read it without
    /// coupling it to the record path.  `SrcOver` for plain opacity layers;
    /// the paint's mode, Porter-Duff or advanced, for `saveLayer` with an
    /// explicit one.
    pub(crate) blend: BlendMode,
    /// Color-filter chain applied to the rendered layer before compositing.
    ///
    /// Forwarded from [`SavedLayer::filters`] at restore time. Empty chain =
    /// plain tint-only composite (the common fast-path). Folded left-to-right
    /// in `flush_opacity_layer` via ping-pong texture acquire/drop.
    pub(crate) filters: LayerFilterChain,
    /// The SDF clip applied to this layer's COMPOSITE: a
    /// `Clip::AntiAliasWithSaveLayer` layer's clip, or the clip a
    /// destination-replacing layer's region needs beyond its rectangle.
    ///
    /// Forwarded from [`SavedLayer::composite_clip`] at restore time and
    /// attached to the composite's `TextureInstance` in `flush_opacity_layer`,
    /// so the clip's coverage multiplies the finished group exactly once.
    pub(crate) composite_clip: Option<GroupClip>,
}

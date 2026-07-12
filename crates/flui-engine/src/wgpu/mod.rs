//! wgpu rendering backend for FLUI
//!
//! This module provides GPU-accelerated rendering using wgpu
//! (Vulkan/Metal/DX12/WebGPU).
//!
//! # Math-backend policy
//!
//! This engine layer uses `glam` (`Vec2`/`Mat4`/`vec4`, with `bytemuck` Pod)
//! **directly** for GPU and paint hot-path math. That is intentional and
//! sanctioned: `glam` is FLUI's chosen linear-algebra backend (it also backs
//! `flui_geometry::Matrix4` underneath), and the SIMD/Pod-friendly types belong
//! at the GPU boundary. Typed `flui_geometry` values are converted to `glam`
//! *here, at the engine edge* (`offset.dx.0`, `point.x.0`, …) — the typed unit
//! barrier lives in the layout/widget layers above, not in pixel-pushing code.
//! New direct `glam` use in this module is expected, not a smell.
//!
//! # Architecture
//!
//! ```text
//! Scene (flui-layer)
//!     │
//!     ▼
//! Renderer (renderer.rs)
//!     │ renders the LayerTree (Scene)
//!     ▼
//! Layer + LayerRender trait
//!     │ dispatch commands
//!     ▼
//! CommandRenderer trait (crate::traits)
//!     │
//!     ▼
//! Backend (backend.rs) → WgpuPainter (painter/)
//!     │                      │
//!     │                      ├── Tessellator
//!     │                      ├── TextRenderer
//!     │                      └── Effects
//!     ▼
//! wgpu (GPU)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::wgpu::Renderer;
//! use flui_layer::Scene;
//!
//! // Create a renderer for a window (owns per-window GPU state)
//! let mut renderer = Renderer::new(&window).await?;
//!
//! // Render a scene
//! renderer.render_scene(&scene)?;
//! ```

// ============================================================================
// CORE MODULES
// ============================================================================

/// Advanced dst-read blend composite driver: backdrop copy, pipeline, and
/// `flush_advanced_layer`.  No production caller exists yet (wired in PR-3);
/// the synthetic-op GPU gate in this module is the authoritative WGSL gate.
pub(crate) mod advanced_blend;
mod atlas;
mod backend;
/// Record-side draw accumulation helpers: `DrawBatcher` owns the tessellator,
/// path cache, and superellipse cache so they can be borrowed independently
/// from the flush-side state during draw recording.
mod batches;
/// Separable Gaussian blur filter: two H/V sub-passes with `exp(-0.5·i²/σ²)`
/// running-sum renormalisation. Premultiplied-direct, sRGB-encoded (PINNED #2).
/// [`blur::apply_blur`] is called by `apply_image_filter_passes` for
/// `ImageFilterPass::Blur`. [`blur::BlurPipeline`] owns the pipeline and
/// bind-group layout. Uses `FilterMode::Linear` (bilinear) — distinct from the
/// morphology pipeline (`NonFiltering` nearest).
pub(crate) mod blur;
mod buffer_pool;
/// Per-pixel 5×4 color-matrix filter pass: [`color_matrix::apply_color_matrix`]
/// applies a [`command_ir::LayerFilter::ColorMatrix`] to a premultiplied layer
/// offscreen via ping-pong into a 2nd pooled texture, then returns the filtered
/// texture for compositing.  [`color_matrix::ColorMatrixPipeline`] owns the
/// pipeline and bind-group layout.
pub(crate) mod color_matrix;
/// Command IR data types: `DrawSegment`, `DrawItem`, `SavedLayer`,
/// `PendingOpacityLayer`, `PendingOffscreenTexture`, and their helpers
/// (`ScissorRect`, `ScissorRegion`, `TessellatedBatch`). Moved here from
/// `painter` so the future batcher/compositor modules share one type home.
pub(crate) mod command_ir;
/// Per-channel sRGB ↔ linear-light gamma transfer filter pass:
/// [`gamma::apply_gamma`] applies a [`command_ir::LayerFilter::Gamma`] to a
/// premultiplied layer offscreen (unpremul → transfer per RGB → clamp →
/// repremul), writing the result into a 2nd pooled texture (ping-pong).
/// Alpha is unchanged.  [`gamma::GammaPipeline`] owns the pipeline and
/// bind-group layout.
pub(crate) mod gamma;
/// Per-pixel ColorFilter::Mode blend pass: [`mode::apply_mode`] applies a
/// [`command_ir::LayerFilter::Mode`] by compositing a solid filter color (SRC)
/// over each layer pixel (DST) using one of the 28 Porter-Duff / W3C blend
/// modes (unpremul DST → blend in straight sRGB → clamp → emit premul).
/// [`mode::ModePipeline`] owns the pipeline and bind-group layout.
pub(crate) mod mode;
// Cycle 4 wave 5 E-10: `buffers.rs` deleted. Module hosted
// `DynamicBuffer` (auto-growing vertex/instance buffer) and
// `BufferManager` (5-buffer GPU resource bag). Workspace grep
// returned zero non-self consumers in any crate; the wgpu module
// graph went through `buffer_pool.rs` (live, distinct logic --
// reusable per-frame allocations) instead. The whole module was
// dead code masked by the `pub use buffers::{...}` re-export at
// `wgpu/mod.rs:148`, which itself had zero consumers (item-level
// audit revealed both layers were dead).
#[cfg(debug_assertions)]
mod debug;
/// Gradient + shadow + blur instance descriptors consumed by `painter`'s
/// instanced-batch pipelines. The previous module-level `#[allow(dead_code)]`
/// reflex was removed in cycle 4 E-4 alongside the forward-looking helpers
/// (`ShadowParams::elevation_*`, `BlurIntensity`, `LinearGradientBuilder`,
/// the parallel `effects::BlurParams`); any zombie that returns lands as an
/// item-level lint, not a broad module suppression.
pub mod effects;
mod effects_pipeline;
mod external_texture_registry;
pub mod font_loader;
/// GPU instance-buffer types: `RectInstance`, `CircleInstance`,
/// `ArcInstance`, `TextureInstance`, gradient instances. All
/// surviving items are consumed by `painter`. Cycle 4 E-5
/// deleted the 6 forward-looking shortcuts the previous
/// module-level `#[allow(dead_code)]` was masking (`RectInstance::rounded_rect`,
/// `RectInstance::with_clip_rsuperellipse`, `RectInstance::with_transform`,
/// `CircleInstance::ellipse`, `ArcInstance::ellipse`,
/// `TextureInstance::with_rotation`); any zombie that returns now
/// surfaces as an item-level lint, not a broad module suppression.
mod instancing;
// NOTE: integration_tests.rs removed - needs rewrite for new
// Pixels/DevicePixels API
/// Separable morphological filter (dilate / erode) pass: [`morphology::apply_morphology`]
/// applies an [`command_ir::ImageFilterPass::Morph`] to a premultiplied layer
/// offscreen via two H/V sub-passes into pooled ping-pong textures, then returns
/// the filtered texture for compositing via `DrawItem::Filter`.
/// [`morphology::MorphologyPipeline`] owns the pipeline and bind-group layout.
pub(crate) mod morphology;
mod multi_draw;
mod offscreen;
mod painter;
pub mod path_cache;
/// Pipeline key types and cache consumed by `painter`. Live items:
/// `PipelineKey` (opaque/alpha-blend factory methods + bitfield queries),
/// `PipelineCache` (get_or_create, viewport_bind_group_layout), and
/// `pipeline_key_from_paint`. Unused constants/methods/cache helpers deleted.
mod pipeline;
// Cycle 4 E-6: the parallel `pipelines.rs` (with its own `PipelineCache` +
// `PipelineBuilder` structs, name-colliding with `pipeline.rs`) was deleted.
// The `pipelines.rs` introduced by T3 is distinct: it defines `PipelineSet`,
// which *composes* the live `PipelineCache` from `pipeline.rs` (singular) and
// adds the nine named pipelines previously scattered as painter fields.
/// Opacity/layer save-state machine: `opacity_stack`, `current_opacity`, and
/// `layer_stack` extracted from `WgpuPainter`.  Owns the book-keeping half of
/// `save_layer`/`restore_layer`; GPU emission lives in `GpuReplay`.
pub(super) mod layer_compositor;
/// Offscreen-layer render helper: extracted from `flush_opacity_layer` so the
/// renderer driver can reuse the same routine for backdrop-read compositing.
pub(super) mod opacity_layer;
pub(crate) mod pipelines;
mod profiler;
/// Frame render-target descriptor: `view` + optional back-reference `texture`
/// for dst-read blend passes.  Frame-scoped borrow, never stored in IR types.
pub(crate) mod render_target;
mod renderer;
/// Replay/submit component: owns GPU plumbing fields, the per-frame
/// `texture_batch` scratch, all six segment-flush phases, the top-level
/// `submit` dispatch loop, and `flush_opacity_layer` recursion.
pub(super) mod replay;
pub(crate) mod resources;
/// Shader cache for offscreen pipelines (`OffscreenRenderer` mask /
/// blur / morph). Cycle 4 E-7 dropped the module-level
/// `#[allow(dead_code)]` mask: the only forward-looking helper
/// (`ShaderCache::clear`) is now gated behind
/// `#[cfg(feature = "devtools")]`, so default-build dead-code
/// surfaces as an item-level lint rather than a broad module
/// suppression. The audit also mentioned `cached_count` but no such
/// method existed -- only `clear`.
mod shader_compiler;
/// naga_oil shader composition helper: resolves `#import` directives
/// in WGSL at pipeline-init time via [`shader_composer::compose_wgsl_shader`].
/// Used by `mode/pipeline.rs` and `advanced_blend/pipeline.rs` to
/// compose `blend_helpers.wgsl` into each entry shader, replacing the
/// previous `concat!(include_str!(...))` approach.
pub(crate) mod shader_composer;
mod shaders;
/// SSAA (2× supersampled) path anti-aliasing: downsample pipeline and replay
/// helper `GpuReplay::render_ssaa_path` / `GpuReplay::downsample_ssaa_tile`.
/// Handles Fill + SrcOver arbitrary paths that were diverted by `draw_path`.
pub(crate) mod ssaa;
/// GPU draw-state stack: the four paired transform/scissor/SDF-clip stacks
/// and their cached current values, extracted from `WgpuPainter` so they can
/// be owned and delegated as a unit. Owned by `WgpuPainter` via the `state`
/// field.
pub(super) mod state_stack;
pub mod superellipse_cache;
mod tessellator;
mod text;
pub mod texture_cache;
mod texture_pool;
mod uniform_pool;
mod vertex;

// ============================================================================
// LAYER RENDERING
// ============================================================================

pub(crate) mod layer_render;

#[cfg(test)]
mod sdf_smoke_test;

// aa_oracle_tests contains both CPU unit tests (no GPU) and GPU readback tests.
// Include whenever test compilation is active.
#[cfg(test)]
mod aa_oracle_tests;

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod deterministic_replay_tests;

// layer_blend_tests contains both cfg(test) unit tests and
// cfg(all(test, feature = "enable-wgpu-tests")) GPU tests.
// Include the file whenever test compilation is active.
#[cfg(test)]
mod layer_blend_tests;

// shape_blend_tests contains PR-4 GPU acceptance tests for shape-level advanced
// blend (rect/rrect tessellated path → DrawItem::AdvancedShape).
// PR-4 unit tests (S1-S4g) are inline in batches/mod.rs.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod shape_blend_tests;

// gradient_image_blend_tests contains PR-5 GPU acceptance tests for gradient
// and image advanced blend (dispatch_shader_rect + draw_image* paths).
// PR-5 unit tests (G1-G6) are inline in batches/mod.rs.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gradient_image_blend_tests;

// color_matrix_filter_tests contains F1-F6 GPU readback tests for the
// color-matrix filter pass (identity, swap-R↔B, translucent premul roundtrip,
// transpose-bug discriminator, brightness on translucent, nested opacity).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod color_matrix_filter_tests;

// morphology_filter_tests contains M1-M6 GPU readback tests for the
// morphology filter pass (identity, dilate border expand, erode border contract,
// premul-direct discriminator, decal boundary, grown_bounds wiring).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod morphology_filter_tests;

// mode_filter_tests contains MO1-MO6 GPU readback tests for the
// ColorFilter::Mode blend pass (Modulate identity, Multiply opaque, SrcOver
// translucent premul-bracket, Screen separable, Hue non-separable,
// Luminosity translucent).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod mode_filter_tests;

// gamma_filter_tests contains GA1-GA6 GPU readback tests for the gamma
// transfer filter pass (SrgbToLinear known value, LinearToSrgb inverse,
// translucent alpha unchanged, round-trip ≈ identity, black/white boundary).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gamma_filter_tests;

// blur_filter_tests contains B1-B5 GPU readback tests for the Gaussian blur
// filter pass (no-dark-halo premul discriminator, anisotropy, oracle match,
// zero-sigma identity, grown_bounds halo extent).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod blur_filter_tests;

// compose_filter_tests contains C1-C5 acceptance tests for ImageFilter::Compose
// flatten + Chain execution: order-matters discriminator (C1), nesting structure
// (C2), deep-chain heap-spill (C3), cumulative bounds (C4), degenerate cases (C5).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod compose_filter_tests;

// color_filter_producer_tests contains P1-P4 GPU readback acceptance tests for
// the T1 producer-path change: Backend::push_color_filter(&ColorFilter) dispatch
// for Mode (P1), LinearToSrgbGamma (P2), SrgbToLinearGamma (P3), and Matrix (P4).
// These tests would fail to compile on main (old &ColorMatrix signature).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod color_filter_producer_tests;

// scenebuilder_filter_chain_tests contains SC1-SC5 GPU readback acceptance tests
// for T2′ of `gpu-filters-consumer-chain`: SceneBuilder→LayerTree→LayerRender→
// Backend→GPU pixel closure for image-filter blur (SC1), Mode/Multiply (SC2),
// LinearToSrgbGamma (SC3), SrgbToLinearGamma (SC4), and Matrix/grayscale (SC5).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod scenebuilder_filter_chain_tests;

// ============================================================================
// PUBLIC API
// ============================================================================

// ----------------------------------------------------------------------------
// Cycle 4 wave 5 E-10: surface trim
//
// Workspace ripgrep of `flui_engine::wgpu::<Name>` returned consumers
// ONLY for `Renderer` (4 callsites in flui-app); every other
// `pub use` re-export here had zero external (non-flui-engine)
// consumers AND zero in-crate consumers either (sibling modules
// reach internal types via their module paths directly, not via
// the re-export at this module level).
//
// The previously-`pub` re-exports were paying public-API
// monomorphization + discoverability + stability cost for nothing.
// They are deleted outright, NOT demoted to `pub(crate)`: a
// `pub(crate) use` line with zero in-crate consumers is itself
// dead code (rustc emits `unused_imports` for it under
// `-D warnings`). The 30+ "dead-code" warnings the previous
// wave-4 attempt produced were masking the real signal: the
// re-exports were never wired into the module graph at this
// level.
//
// `Backend`, `FontLoader`, `LayerRender`, `WgpuPainter`, `DebugBackend`,
// `Renderer`, and the `commands::` / `traits::` re-exports stay
// because they have verified consumers (either flui-app callsites
// or lib.rs's crate-root re-export chain).
// ----------------------------------------------------------------------------

// Backend (external via lib.rs re-export at crate root)
pub use backend::Backend;
// Command rendering (re-exported from crate root)
pub use crate::{
    commands::{dispatch_command, dispatch_commands},
    traits::CommandRenderer,
};
#[cfg(debug_assertions)]
pub use debug::DebugBackend;
// Layer rendering (external via lib.rs re-export at crate root)
pub use layer_render::LayerRender;
pub use painter::WgpuPainter;

// Renderer (the one and only externally-consumed wgpu/* type)
pub use renderer::Renderer;
// Font loading utilities (external via lib.rs re-export at crate root)
pub use font_loader::FontLoader;
// GPU frame profile — feature-independent type, always available so callers
// can store/display profiling results without gating on `gpu-profiler`.
pub use profiler::{GpuFrameProfile, PassTiming};

// Offscreen renderer + texture pool — re-exported ONLY under the
// `enable-wgpu-tests` feature for the `offscreen_resource_cache` criterion bench.
// These are internal GPU types and are NOT part of the public API; consumers use
// `Renderer` / `WgpuPainter`. Gated so benching does not widen the public surface.
#[cfg(feature = "enable-wgpu-tests")]
pub use offscreen::OffscreenRenderer;
#[cfg(feature = "enable-wgpu-tests")]
pub use texture_pool::{PooledTexture, TexturePool};

// Painter (WgpuPainter is the concrete implementation; the `Painter` trait was deleted)

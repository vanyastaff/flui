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
//! CommandRenderer trait (crate::command_renderer)
//!     │
//!     ▼
//! LayerDispatcher (layer_dispatcher.rs) → WgpuPainter (painter/)
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
//! ```rust,no_run
//! # async fn render(
//! #     window: impl flui_engine::wgpu::WindowTarget,
//! #     scene: &flui_layer::Scene,
//! # ) -> Result<(), flui_engine::EngineError> {
//! use flui_engine::wgpu::Renderer;
//!
//! // Create a renderer for a window (owns per-window GPU state). `window`
//! // is an owned, `'static` handle source — see `WindowTarget` — not a
//! // borrow, so the renderer can outlive the caller's stack frame.
//! let mut renderer = Renderer::new(window).await?;
//!
//! // Render a scene
//! renderer.render_scene(scene)?;
//! # Ok(())
//! # }
//! ```

// ============================================================================
// CORE MODULES
// ============================================================================

/// Advanced dst-read blend composite driver: backdrop copy, pipeline, and
/// `flush_advanced_layer`.  No production caller exists yet (wired in PR-3);
/// the synthetic-op GPU gate in this module is the authoritative WGSL gate.
/// Shared adapter/device acquisition policy: the one `RequestAdapterOptions`
/// used everywhere, the capability-negotiated `DeviceDescriptor`, and the
/// composed offscreen acquisition the offscreen stacks share.
mod adapter;
pub(crate) mod advanced_blend;
mod atlas;
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
mod layer_dispatcher;
/// Per-pixel ColorFilter::Mode blend pass: [`mode::apply_mode`] applies a
/// [`command_ir::LayerFilter::Mode`] by compositing a solid filter color (SRC)
/// over each layer pixel (DST) using one of the 28 Porter-Duff / W3C blend
/// modes (unpremul DST → blend in straight sRGB → clamp → emit premul).
/// [`mode::ModePipeline`] owns the pipeline and bind-group layout.
pub(crate) mod mode;
// A command recorder with no GPU: it exists so the dispatch tests can assert
// which `render_*` arm fired without a device. Test-only, which is also what
// makes it honest — nothing in a shipped build constructs one.
#[cfg(test)]
pub(crate) mod debug;
/// Gradient, shadow, and blur instance descriptors.
///
/// `GradientStop` and `ShadowParams` appear in `WgpuPainter`'s public
/// methods, so the module stays reachable; the instance structs are
/// `#[doc(hidden)]` batch payloads a caller never constructs by hand.
pub mod effects;
mod effects_pipeline;
mod external_texture_registry;
/// Windowless GPU capture: rasterize a `LayerTree` to an offscreen texture and
/// read the pixels back (golden-image / screenshot tooling).
pub mod headless;
/// GPU instance-buffer types: `RectInstance`, `CircleInstance`,
/// `ArcInstance`, `TextureInstance`, and the gradient/shadow instances.
/// Consumed by `painter`, which owns the per-primitive batch layout.
mod instancing;
/// Opacity/layer save-state machine: `opacity_stack`, `current_opacity`, and
/// `layer_stack` extracted from `WgpuPainter`.  Owns the book-keeping half of
/// `save_layer`/`restore_layer`; GPU emission lives in `GpuReplay`.
pub(super) mod layer_compositor;
/// Offscreen-layer rendering and compositing: `render_segment_to_offscreen`,
/// `render_layer_to_offscreen`, `flush_opacity_layer`, and the filter-chain
/// folding they drive. Named for the job (a layer rendered to a texture), not
/// for one of its callers.
pub(super) mod layer_offscreen;
/// Separable morphological filter (dilate / erode) pass: [`morphology::apply_morphology`]
/// applies an [`command_ir::ImageFilterPass::Morph`] to a premultiplied layer
/// offscreen via two H/V sub-passes into pooled ping-pong textures, then returns
/// the filtered texture for compositing via `DrawItem::Filter`.
/// [`morphology::MorphologyPipeline`] owns the pipeline and bind-group layout.
pub(crate) mod morphology;
mod offscreen;
mod painter;
/// Path tessellation cache. `#[doc(hidden)] pub` rather than `pub(crate)`
/// because this crate's own `render_throughput` criterion bench — a separate
/// crate target — measures a warm `PathCache::get` hit; that measurement is
/// the only reason the name is reachable from outside. It is not an
/// embedder API and is hidden from a published docs page.
#[doc(hidden)]
pub mod path_cache;
/// Pipeline key types and cache consumed by `painter`: `PipelineKey`
/// (opaque/alpha-blend factory methods + bitfield queries), `PipelineCache`
/// (get_or_create, viewport_bind_group_layout), and `pipeline_key_from_paint`.
mod pipeline_cache;
/// `PipelineSet` composes the live `PipelineCache` from `pipeline.rs`
/// (singular) and adds the nine named pipelines previously scattered as
/// painter fields. The name-colliding earlier file with its own
/// `PipelineCache`/`PipelineBuilder` is gone; this module is the surviving
/// half.
pub(crate) mod pipeline_set;
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
/// Shader cache for the offscreen pipelines (`OffscreenRenderer` mask /
/// blur / morph). Its surface is exactly what those pipelines call.
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
// The owned-target/surface protocol (`SurfaceLease`) — GPU-free so its
// probe-before-build and drop-order invariants can be tested and (later)
// interpreted under Miri without a real `wgpu::Surface`. See the module's
// own `//!` doc for details; no outer doc here to avoid duplicating it in a
// scope where its intra-doc links resolve differently (rustdoc quirk).
mod surface_lease;
mod tessellator;
mod text;
pub(crate) mod texture_cache;
mod texture_pool;
mod uniform_pool;
mod vertex;
// What a windowed `Renderer` draws into: an owned, `'static`, `Send + Sync`
// handle source (issue #1043). Exported here beside `Renderer`. See the
// module's own `//!` doc for details; no outer doc here for the same reason
// as `surface_lease` above.
mod window_target;

// ============================================================================
// LAYER RENDERING
// ============================================================================

pub(crate) mod layer_render;
pub(crate) mod layer_walk;

// readback_dump is shared test-support for every GPU readback/oracle test in
// this module: when `FLUI_READBACK_DUMP_DIR` is set, each local readback
// helper dumps the actual frame as a PNG there (the CI GPU job uploads
// `<dir>/**/*.png` as an artifact on oracle failure). Compiled for all test
// builds — not just `enable-wgpu-tests` — so its no-GPU unit tests run on
// every host; with the env var unset every entry point is a no-op.
#[cfg(test)]
mod readback_dump;

// test_support is the other half of the shared scaffolding: adapter/device
// acquisition, render-target creation, clear passes, and the padded-row
// staging readback that every GPU suite previously carried its own copy of.
//
// Gated on `cfg(test)` alone, not on `enable-wgpu-tests`. It was gated on the
// feature "like its consumers" while every consumer was a feature-gated filter
// suite; the three readback suites below are `cfg(test)` only, and they are the
// ones that needed `renderer_or_skip` — a helper that lives behind a feature its
// callers do not have is a helper they cannot call.
#[cfg(test)]
pub(crate) mod test_support;

// A GPU-free `WindowTarget` test double shared by `surface_lease.rs`'s and
// `renderer.rs`'s own unit tests, so both exercise the identical fake
// instead of two copies that could silently diverge (issue #1043).
#[cfg(test)]
pub(crate) mod fake_window_target;

#[cfg(test)]
mod sdf_smoke_test;

#[cfg(test)]
mod clip_layer_readback_tests;

// The CPU model of the fixed-function blender that every readback suite
// asserting an exact blended byte predicts against.
#[cfg(test)]
mod blend_oracle;

// The clip suite's companion: same clip, but asserting the ARITHMETIC of a
// partially covered blend rather than counting clipped pixels.
#[cfg(test)]
mod coverage_blend_readback_tests;

// The same two questions asked of the GRADIENT path, which reaches the blender
// through its own instanced pipelines rather than the tessellated funnel.
#[cfg(test)]
mod gradient_blend_readback_tests;

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

// gradient_image_blend_tests holds the GPU acceptance tests for advanced
// (dst-read) blend on gradients and images — the `dispatch_shader_rect` and
// `draw_image*` paths. Gradient-diversion unit tests are inline in
// batches/mod.rs.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gradient_image_blend_tests;

// color_matrix_filter_tests contains GPU readback tests for the
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
// the T1 producer-path change: LayerDispatcher::push_color_filter(&ColorFilter) dispatch
// for Mode (P1), LinearToSrgbGamma (P2), SrgbToLinearGamma (P3), and Matrix (P4).
// These tests would fail to compile on main (old &ColorMatrix signature).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod color_filter_producer_tests;

// scenebuilder_filter_chain_tests contains SC1-SC5 GPU readback acceptance tests
// for T2′ of `gpu-filters-consumer-chain`: SceneBuilder→LayerTree→LayerRender→
// LayerDispatcher→GPU pixel closure for image-filter blur (SC1), Mode/Multiply (SC2),
// LinearToSrgbGamma (SC3), SrgbToLinearGamma (SC4), and Matrix/grayscale (SC5).
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod scenebuilder_filter_chain_tests;

// ============================================================================
// PUBLIC API
// ============================================================================

// ----------------------------------------------------------------------------
// Public re-export surface
//
// The embedder-facing names are `Renderer`, `WindowTarget`, `WgpuPainter`,
// `HeadlessRenderer`, and `GpuResourceGeneration`. Everything else this
// module used to re-export (`LayerDispatcher`, `LayerRender`, `CommandRenderer`, the
// `dispatch_*` pair, `DebugBackend`, `FontLoader`) has no consumer outside
// the workspace: `flui-app` reaches the renderer only through
// `RasterBackend`, and the layer walk / command dispatch is internal
// machinery no embedder composes by hand. Those names stay reachable at
// `flui_engine::wgpu::<module>` for in-workspace callers but are no longer
// re-exported here, so the docs.rs surface is the entry points rather than
// the plumbing.
//
// `OffscreenRenderer`/`PooledTexture`/`TexturePool` stay gated on
// `enable-wgpu-tests` for the `offscreen_resource_cache` criterion bench;
// that feature is not enabled by `[package.metadata.docs.rs]`, so they do
// not appear on a published page.
// ----------------------------------------------------------------------------

// The dispatcher and command dispatch stay `pub(crate)` at this module:
// `pub(crate) use` lines with no in-crate consumer would themselves be
// dead imports, and in-crate callers name `layer_dispatcher::LayerDispatcher` /
// `layer_render::LayerRender` directly.
// Windowless capture (golden-image / screenshot tooling).
pub use headless::HeadlessRenderer;
pub use painter::WgpuPainter;

// Renderer (the one externally-consumed wgpu/* type)
pub use renderer::Renderer;
// What a windowed `Renderer` draws into (issue #1043); exported beside
// `Renderer` here at `flui_engine::wgpu`.
pub use window_target::WindowTarget;
// The GPU-resource freshness stamp (ADR-0045 decision 4). Declared in
// `flui-foundation` because `FrameStamp` compares this axis at the raster
// boundary and cannot name a `flui-engine` type; re-exported here and at the
// crate root so `flui_engine::GpuResourceGeneration` keeps resolving for the
// compile-fail fixture that pins its private field.
pub use flui_foundation::GpuResourceGeneration;
// GPU frame profile — feature-independent type, always available so callers
// can store/display profiling results without gating on `gpu-profiler`.
pub use profiler::{GpuFrameProfile, PassTiming};

// Offscreen renderer + texture pool — re-exported ONLY under the
// `enable-wgpu-tests` feature for the `offscreen_resource_cache` criterion bench.
// Gated so benching does not widen the public surface.
#[cfg(feature = "enable-wgpu-tests")]
pub use offscreen::OffscreenRenderer;
#[cfg(feature = "enable-wgpu-tests")]
pub use texture_pool::{PooledTexture, TexturePool};

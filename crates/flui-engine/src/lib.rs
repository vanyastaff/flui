// `missing_debug_implementations` is suppressed crate-wide for the public
// types that hold `lyon`/atlas state or boxed closures with no `Debug`
// (`WgpuPainter`, `RasterOwner<B>`'s backend, the pre-present hook). wgpu 30's
// own handles (Device, Queue, Texture, Buffer, …) all derive `Debug`, so they
// are not the reason. It is an `expect`, not an `allow`: if the last such
// type gains `Debug`, the attribute must go with it.
#![expect(missing_debug_implementations)]
// No hand-written production `unsafe` in this crate: surfaces come from wgpu's safe
// `create_surface` over an owned `WindowTarget` (ADR-0063), and every GPU
// handle is a wgpu value type. `deny` rather than `forbid` because
// wgsl_bindgen's generated wrappers (`*/generated.rs`) declare an
// `unsafe fn from_raw` this crate never calls and `allow` it locally.
// `fake_window_target` uses test-only raw-handle borrows for synthetic window
// and display handles, gated on `cfg(test)`.
#![cfg_attr(not(test), deny(unsafe_code))]
// `unreachable_pub` is a warning, not an error, because the workspace lint set
// does not carry it: it fires for a `pub` item in a private or `pub(crate)`
// module, which is a real finding (it claims reachability it does not have)
// but not a correctness one. It caught 180 such declarations in this crate
// when it was enabled, and every remaining `pub` now either reaches the crate
// root or carries a scoped `#[cfg_attr(..., expect(unreachable_pub))]` naming
// the feature that makes it reachable. Keep it on: the next one is a warning,
// not a discovery.
#![warn(unreachable_pub)]
// Two lints are suppressed crate-wide for the wgpu backend, each with a
// named site rather than a blanket rationale:
//
// - `struct_field_names`: the instance batches carry one `*_batch` field per
//   primitive family (`rect_batch`, `circle_batch`, …), which is the field's
//   domain name, not a redundant suffix.
// - `large_enum_variant`: `command_ir::ImageFilterSpec` and
//   `layer_compositor::RestoreOutcome` are dominated by one variant each;
//   both are short-lived per-frame values, so boxing the large variant would
//   trade an allocation on the frame path for a stack-size nit.
#![expect(clippy::struct_field_names, clippy::large_enum_variant)]
// The next-generation trait solver counts auto-trait proof depth honestly,
// and proving `wgpu::Renderer: Send` descends through wgpu-core
// (`Global -> Hub -> Registry -> RwLock<Storage<..>>`) past the default
// limit of 128. On stable that is invisible; on nightly it is the
// future-incompat lint `recursion_depth_exceeding_limit`
// (rust-lang/rust#159228), which becomes a hard error once the solver
// stabilises. Auto-trait proofs are structural and re-run in every crate
// that needs `Renderer: Send`, so flui-app and the root examples carry the
// same attribute. A manual `impl Send` on an intermediate type would
// short-circuit the proof, but reopening a hand-written `Send` assertion
// is exactly what ADR-0045 decision 1 removed; the limit is the honest fix
// until wgpu adds the impls upstream (same class as rust-lang/rust#160036).
#![recursion_limit = "256"]

//! GPU compositor for FLUI: it turns a [`flui_layer::Scene`] into wgpu draw
//! calls (Vulkan / Metal / DX12 / WebGPU).
//!
//! wgpu is the engine, not a backend behind one: the crate re-exports the
//! exact [`wgpu`] it links against, so an embedder that hands over a device
//! or reads a surface format names the same version without a second
//! dependency line.
//!
//! # Which entry point do you want?
//!
//! | You are | Use |
//! |---|---|
//! | rendering into a window | [`Renderer::new`] — owns one window's device, queue, and surface, and recovers from device loss |
//! | rendering without a window (tests, screenshots, thumbnails, CI) | [`HeadlessRenderer::new`] — rasterizes a layer tree to RGBA8 bytes |
//! | driving draw calls yourself | [`WgpuPainter`] — the per-frame painter; `examples/painting_demo` is the worked example |
//! | writing the application frame loop | [`RasterBackend`] — the trait FLUI's own runners call, so the backend is one construction site |
//!
//! All three constructors are `async`, because wgpu's adapter and device
//! requests are. The library does not choose a blocking strategy for you: an
//! embedder with an async runtime awaits, and one without wraps the call in
//! its own `block_on`.
//!
//! # One frame
//!
//! ```rust,no_run
//! # async fn frame(
//! #     window: impl flui_engine::WindowTarget,
//! #     scene: &flui_layer::Scene,
//! # ) -> Result<(), flui_engine::EngineError> {
//! use flui_engine::Renderer;
//!
//! // `window` is an OWNED, `'static` handle source (`WindowTarget`), not a
//! // borrow: the renderer keeps it for as long as its surface lives, so it
//! // outlives this stack frame. Almost anything that implements
//! // `HasWindowHandle + HasDisplayHandle + Send + Sync + 'static` qualifies —
//! // including `Arc<dyn PlatformWindow>` from `flui-platform`.
//! let mut renderer = Renderer::new(window).await?;
//!
//! // Render on every frame your event loop asks for; `render_scene` is sync
//! // and does not block on vsync unless the surface does.
//! // `Ok(false)` means the frame was skipped (no damage, or an occluded
//! // surface) rather than drawn.
//! renderer.render_scene(scene)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Architecture
//!
//! Everything below [`Scene`](flui_layer::Scene) is this crate's internals,
//! reached through the entry point you picked above:
//!
//! ```text
//! Scene (flui-layer)          built by the widget tree's Canvas
//!     │
//!     ▼
//! Renderer                    owns one window's GPU stack
//!     │ walks the LayerTree
//!     ▼
//! LayerDispatcher             routes each DrawCommand to the painter
//!     │
//!     ▼
//! WgpuPainter                 record: batched Command IR
//!     │
//!     ▼
//! GpuReplay                   replay: Command IR → wgpu draw calls
//!     │
//!     ▼
//! GPU (wgpu)
//! ```
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
//! New direct `glam` use in this crate is expected, not a smell.
//!
//! # Cargo features
//!
//! - `vulkan` / `metal` / `dx12` / `webgpu` / `gles` — add a wgpu backend API
//!   explicitly. These are additive pass-throughs to wgpu's own features, not
//!   selectors: the right one for the target OS is already enabled by this
//!   crate's per-target dependency entries, and enabling two compiles both.
//! - `gpu-profiler` — per-pass GPU timings, off by default; no-ops at runtime
//!   on an adapter without timestamp-query support. Rejected on wasm32.
//! - `testing` — test support only; not part of the public API.

// Every public item is documented; keep it that way.
#![deny(missing_docs)]

// Compile-time guard: the `fragile-send-sync-non-atomic-wasm` wgpu feature
// marks !Send types as Send+Sync, which is only sound when the wasm target has
// no threads (no atomics target feature). Enabling atomics with this feature
// is UB — catch it at compile time instead of silently producing data races.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
compile_error!(
    "fragile-send-sync-non-atomic-wasm is unsound with threads/atomics enabled \
     — see flui-engine Cargo.toml [target.wasm32] note"
);

// The `gpu-profiler` + wasm32 combination is rejected at compile time so a
// downstream consumer cannot enable both by accident.
//
// The original reason was that wgpu-profiler 0.27 did not build for wasm32 at
// all. That is no longer why: 0.28 type-checks clean for
// `wasm32-unknown-unknown` (measured — `cargo check -p flui-engine --target
// wasm32-unknown-unknown --features gpu-profiler` succeeds with the guard
// lifted). What has NOT changed is that nothing here exercises timestamp
// queries on a browser WebGPU adapter, so the guard stays: it now marks an
// unverified configuration rather than an impossible one, and lifting it is a
// deliberate decision that needs a run on a real wasm target, not a
// side effect of a version bump.
#[cfg(all(target_arch = "wasm32", feature = "gpu-profiler"))]
compile_error!(
    "the `gpu-profiler` feature is not supported on wasm32 by this workspace; \
     build without it"
);

// ============================================================================
// ABSTRACT LAYER (backend-agnostic)
// ============================================================================

/// Common error types for all rendering backends
pub mod error;

/// The command dispatch surface (`CommandRenderer` — crate-private). Plumbing: the
/// layer walk dispatches through it, no embedder implements it today.
pub(crate) mod command_renderer;

/// The layer-tree state hand-off (`LayerStateStack`), the sibling of
/// `command_renderer`.
pub(crate) mod layer_state_stack;

/// `DrawCommand` dispatch functions (crate-private). Same as the traits.
pub(crate) mod dispatch;

/// Backend-agnostic superellipse (iOS squircle) path generation.
/// Pure geometry — no wgpu, no lyon.
///
/// Test-only since issue #935 retired the tessellation route: a squircle clip
/// is a signed distance field evaluated per fragment, so nothing in a shipped
/// build asks for the path. What the CPU generator is now is the ORACLE for
/// that SDF — `common/clip.wgsl`'s `sdRoundedSuperellipse` is the shipped
/// evaluator of the same `n = 4` parametric form, and
/// `the_squircle_sdf_agrees_with_the_cpu_path_across_the_whole_boundary` holds
/// the two to each other pixel by pixel. Compiling it only where that test is
/// compiled says so plainly, rather than shipping a function nothing calls —
/// and the feature half of the gate is load-bearing, not belt-and-braces: the
/// readback suite lives under `mod wgpu`, so `--no-default-features` drops the
/// oracle and `cfg(test)` alone would leave the generator unused there. The
/// per-feature CI pass is what catches that; a default-feature build does not.
#[cfg(test)]
pub(crate) mod superellipse;

/// The frame-driver trait ([`RasterBackend`]) `Renderer` implements and
/// flui-app's GPU-free test doubles stand in for.
pub mod raster;

/// The raster mailbox + dedicated ack channel boundary
/// ([`RasterOwner`]/[`RasterHandle`]/[`RasterAck`]).
/// Generic over [`RasterBackend`].
pub mod raster_owner;

/// Portable monotonic timestamps for GPU-path diagnostic spans.
/// Acquire/present timers must not panic on wasm32 before the wgpu backend is
/// even asked for a surface texture (issue #1045).
pub(crate) mod frame_timing;

// ============================================================================
// THE wgpu RASTER PIPELINE
// ============================================================================

/// The wgpu this crate links against, re-exported so embedders name the same
/// version (device handoff, surface formats, `wgpu::Features` queries).
pub use ::wgpu;

/// Advanced dst-read blend composite driver: backdrop copy, pipeline, and
/// `flush_advanced_layer`; the synthetic-op GPU gate in this module is the
/// authoritative WGSL gate.
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
/// Per-frame dirty-rect accumulator behind the `render_scene` scissor.
mod damage;
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
/// Gradient, shadow, and blur instance descriptors: the batch payloads the
/// shader-paint and shadow dispatch build from a `Paint`/`DrawOp`.
mod effects;
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
pub(crate) mod layer_compositor;
/// Offscreen-layer rendering and compositing: `render_segment_to_offscreen`,
/// `render_layer_to_offscreen`, `flush_opacity_layer`, and the filter-chain
/// folding they drive. Named for the job (a layer rendered to a texture), not
/// for one of its callers.
pub(crate) mod layer_offscreen;
/// Separable morphological filter (dilate / erode) pass: [`morphology::apply_morphology`]
/// applies an [`command_ir::ImageFilterPass::Morph`] to a premultiplied layer
/// offscreen via two H/V sub-passes into pooled ping-pong textures, then returns
/// the filtered texture for compositing via `DrawItem::Filter`.
/// [`morphology::MorphologyPipeline`] owns the pipeline and bind-group layout.
pub(crate) mod morphology;
mod offscreen;
mod painter;
/// Path tessellation cache; `PathCache` is re-exported under
/// `testing` for the `render_throughput` bench alone.
mod path_cache;
/// Pipeline key types and cache consumed by `painter`: `PipelineKey`
/// (opaque/alpha-blend factory methods + bitfield queries), `PipelineCache`
/// (get_or_create, viewport_bind_group_layout), and `pipeline_key_from_paint`.
mod pipeline_cache;
/// `pipeline_set.rs` — `PipelineSet` composes the live `PipelineCache` from
/// `pipeline_cache.rs` and adds the nine named pipelines previously scattered
/// as painter fields. The name-colliding earlier file with its own
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
pub(crate) mod replay;
pub(crate) mod resources;
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
/// Transform and clip state with complete save/restore snapshots, owned by `WgpuPainter`.
pub(crate) mod state_stack;
// The owned-target/surface protocol (`SurfaceLease`) — GPU-free so its
// probe-before-build and drop-order invariants can be tested and (later)
// interpreted under Miri without a real `wgpu::Surface`. See the module's
// own `//!` doc for details; no outer doc here to avoid duplicating it in a
// scope where its intra-doc links resolve differently (rustdoc quirk).
mod glyph_atlas;
mod surface_lease;
mod tessellator;
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
// builds — not just `testing` — so its no-GPU unit tests run on
// every host; with the env var unset every entry point is a no-op.
#[cfg(test)]
mod readback_dump;

// test_support is the other half of the shared scaffolding: adapter/device
// acquisition, render-target creation, clear passes, and the padded-row
// staging readback that every GPU suite previously carried its own copy of.
//
// Gated on `cfg(test)` alone, not on `testing`. It was gated on the
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

// The engine paints the paragraph it is handed and shapes nothing (ADR-0065).
#[cfg(test)]
mod paragraph_readback_tests;

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

#[cfg(all(test, feature = "testing"))]
mod deterministic_replay_tests;

// layer_blend_tests contains both cfg(test) unit tests and
// cfg(all(test, feature = "testing")) GPU tests.
// Include the file whenever test compilation is active.
#[cfg(test)]
mod layer_blend_tests;

// GPU acceptance tests for shape-level advanced blend (rect/rrect
// tessellated path → DrawItem::AdvancedShape); the CPU-side unit tests are
// inline in batches/mod.rs.
#[cfg(all(test, feature = "testing"))]
mod shape_blend_tests;

// gradient_image_blend_tests holds the GPU acceptance tests for advanced
// (dst-read) blend on gradients and images — the `dispatch_shader_rect` and
// `draw_image*` paths. Gradient-diversion unit tests are inline in
// batches/mod.rs.
#[cfg(all(test, feature = "testing"))]
mod gradient_image_blend_tests;

// color_matrix_filter_tests contains GPU readback tests for the
// color-matrix filter pass (identity, swap-R↔B, translucent premul roundtrip,
// transpose-bug discriminator, brightness on translucent, nested opacity).
#[cfg(all(test, feature = "testing"))]
mod color_matrix_filter_tests;

// morphology_filter_tests contains M1-M6 GPU readback tests for the
// morphology filter pass (identity, dilate border expand, erode border contract,
// premul-direct discriminator, decal boundary, grown_bounds wiring).
#[cfg(all(test, feature = "testing"))]
mod morphology_filter_tests;

// mode_filter_tests contains MO1-MO6 GPU readback tests for the
// ColorFilter::Mode blend pass (Modulate identity, Multiply opaque, SrcOver
// translucent premul-bracket, Screen separable, Hue non-separable,
// Luminosity translucent).
#[cfg(all(test, feature = "testing"))]
mod mode_filter_tests;

// gamma_filter_tests contains GA1-GA6 GPU readback tests for the gamma
// transfer filter pass (SrgbToLinear known value, LinearToSrgb inverse,
// translucent alpha unchanged, round-trip ≈ identity, black/white boundary).
#[cfg(all(test, feature = "testing"))]
mod gamma_filter_tests;

// blur_filter_tests contains B1-B5 GPU readback tests for the Gaussian blur
// filter pass (no-dark-halo premul discriminator, anisotropy, oracle match,
// zero-sigma identity, grown_bounds halo extent).
#[cfg(all(test, feature = "testing"))]
mod blur_filter_tests;

// compose_filter_tests contains C1-C5 acceptance tests for ImageFilter::Compose
// flatten + Chain execution: order-matters discriminator (C1), nesting structure
// (C2), deep-chain heap-spill (C3), cumulative bounds (C4), degenerate cases (C5).
#[cfg(all(test, feature = "testing"))]
mod compose_filter_tests;

// color_filter_producer_tests contains P1-P4 GPU readback acceptance tests for
// the T1 producer-path change: LayerDispatcher::push_color_filter(&ColorFilter) dispatch
// for Mode (P1), LinearToSrgbGamma (P2), SrgbToLinearGamma (P3), and Matrix (P4).
// These tests would fail to compile on main (old &ColorMatrix signature).
#[cfg(all(test, feature = "testing"))]
mod color_filter_producer_tests;

// scenebuilder_filter_chain_tests contains SC1-SC5 GPU readback acceptance tests
// for T2′ of `gpu-filters-consumer-chain`: SceneBuilder→LayerTree→LayerRender→
// LayerDispatcher→GPU pixel closure for image-filter blur (SC1), Mode/Multiply (SC2),
// LinearToSrgbGamma (SC3), SrgbToLinearGamma (SC4), and Matrix/grayscale (SC5).
#[cfg(all(test, feature = "testing"))]
mod scenebuilder_filter_chain_tests;

// ============================================================================
// RE-EXPORTS (convenience)
// ============================================================================

// Abstract traits and errors
pub use error::{EngineError, EngineResult, Recoverability};
// RasterBackend: the frame-driver swap point. The trait is unconditional;
// only the wgpu impl is feature-gated.
pub use raster::{PrePresentHook, PresentDisposition, RasterBackend};
// Raster mailbox + dedicated ack channel boundary.
pub use raster_owner::{
    FrameDropReason, PumpOutcome, RasterAck, RasterCompletion, RasterHandle, RasterOwner,
    RasterSubmitError, SurfaceState,
};
// The GPU-resource freshness stamp (ADR-0045 decision 4); declared in
// `flui-foundation` and re-exported here so `flui_engine::GpuResourceGeneration`
// keeps resolving — the compile-fail fixture for its private field cites this
// path, and `RasterAck`/`FrameStamp` compare this axis.
pub use flui_foundation::GpuResourceGeneration;
// Windowless capture (golden-image / screenshot tooling).
pub use headless::HeadlessRenderer;
pub use painter::WgpuPainter;
// Renderer (the one externally-consumed wgpu/* type)
pub use renderer::{GpuCapabilities, Renderer};
// What a windowed `Renderer` draws into (issue #1043); exported beside
// `Renderer` here at `flui_engine::wgpu`.
pub use window_target::WindowTarget;
// GPU frame profile — feature-independent type, always available so callers
// can store/display profiling results without gating on `gpu-profiler`.
pub use profiler::{GpuFrameProfile, PassTiming};

// Offscreen renderer + texture pool — re-exported ONLY under the
// `testing` feature for the `offscreen_resource_cache` criterion bench.
// Gated so benching does not widen the public surface.
#[cfg(feature = "testing")]
pub use offscreen::OffscreenRenderer;
#[cfg(feature = "testing")]
pub use texture_pool::{PooledTexture, TexturePool};
// The tessellation cache, for the `render_throughput` bench's warm-hit case.
#[cfg(feature = "testing")]
pub use path_cache::PathCache;

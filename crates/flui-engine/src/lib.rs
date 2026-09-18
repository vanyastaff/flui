// Engine crate -- many types contain wgpu handles that don't implement Debug.
// `missing_debug_implementations` is suppressed crate-wide because wgpu's
// resource handles (Device, Queue, Texture, Buffer, etc.) intentionally do
// not impl Debug (large, not human-readable), and most public types here hold
// one. It is an `expect`, not an `allow`: if the last such type leaves, the
// attribute must go with it.
#![expect(missing_debug_implementations)]
// The wgpu backend's instance batches carry a `*_batch` field per primitive
// family, and the segment enum is dominated by its largest variant; both are
// deliberate. Gated on the feature because neither type exists without it.
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
#![cfg_attr(
    feature = "wgpu-backend",
    expect(clippy::struct_field_names, clippy::large_enum_variant)
)]
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

//! FLUI Rendering Engine - GPU-accelerated rendering for FLUI
//!
//! This crate provides GPU rendering backends for FLUI. The default backend
//! uses wgpu (Vulkan/Metal/DX12/WebGPU).
//!
//! # Architecture
//!
//! ```text
//! Scene (flui-layer)
//!     │
//!     ▼
//! Renderer
//!     │ renders the LayerTree (Scene)
//!     ▼
//! Layer + LayerRender trait
//!     │ dispatch commands
//!     ▼
//! LayerDispatcher
//!     │ implements the CommandRenderer + LayerStateStack traits,
//!     │ routing each DrawCommand to the painter
//!     ▼
//! WgpuPainter → GpuReplay
//!     │ record: batched Command IR; replay: wgpu draw calls
//!     ▼
//! GPU (wgpu)
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! # async fn render(
//! #     window: impl flui_engine::wgpu::WindowTarget,
//! # ) -> Result<(), flui_engine::EngineError> {
//! use flui_engine::wgpu::Renderer;
//! use flui_layer::{Scene, CanvasLayer, Layer};
//! use flui_types::{Size, geometry::px};
//!
//! // 1. Build a Scene (in framework layer)
//! let scene = Scene::from_layer(
//!     Size::new(px(800.0), px(600.0)),
//!     Layer::Canvas(Box::new(CanvasLayer::new())),
//!     0,
//! );
//!
//! // 2. Render the Scene (in the engine layer) — `Renderer` owns per-window
//! //    GPU state, and owns its target itself (an owned, `'static` handle
//! //    source — see `flui_engine::wgpu::WindowTarget` — not a borrow).
//! //    `window` is any `WindowTarget`: usually an
//! //    `Arc<dyn PlatformWindow>`, or an owned raw-window-handle type.
//! let mut renderer = Renderer::new(window).await?;
//! renderer.render_scene(&scene)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Feature Flags
//!
//! - `wgpu` (default) - wgpu GPU backend
//! - Future: `skia`, `vello`, `software`

// Ship bar (wave 2): every public item is documented; keep it that way.
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

/// The command dispatch surface (`CommandRenderer`). Crate plumbing: the
/// layer walk dispatches through it, no embedder implements it today.
pub(crate) mod command_renderer;

/// The layer-tree state hand-off (`LayerStateStack`), the sibling of
/// `command_renderer`.
pub(crate) mod layer_state_stack;

/// `DrawCommand` dispatch functions. Crate plumbing, same as the traits.
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
#[cfg(all(test, feature = "wgpu-backend"))]
pub(crate) mod superellipse;

/// Backend-agnostic frame-driver trait ([`RasterBackend`]).
/// The trait itself is unconditional; `impl RasterBackend for Renderer`
/// is gated on the `wgpu-backend` feature.
pub mod raster;

/// The raster mailbox + dedicated ack channel boundary
/// ([`RasterOwner`]/[`RasterHandle`]/[`RasterAck`]).
/// Generic over [`RasterBackend`]; unconditional like `raster` itself.
pub mod raster_owner;

/// The font faces embedded in this crate, as bytes.
/// Unconditional: they are data, not a backend, and a caller pinning a
/// deterministic face set needs them without the wgpu stack.
pub mod fonts;

/// Portable monotonic timestamps for GPU-path diagnostic spans.
/// Acquire/present timers must not panic on wasm32 before the wgpu backend is
/// even asked for a surface texture (issue #1045).
#[cfg(feature = "wgpu-backend")]
pub(crate) mod frame_timing;

// ============================================================================
// BACKENDS
// ============================================================================

/// wgpu rendering backend (Vulkan/Metal/DX12/WebGPU)
#[cfg(feature = "wgpu-backend")]
pub mod wgpu;

// ============================================================================
// RE-EXPORTS (convenience)
// ============================================================================

// Abstract traits and errors
pub use error::{EngineError, EngineResult, Recoverability};
// RasterBackend: the frame-driver swap point. The trait is unconditional;
// only the wgpu impl is feature-gated.
pub use raster::{PrePresentHook, RasterBackend};
// Raster mailbox + dedicated ack channel boundary.
pub use raster_owner::{
    FrameDropReason, PumpOutcome, RasterAck, RasterCompletion, RasterHandle, RasterOwner,
    RasterSubmitError, SurfaceState,
};
// The GPU-resource freshness stamp (ADR-0045 decision 4); declared in
// `flui-foundation` and re-exported here so `flui_engine::GpuResourceGeneration`
// keeps resolving — the compile-fail fixture for its private field cites this
// path, and `RasterAck`/`FrameStamp` compare this axis.
#[cfg(feature = "wgpu-backend")]
pub use wgpu::GpuResourceGeneration;
// The one renderer type an embedder names; the rest of the wgpu backend
// (painter, backend, layer dispatch, shader/command plumbing) is
// `pub(crate)` — embedders drive it through `Renderer`/`RasterBackend`, and
// in-workspace callers name the concrete types through `flui_engine::wgpu`.
#[cfg(feature = "wgpu-backend")]
pub use wgpu::Renderer;
// The per-frame painter. Public because `examples/painting_demo` (a separate
// workspace crate) drives its own painter to exercise the drawing API
// directly; it is the embedder-facing half of the engine beside `Renderer`.
#[cfg(feature = "wgpu-backend")]
pub use wgpu::WgpuPainter;

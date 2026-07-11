// Engine crate -- many types contain wgpu handles that don't implement Debug.
// `missing_debug_implementations` stays suppressed because wgpu's resource
// handles (Device, Queue, Texture, Buffer, etc.) intentionally do not impl
// Debug (large, not human-readable). The `dead_code` global suppression was
// removed in Mythos U10; surviving `#[allow(dead_code)]` markers are scoped
// to specific modules where forward-looking infrastructure has named consumers
// that are not yet wired up.
#![allow(missing_debug_implementations)]
// GPU capability structs legitimately use many bools; field name postfixes
// are unavoidable when wrapping distinct pipeline/stack types.
#![allow(
    clippy::struct_excessive_bools,
    clippy::struct_field_names,
    clippy::large_enum_variant
)]

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
//! CommandRenderer trait (abstract)
//!     │
//!     ▼
//! Backend → WgpuPainter
//!     │
//!     ▼
//! GPU (wgpu)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::wgpu::Renderer;
//! use flui_layer::{Scene, CanvasLayer, Layer};
//! use flui_types::Size;
//!
//! // 1. Build a Scene (in framework layer)
//! let scene = Scene::from_layer(
//!     Size::new(800.0, 600.0),
//!     Layer::Canvas(CanvasLayer::new()),
//!     0,
//! );
//!
//! // 2. Render the Scene (in the engine layer) — `Renderer` owns per-window GPU state
//! let mut renderer = Renderer::new(&window).await?;
//! renderer.render_scene(&scene)?;
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

// wgpu-profiler 0.27 has no wasm32 support; reject the combination at compile
// time so a downstream consumer cannot accidentally enable both.
#[cfg(all(target_arch = "wasm32", feature = "gpu-profiler"))]
compile_error!(
    "the `gpu-profiler` feature (wgpu-profiler 0.27) is not supported on wasm32; \
     build without it"
);

// ============================================================================
// ABSTRACT LAYER (backend-agnostic)
// ============================================================================

/// Common error types for all rendering backends
pub mod error;

/// Abstract rendering traits (CommandRenderer, Painter)
pub mod traits;

/// RenderCommand dispatch functions
pub mod commands;

/// Backend-agnostic superellipse (iOS squircle) path generation.
/// Pure geometry — no wgpu, no lyon. Declared here, outside the
/// wgpu-backend feature gate, so `CommandRenderer`'s default impl
/// can call it without the abstract trait depending on the concrete backend.
pub(crate) mod superellipse;

/// Backend-agnostic frame-driver trait ([`RasterBackend`]).
/// The trait itself is unconditional; `impl RasterBackend for Renderer`
/// is gated on the `wgpu-backend` feature.
pub mod raster;

/// The raster mailbox + dedicated ack channel boundary
/// ([`RasterOwner`]/[`RasterHandle`]/[`RasterAck`], ADR-0027 §4/§5/§7).
/// Generic over [`RasterBackend`]; unconditional like `raster` itself.
pub mod raster_owner;

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
pub use commands::{dispatch_command, dispatch_commands};
pub use error::{EngineError, EngineResult, Recoverability};
// Re-export layer types from flui-layer
pub use flui_layer::{
    CanvasLayer, DamageRegion, Layer, LayerId, LayerTree, LinkRegistry, Scene, SceneBuilder,
    SceneCompositor, SceneSnapshot, ShaderMaskLayer,
};
// Re-export Paint from flui_painting
pub use flui_painting::Paint;
// Cycle 4 E-9: CommandRenderer trait split into render-visitor
// (CommandRenderer, ~34 methods) + layer-tree state-stack
// (LayerStateStack, 13 methods). Backends that only emit
// commands implement CommandRenderer only; compositors implement
// both. See traits.rs E-9 commentary.
pub use traits::{CommandRenderer, LayerStateStack};
// RasterBackend: the frame-driver swap point. The trait is unconditional;
// only the wgpu impl is feature-gated.
pub use raster::RasterBackend;
// Raster mailbox + dedicated ack channel boundary (ADR-0027 §4/§5/§7).
pub use raster_owner::{
    FrameDropReason, PumpOutcome, RasterAck, RasterHandle, RasterOwner, RasterSubmitError,
};
#[cfg(all(feature = "wgpu-backend", debug_assertions))]
pub use wgpu::DebugBackend;
// wgpu backend exports
#[cfg(feature = "wgpu-backend")]
pub use wgpu::{Backend, FontLoader, LayerRender, WgpuPainter};

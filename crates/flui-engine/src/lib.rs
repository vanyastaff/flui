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
//! SceneRenderer
//!     │ renders LayerTree
//!     ▼
//! Layer + LayerRender trait
//!     │ dispatch commands
//!     ▼
//! CommandRenderer trait (abstract)
//!     │
//!     ▼
//! Backend → Painter
//!     │
//!     ▼
//! GPU (wgpu)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::wgpu::SceneRenderer;
//! use flui_layer::{Scene, SceneBuilder, CanvasLayer, Layer};
//! use flui_types::Size;
//!
//! // 1. Build a Scene (in framework layer)
//! let scene = Scene::from_layer(
//!     Size::new(800.0, 600.0),
//!     Layer::Canvas(CanvasLayer::new()),
//!     0,
//! );
//!
//! // 2. Render Scene (in engine layer)
//! let mut renderer = SceneRenderer::new(surface, 800, 600);
//! renderer.render_scene(&scene)?;
//! ```
//!
//! # Feature Flags
//!
//! - `wgpu` (default) - wgpu GPU backend
//! - Future: `skia`, `vello`, `software`

// Compile-time guard: the `fragile-send-sync-non-atomic-wasm` wgpu feature
// marks !Send types as Send+Sync, which is only sound when the wasm target has
// no threads (no atomics target feature). Enabling atomics with this feature
// is UB — catch it at compile time instead of silently producing data races.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
compile_error!(
    "fragile-send-sync-non-atomic-wasm is unsound with threads/atomics enabled \
     — see flui-engine Cargo.toml [target.wasm32] note"
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
pub use error::{EngineError, EngineResult};
// Re-export layer types from flui-layer
pub use flui_layer::{
    CanvasLayer, Layer, LayerId, LayerTree, LinkRegistry, Scene, SceneBuilder, SceneCompositor,
    ShaderMaskLayer,
};
// Re-export Paint from flui_painting
pub use flui_painting::Paint;
// Cycle 4 E-9: CommandRenderer trait split into render-visitor
// (CommandRenderer, ~34 methods) + layer-tree state-stack
// (LayerStateStack, 13 methods). Backends that only emit
// commands implement CommandRenderer only; compositors implement
// both. See traits.rs E-9 commentary.
pub use traits::{CommandRenderer, LayerStateStack};
#[cfg(all(feature = "wgpu-backend", debug_assertions))]
pub use wgpu::DebugBackend;
// wgpu backend exports
#[cfg(feature = "wgpu-backend")]
pub use wgpu::{Backend, FontLoader, LayerRender, WgpuPainter};

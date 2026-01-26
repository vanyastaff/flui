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

// ============================================================================
// Platform-Specific Modules
// ============================================================================

#[cfg(target_os = "android")]
pub mod android;

// ============================================================================
// ABSTRACT LAYER (backend-agnostic)
// ============================================================================

/// Common error types for all rendering backends
pub mod error;

/// Abstract rendering traits (CommandRenderer, Painter)
pub mod traits;

/// RenderCommand dispatch functions
pub mod commands;

/// Utility modules (vector text, etc.)
pub mod utils;

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
pub use error::{RenderError, RenderResult};
pub use traits::{CommandRenderer, Painter};

// wgpu backend exports
#[cfg(feature = "wgpu-backend")]
pub use wgpu::{Backend, LayerRender, WgpuPainter};

#[cfg(all(feature = "wgpu-backend", debug_assertions))]
pub use wgpu::DebugBackend;

// Re-export layer types from flui-layer
pub use flui_layer::{
    CanvasLayer, Layer, LayerId, LayerTree, LinkRegistry, Scene, SceneBuilder, SceneCompositor,
    ShaderMaskLayer,
};

// Re-export Paint from flui_painting
pub use flui_painting::Paint;

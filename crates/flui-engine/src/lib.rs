#![allow(dead_code, missing_debug_implementations)]

//! FLUI Rendering Engine — GPU-accelerated rendering for FLUI
//!
//! # Architecture
//!
//! ```text
//! GpuDevice (shared: device, queue, pipelines, caches)
//!     │
//!     ▼
//! RenderSurface (per-window: surface, config, viewport)
//!     │ begin_frame()
//!     ▼
//! FrameEncoder (per-frame: batchers, state stacks)
//!     │ render_scene() / batchers_mut()
//!     ▼
//! Layer traversal → DrawCommand dispatch → Batchers → GPU submit
//! ```
//!
//! # Modules
//!
//! - [`context`] — GPU device, surface, and capability management
//! - [`frame`] — Per-frame state, encoding, and submission
//! - [`batchers`] — Draw call batching by primitive type
//! - [`pipelines`] — Render pipeline creation and caching
//! - [`text`] — Text rendering subsystem (glyphon)
//! - [`resources`] — Buffer pools, texture caches, atlases
//! - [`platform`] — Platform-specific GPU optimizations
//! - [`vertex`] — Consolidated vertex and instance types
//! - [`debug`] — Debug encoder for non-GPU command tracing

/// Common error types for all rendering backends.
pub mod error;

/// Consolidated vertex and instance types for GPU rendering.
pub mod vertex;

/// GPU device, surface, and capability management.
#[cfg(feature = "wgpu-backend")]
pub mod context;

/// Per-frame rendering state and command encoding.
#[cfg(feature = "wgpu-backend")]
pub mod frame;

/// Draw call batching by primitive type.
#[cfg(feature = "wgpu-backend")]
pub mod batchers;

/// Render pipeline creation and caching.
#[cfg(feature = "wgpu-backend")]
pub mod pipelines;

/// Text rendering subsystem.
#[cfg(feature = "wgpu-backend")]
pub mod text;

/// GPU resource management (buffers, textures, atlases).
#[cfg(feature = "wgpu-backend")]
pub mod resources;

/// Platform-specific GPU backend optimizations.
pub mod platform;

/// Debug encoder for non-GPU command tracing.
pub mod debug;

// ============================================================================
// RE-EXPORTS
// ============================================================================

pub use error::{RenderError, RenderResult};

// GPU context
#[cfg(feature = "wgpu-backend")]
pub use context::capabilities::GpuCapabilities;
#[cfg(feature = "wgpu-backend")]
pub use context::gpu_device::GpuDevice;
#[cfg(feature = "wgpu-backend")]
pub use context::render_surface::RenderSurface;

// Frame encoding
#[cfg(feature = "wgpu-backend")]
pub use frame::dispatch::Batchers;
#[cfg(feature = "wgpu-backend")]
pub use frame::encoder::FrameEncoder;
#[cfg(feature = "wgpu-backend")]
pub use frame::state_stack::StateStack;
#[cfg(feature = "wgpu-backend")]
pub use frame::submission::{BatchedDraw, ScissorRect};

// Pipelines
#[cfg(feature = "wgpu-backend")]
pub use pipelines::registry::{PipelineId, PipelineRegistry};

// Debug
pub use debug::DebugEncoder;

// Layer types from flui-layer
pub use flui_layer::{
    CanvasLayer, Layer, LayerId, LayerTree, LinkRegistry, Scene, SceneBuilder, SceneCompositor,
    ShaderMaskLayer,
};

// Paint from flui-painting
pub use flui_painting::Paint;

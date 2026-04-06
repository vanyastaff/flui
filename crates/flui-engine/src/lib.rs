// Engine crate is under active development — many types contain wgpu handles
// that don't implement Debug, and many fields/constants are reserved for future
// rendering paths not yet wired up.
#![allow(dead_code, missing_debug_implementations)]

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
//! # Module Structure (new)
//!
//! - [`context`] — GPU device, surface, and capability management
//! - [`frame`] — Per-frame state, encoding, and submission
//! - [`batchers`] — Draw call batching by primitive type
//! - [`pipelines`] — Render pipeline creation and caching
//! - [`text`] — Text rendering subsystem
//! - [`resources`] — Buffer pools, texture caches, atlases
//! - [`platform`] — Platform-specific GPU optimizations
//! - [`vertex`] — Consolidated vertex and instance types
//!
//! # Legacy Module
//!
//! The [`wgpu`] module contains the original `WgpuPainter` implementation.
//! It will be removed once the migration to the new module structure is complete.

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
// NEW MODULE STRUCTURE (migration in progress)
// ============================================================================

/// GPU device, surface, and capability management
pub mod context;

/// Per-frame rendering state and command encoding
pub mod frame;

/// Draw call batching by primitive type
pub mod batchers;

/// Render pipeline creation and caching
pub mod pipelines;

/// Text rendering subsystem
pub mod text;

/// GPU resource management (buffers, textures, atlases)
pub mod resources;

/// Platform-specific GPU backend optimizations
pub mod platform;

/// Consolidated vertex and instance types for GPU rendering
pub mod vertex;

/// Debug encoder for non-GPU command tracing
pub mod debug;

// ============================================================================
// LEGACY BACKEND (kept during migration — will be removed in Task 16)
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

// wgpu backend exports (legacy — kept during migration)
#[cfg(feature = "wgpu-backend")]
pub use wgpu::{Backend, LayerRender, WgpuPainter};

#[cfg(all(feature = "wgpu-backend", debug_assertions))]
pub use wgpu::DebugBackend;

// New public API — GPU context
#[cfg(feature = "wgpu-backend")]
pub use context::gpu_device::GpuDevice;
#[cfg(feature = "wgpu-backend")]
pub use context::render_surface::RenderSurface;
#[cfg(feature = "wgpu-backend")]
pub use context::capabilities::GpuCapabilities;

// New public API — frame encoding and submission
#[cfg(feature = "wgpu-backend")]
pub use frame::encoder::FrameEncoder;
#[cfg(feature = "wgpu-backend")]
pub use frame::dispatch::Batchers;
#[cfg(feature = "wgpu-backend")]
pub use frame::state_stack::StateStack;
#[cfg(feature = "wgpu-backend")]
pub use frame::submission::{BatchedDraw, ScissorRect};

// New public API — pipeline registry
#[cfg(feature = "wgpu-backend")]
pub use pipelines::registry::{PipelineId, PipelineRegistry};

// New public API — debug encoder (no GPU required)
pub use debug::DebugEncoder;

// Re-export layer types from flui-layer
pub use flui_layer::{
    CanvasLayer, Layer, LayerId, LayerTree, LinkRegistry, Scene, SceneBuilder, SceneCompositor,
    ShaderMaskLayer,
};

// Re-export Paint from flui_painting
pub use flui_painting::Paint;

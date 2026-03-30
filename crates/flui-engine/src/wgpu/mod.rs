//! wgpu rendering backend for FLUI
//!
//! This module provides GPU-accelerated rendering using wgpu
//! (Vulkan/Metal/DX12/WebGPU).
//!
//! # Architecture
//!
//! ```text
//! Scene (flui-layer)
//!     │
//!     ▼
//! SceneRenderer (scene.rs)
//!     │ renders LayerTree
//!     ▼
//! Layer + LayerRender trait
//!     │ dispatch commands
//!     ▼
//! CommandRenderer trait (crate::traits)
//!     │
//!     ▼
//! Backend (backend.rs) → WgpuPainter (painter.rs)
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
//! use flui_engine::wgpu::{SceneRenderer, WgpuPainter};
//! use flui_engine::Painter;  // Abstract trait from crate root
//! use flui_layer::Scene;
//!
//! // Create renderer
//! let mut renderer = SceneRenderer::new(surface, 800, 600);
//!
//! // Render a scene
//! renderer.render(&scene)?;
//! ```

// ============================================================================
// CORE MODULES
// ============================================================================

mod atlas;
mod backend;
mod buffer_pool;
mod buffers;
mod commands;
mod compositor;
#[cfg(debug_assertions)]
mod debug;
#[cfg(target_os = "windows")]
pub mod dx12;
#[allow(dead_code)]
mod effects;
mod effects_pipeline;
mod external_texture_registry;
#[allow(dead_code)]
mod instancing;
// NOTE: integration_tests.rs removed - needs rewrite for new
// Pixels/DevicePixels API
#[cfg(target_os = "macos")]
pub mod metal;
mod multi_draw;
mod offscreen;
mod painter;
#[allow(dead_code)]
mod pipeline;
mod pipelines;
mod renderer;
mod scene;
#[allow(dead_code)]
mod shader_compiler;
mod shaders;
mod tessellator;
mod text;
mod text_renderer;
pub mod texture_cache;
mod texture_pool;
mod vertex;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod vulkan;

// ============================================================================
// LAYER RENDERING
// ============================================================================

mod layer_render;

// ============================================================================
// PUBLIC API
// ============================================================================

// Scene types
// Texture atlas
pub use atlas::{AtlasEntry, AtlasRect, TextureAtlas};
// Backend
pub use backend::Backend;
// Buffer management
pub use buffer_pool::{BufferPool, BufferPoolStats};
pub use buffers::{BufferManager, DynamicBuffer};
// Command rendering (re-exported from crate root)
pub use commands::{CommandRenderer, dispatch_command, dispatch_commands};
// Compositor
pub use compositor::{Compositor, RenderContext, TransformStack};
#[cfg(debug_assertions)]
pub use debug::DebugBackend;
// External texture registry
pub use external_texture_registry::{ExternalTextureEntry, ExternalTextureRegistry};
// Layer rendering
pub use layer_render::LayerRender;
// Multi-draw indirect batching
pub use multi_draw::{DrawCommand, DrawIndexedIndirectArgs, MultiDrawBatcher, MultiDrawStats, PipelineId};
// Offscreen rendering
pub use offscreen::{MaskedRenderResult, OffscreenRenderer, PipelineManager};
pub use painter::WgpuPainter;
// Pipeline management
pub use pipelines::{PipelineBuilder, PipelineCache};
// Renderer (cross-platform GPU renderer)
pub use renderer::{GpuCapabilities, Renderer};
pub use scene::{Scene, SceneBuilder};
// Shader compilation
pub use shader_compiler::{ShaderCache, ShaderType};
// Tessellator
pub use tessellator::Tessellator;
// Text rendering (feature-gated)
#[cfg(feature = "wgpu-backend")]
pub use text_renderer::{TextRenderingSystem, TextRun};
// Texture pool
pub use texture_pool::{GpuTexture, PooledTexture, PoolStats, TextureDesc, TexturePool};
// Vertex types
pub use vertex::{ImageInstance, PathVertex, RectInstance, RectVertex, Vertex};

// Painter (WgpuPainter is the concrete implementation, Painter trait from crate::traits)
pub use crate::traits::Painter;

//! wgpu rendering backend for FLUI
//!
//! This module provides GPU-accelerated rendering using wgpu (Vulkan/Metal/DX12/WebGPU).
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
#[allow(dead_code)]
mod buffer_pool;
mod buffers;
mod commands;
mod compositor;
#[cfg(debug_assertions)]
mod debug;
#[cfg(test)]
mod integration_tests;
#[allow(dead_code)]
mod effects;
mod effects_pipeline;
mod external_texture_registry;
#[allow(dead_code)]
mod instancing;
#[allow(dead_code)]
mod multi_draw;
mod offscreen;
mod painter;
#[allow(dead_code)]
mod pipeline;
mod pipelines;
mod scene;
#[allow(dead_code)]
mod shader_compiler;
mod tessellator;
mod text;
mod text_renderer;
pub mod texture_cache;
mod texture_pool;
mod vertex;

// ============================================================================
// LAYER RENDERING
// ============================================================================

mod layer_render;

// ============================================================================
// PUBLIC API
// ============================================================================

// Scene rendering
pub use scene::SceneRenderer;

// Layer rendering
pub use layer_render::LayerRender;

// Command rendering (re-exported from crate root)
pub use commands::{dispatch_command, dispatch_commands, CommandRenderer};

// Backend
pub use backend::Backend;
#[cfg(debug_assertions)]
pub use debug::DebugBackend;

// Painter (WgpuPainter is the concrete implementation, Painter trait from crate::traits)
pub use crate::traits::Painter;
pub use painter::WgpuPainter;

// Vertex types
pub use vertex::{ImageInstance, PathVertex, RectInstance, RectVertex};

// Tessellator
pub use tessellator::Tessellator;

// External texture registry
pub use external_texture_registry::{ExternalTextureEntry, ExternalTextureRegistry};

// Offscreen rendering
pub use offscreen::{MaskedRenderResult, OffscreenRenderer, PipelineManager};

// Shader compilation
pub use shader_compiler::{ShaderCache, ShaderType};

// Texture pool
pub use texture_pool::{PooledTexture, TextureDesc, TexturePool};

// Buffer management
pub use buffers::{BufferManager, DynamicBuffer};

// Pipeline management
pub use pipelines::{PipelineBuilder, PipelineCache};

// Texture atlas
pub use atlas::{AtlasEntry, AtlasRect, TextureAtlas};

// Compositor
pub use compositor::{Compositor, RenderContext, TransformStack};

// Text rendering (feature-gated)
#[cfg(feature = "wgpu-backend")]
pub use text_renderer::{TextRenderingSystem, TextRun};

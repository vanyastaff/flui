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
#[cfg(debug_assertions)]
mod debug;
/// Gradient + shadow + blur instance descriptors consumed by `painter.rs`'s
/// instanced-batch pipelines. `#[allow(dead_code)]` retained at the module
/// level because several builder/constant items (`ShadowParams::elevation_*`,
/// `BlurIntensity`, `LinearGradientBuilder`) are forward-looking helpers that
/// painter.rs has not yet wired into a public API; deletion would be premature
/// before painter.rs's internal cleanup.
#[allow(dead_code)]
pub mod effects;
mod effects_pipeline;
mod external_texture_registry;
pub mod font_loader;
/// GPU instance-buffer types: `RectInstance`, `CircleInstance`, `ArcInstance`,
/// `TextureInstance`, gradient instances. Most variants are consumed by
/// `painter.rs`. `#[allow(dead_code)]` retained because several constructor
/// shortcuts (`rounded_rect`, `with_transform`, `ellipse`, `with_rotation`)
/// are forward-looking helpers not yet wired into painter.rs's public surface;
/// the per-item audit + deletion is tracked in
/// `crates/flui-engine/ARCHITECTURE.md` `## Outstanding refactors`.
#[allow(dead_code)]
mod instancing;
// NOTE: integration_tests.rs removed - needs rewrite for new
// Pixels/DevicePixels API
mod multi_draw;
pub mod occlusion;
mod offscreen;
mod painter;
pub mod path_cache;
/// Pipeline key types + descriptors consumed by `painter.rs`'s pipeline cache.
/// `#[allow(dead_code)]` retained because the `PipelineKey::from_color` /
/// related constructors are not yet wired into painter.rs's pipeline
/// construction. Per-item audit tracked in ARCHITECTURE.md.
#[allow(dead_code)]
mod pipeline;
mod pipelines;
mod renderer;
/// Shader cache for offscreen pipelines (`OffscreenRenderer` mask/blur/morph).
/// `#[allow(dead_code)]` retained because `ShaderCache::cached_count` and
/// `ShaderCache::clear` introspection methods are forward-looking devtools
/// helpers; deletion is tracked in ARCHITECTURE.md Outstanding refactors.
#[allow(dead_code)]
mod shader_compiler;
mod shaders;
pub mod superellipse_cache;
mod tessellator;
mod text;
pub mod texture_cache;
mod texture_pool;
mod vertex;

// ============================================================================
// LAYER RENDERING
// ============================================================================

pub(crate) mod layer_render;

#[cfg(test)]
mod sdf_smoke_test;

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
pub use crate::{
    commands::{dispatch_command, dispatch_commands},
    traits::CommandRenderer,
};
#[cfg(debug_assertions)]
pub use debug::DebugBackend;
// External texture registry
pub use external_texture_registry::{ExternalTextureEntry, ExternalTextureRegistry};
// Layer rendering
pub use layer_render::LayerRender;
// Multi-draw indirect batching
pub use multi_draw::{
    DrawCommand, DrawIndexedIndirectArgs, MultiDrawBatcher, MultiDrawStats, PipelineId,
};
// Offscreen rendering
pub use offscreen::{MaskedRenderResult, OffscreenRenderer, PipelineManager};
pub use painter::WgpuPainter;
// Pipeline management
pub use pipelines::{PipelineBuilder, PipelineCache};
// Renderer (cross-platform GPU renderer)
pub use renderer::{GpuCapabilities, Renderer};
// Shader compilation
pub use shader_compiler::{ShaderCache, ShaderType};
// Tessellator
pub use tessellator::Tessellator;
// Texture pool
pub use texture_pool::{GpuTexture, PoolStats, PooledTexture, TextureDesc, TexturePool};
// Vertex types
pub use vertex::{ImageInstance, PathVertex, RectInstance, RectVertex, Vertex};

// Font loading utilities
pub use font_loader::FontLoader;

// Painter (WgpuPainter is the concrete implementation; Painter trait deleted in Mythos U5)

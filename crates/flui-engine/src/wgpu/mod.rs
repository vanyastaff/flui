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
/// instanced-batch pipelines. The previous module-level `#[allow(dead_code)]`
/// reflex was removed in cycle 4 E-4 alongside the forward-looking helpers
/// (`ShadowParams::elevation_*`, `BlurIntensity`, `LinearGradientBuilder`,
/// the parallel `effects::BlurParams`); any zombie that returns lands as an
/// item-level lint, not a broad module suppression.
pub mod effects;
mod effects_pipeline;
mod external_texture_registry;
pub mod font_loader;
/// GPU instance-buffer types: `RectInstance`, `CircleInstance`,
/// `ArcInstance`, `TextureInstance`, gradient instances. All
/// surviving items are consumed by `painter.rs`. Cycle 4 E-5
/// deleted the 6 forward-looking shortcuts the previous
/// module-level `#[allow(dead_code)]` was masking (`RectInstance::rounded_rect`,
/// `RectInstance::with_clip_rsuperellipse`, `RectInstance::with_transform`,
/// `CircleInstance::ellipse`, `ArcInstance::ellipse`,
/// `TextureInstance::with_rotation`); any zombie that returns now
/// surfaces as an item-level lint, not a broad module suppression.
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
// Cycle 4 E-6: parallel `pipelines.rs` module (with its own
// `PipelineCache` + `PipelineBuilder` structs, name-colliding with
// `pipeline.rs`) was deleted. Workspace grep showed zero non-self
// consumers of `pipelines::PipelineCache` / `pipelines::PipelineBuilder`
// outside the re-export line below; `painter.rs:22` imports from
// `pipeline` (singular), which is the live version. The audit
// (R-7 entry E-6) had the singular/plural mapping reversed; the
// outcome is the same — one of the two parallel modules dies.
mod renderer;
/// Shader cache for offscreen pipelines (`OffscreenRenderer` mask /
/// blur / morph). Cycle 4 E-7 dropped the module-level
/// `#[allow(dead_code)]` mask: the only forward-looking helper
/// (`ShaderCache::clear`) is now gated behind
/// `#[cfg(feature = "devtools")]`, so default-build dead-code
/// surfaces as an item-level lint rather than a broad module
/// suppression. The audit also mentioned `cached_count` but no such
/// method existed -- only `clear`.
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
// Offscreen rendering. `PipelineManager` was deleted in cycle 4 E-3
// (zombie wrapper carrying only an `Arc<ShaderCache>` field with 0
// consumers); the real pipeline ownership lives in
// `pipeline::PipelineCache` (singular -- the parallel `pipelines.rs`
// plural module with its own competing `PipelineCache` was deleted in
// cycle 4 E-6 with zero workspace consumers).
pub use offscreen::{MaskedRenderResult, OffscreenRenderer};
pub use painter::WgpuPainter;
// Pipeline cache (cycle 4 E-6: re-export now points at the live
// singular-name `pipeline::` module; the parallel `pipelines.rs`
// plural module was deleted as zero-consumer dead code).
pub use pipeline::PipelineCache;

// Cycle 4 PR #112 review fix: deprecated migration shim for
// `PipelineBuilder`, which was re-exported from the deleted
// `pipelines.rs` (plural) module pre-cycle. Workspace grep showed
// zero consumers; if a downstream consumer DID import the name,
// they hit a `#[deprecated]` warning rather than an unresolved-symbol
// error. The shim resolves to the unit type so any method call on it
// will fail compilation immediately -- the type's role was to host
// builder methods that no longer exist. One-cycle migration window
// before the alias goes away in cycle 5.
/// Deprecated migration shim — see cycle 4 E-6 / PR #112 review.
#[deprecated(
    since = "0.1.0",
    note = "`PipelineBuilder` (from the deleted `wgpu::pipelines` \
            module) had zero workspace consumers and was removed in \
            cycle 4 E-6. There is no replacement; pipeline construction \
            goes through `pipeline::PipelineCache` (singular) directly. \
            This deprecated alias will be removed in cycle 5."
)]
pub type PipelineBuilder = ();
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

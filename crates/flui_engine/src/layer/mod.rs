//! Layer system - Compositor layers for advanced effects
//!
//! This module provides layer types for the compositor:
//! - **CanvasLayer**: Standard canvas drawing commands
//! - **ShaderMaskLayer**: GPU shader masking effects (gradient fades, vignettes)
//! - **BackdropFilterLayer**: Backdrop filtering effects (frosted glass, blur)
//! - **CachedLayer**: Cached layer for RepaintBoundary optimization
//!
//! ## Architecture
//!
//! ```text
//! RenderObject (flui_rendering)
//!     |
//!     | paint() generates Canvas OR pushes Layer
//!     v
//! Layer (flui-layer crate - core types)
//!     |
//!     | LayerRender trait (this module - GPU rendering)
//!     v
//! GPU Rendering (wgpu)
//! ```
//!
//! ## Re-exports from flui-layer
//!
//! Core layer types are provided by the `flui-layer` crate:
//! - `Layer` enum with Canvas, ShaderMask, BackdropFilter, Cached variants
//! - `CanvasLayer`, `ShaderMaskLayer`, `BackdropFilterLayer`, `CachedLayer`
//! - `LayerBounds` trait
//!
//! ## Engine-specific Extensions
//!
//! This module adds GPU rendering capabilities:
//! - `LayerRender` trait for rendering layers via CommandRenderer
//! - GPU infrastructure: OffscreenRenderer, ShaderCompiler, TexturePool

// GPU-specific modules (stay in flui_engine)
pub mod offscreen_renderer;
pub mod shader_compiler;
pub mod texture_pool;

// Re-export core layer types from flui-layer
pub use flui_layer::{
    BackdropFilterLayer, CachedLayer, CanvasLayer, Layer, LayerBounds, ShaderMaskLayer,
};

// Re-export GPU infrastructure
pub use offscreen_renderer::{MaskedRenderResult, OffscreenRenderer, PipelineManager};
pub use shader_compiler::{ShaderCache, ShaderType};
pub use texture_pool::{PooledTexture, TextureDesc, TexturePool};

use crate::renderer::CommandRenderer;
use flui_painting::DisplayListCore;

// ============================================================================
// LAYER RENDER TRAIT - GPU rendering extension
// ============================================================================

/// Extension trait for rendering layers via CommandRenderer.
///
/// This trait adds GPU rendering capabilities to the core layer types
/// from flui-layer.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::layer::{Layer, LayerRender};
/// use flui_engine::renderer::WgpuRenderer;
///
/// let layer = Layer::Canvas(CanvasLayer::new());
/// layer.render(&mut renderer);
/// ```
pub trait LayerRender {
    /// Render this layer using the provided command renderer.
    fn render(&self, renderer: &mut dyn CommandRenderer);
}

impl LayerRender for Layer {
    fn render(&self, renderer: &mut dyn CommandRenderer) {
        match self {
            Layer::Canvas(canvas_layer) => canvas_layer.render(renderer),
            Layer::ShaderMask(shader_mask_layer) => shader_mask_layer.render(renderer),
            Layer::BackdropFilter(backdrop_filter_layer) => backdrop_filter_layer.render(renderer),
            Layer::Cached(cached_layer) => cached_layer.render(renderer),
        }
    }
}

impl LayerRender for CanvasLayer {
    /// Render canvas layer using the command renderer (visitor pattern).
    ///
    /// Dispatches all drawing commands from the display list to the renderer.
    fn render(&self, renderer: &mut dyn CommandRenderer) {
        use crate::renderer::dispatch_commands;

        // Use visitor pattern dispatcher for clean separation of concerns
        dispatch_commands(self.display_list().commands(), renderer);
    }
}

impl LayerRender for ShaderMaskLayer {
    /// Render shader mask layer.
    ///
    /// TODO: Implement actual GPU rendering in Phase 1.3
    /// - Allocate offscreen texture
    /// - Render child to texture
    /// - Apply shader mask via GPU
    /// - Composite to framebuffer
    fn render(&self, _renderer: &mut dyn CommandRenderer) {
        tracing::warn!(
            "ShaderMaskLayer::render() called but not yet implemented (Phase 1.3 pending)"
        );
    }
}

impl LayerRender for BackdropFilterLayer {
    /// Render backdrop filter layer.
    ///
    /// TODO: Implement actual GPU rendering in Phase 2.3
    /// - Capture framebuffer in bounds
    /// - Apply image filter via GPU compute shader
    /// - Render filtered result to framebuffer
    /// - Render child content on top (if present)
    fn render(&self, _renderer: &mut dyn CommandRenderer) {
        tracing::warn!(
            "BackdropFilterLayer::render() called but not yet implemented (Phase 2.3 pending)"
        );
    }
}

impl LayerRender for CachedLayer {
    /// Render cached layer.
    ///
    /// If dirty, renders the wrapped layer. Otherwise, reuses cached result.
    ///
    /// Note: Current implementation always renders since we don't have
    /// GPU-level caching infrastructure yet. The dirty flag is tracked
    /// for future optimization.
    fn render(&self, renderer: &mut dyn CommandRenderer) {
        // Note: Full caching optimization requires:
        // - GPU texture cache for rendered content
        // - Render target pooling
        // - Texture atlas management
        //
        // For now, we always render but track dirty state for when
        // the infrastructure is available.

        let inner = self.inner();
        inner.render(renderer);

        // Mark as clean after rendering
        self.mark_clean();
    }
}

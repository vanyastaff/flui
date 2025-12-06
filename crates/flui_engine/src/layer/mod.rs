//! Layer system - Compositor layers for advanced effects
//!
//! This module provides layer types for the compositor:
//! - **CanvasLayer**: Standard canvas drawing commands
//! - **ShaderMaskLayer**: GPU shader masking effects (gradient fades, vignettes)
//! - **BackdropFilterLayer**: Backdrop filtering effects (frosted glass, blur) - Coming in Phase 2
//!
//! Most layer effects (Transform, Opacity, Clip, etc.) are still implemented
//! as RenderObjects in flui_rendering. Only effects requiring compositor-level
//! support (offscreen rendering, backdrop access) use layers.
//!
//! ## Architecture
//!
//! ```text
//! RenderObject (flui_rendering)
//!     |
//!     | paint() generates Canvas OR pushes Layer
//!     v
//! Layer (flui_engine - this module)
//!     |
//!     | render() → CommandRenderer
//!     v
//! GPU Rendering (wgpu)
//! ```
//!
//! ## Layer Types
//!
//! - **CanvasLayer**: Contains Canvas → DisplayList → DrawCommands
//! - **ShaderMaskLayer**: Child → Offscreen Texture → Shader Mask → Composite
//! - **BackdropFilterLayer**: Capture Backdrop → Filter → Render
//!
//! ## Rendering Path
//!
//! - **CommandRenderer**: Visitor pattern for canvas drawing commands
//! - **WgpuRenderer**: GPU-accelerated rendering (handles both canvas and layers)
//!
//! All layer composition is handled by the paint pipeline in flui_core.

pub mod backdrop_filter;
pub mod cached;
pub mod offscreen_renderer;
pub mod picture;
pub mod shader_compiler;
pub mod shader_mask;
pub mod texture_pool;

pub use backdrop_filter::BackdropFilterLayer;
pub use cached::CachedLayer;
pub use offscreen_renderer::{MaskedRenderResult, OffscreenRenderer, PipelineManager};
pub use picture::CanvasLayer;
pub use shader_compiler::{ShaderCache, ShaderType};
pub use shader_mask::ShaderMaskLayer;
pub use texture_pool::{PooledTexture, TextureDesc, TexturePool};

use crate::renderer::CommandRenderer;

/// Compositor layer - polymorphic layer types for advanced rendering
///
/// This enum provides a type-safe wrapper for different layer types,
/// enabling clean dispatch without requiring trait objects.
///
/// # Architecture
///
/// ```text
/// Layer (enum)
///   ├─ Canvas(CanvasLayer)          - Standard drawing commands
///   ├─ ShaderMask(ShaderMaskLayer)  - GPU shader masking
///   ├─ BackdropFilter(...)          - Backdrop filtering
///   └─ Cached(CachedLayer)          - Cached layer for RepaintBoundary
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::{Layer, CanvasLayer};
///
/// let canvas_layer = CanvasLayer::new();
/// let layer = Layer::Canvas(canvas_layer);
///
/// // Polymorphic rendering
/// layer.render(&mut renderer);
/// ```
#[derive(Debug)]
pub enum Layer {
    /// Canvas layer - standard drawing commands
    Canvas(CanvasLayer),

    /// Shader mask layer - GPU shader masking effects
    ShaderMask(ShaderMaskLayer),

    /// Backdrop filter layer - backdrop filtering effects (frosted glass, blur)
    BackdropFilter(BackdropFilterLayer),

    /// Cached layer - optimized layer caching for RepaintBoundary
    Cached(CachedLayer),
}

impl Layer {
    /// Render this layer using the provided renderer
    ///
    /// Dispatches to the appropriate rendering method based on layer type.
    ///
    /// # Architecture
    ///
    /// - **CanvasLayer**: Renders via CommandRenderer (visitor pattern)
    /// - **ShaderMaskLayer**: Renders via OffscreenRenderer (GPU pipeline)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut renderer = WgpuRenderer::new(painter);
    /// layer.render(&mut renderer);
    /// ```
    pub fn render(&self, renderer: &mut dyn CommandRenderer) {
        match self {
            Layer::Canvas(canvas_layer) => {
                canvas_layer.render(renderer);
            }
            Layer::ShaderMask(shader_mask_layer) => {
                shader_mask_layer.render(renderer);
            }
            Layer::BackdropFilter(backdrop_filter_layer) => {
                backdrop_filter_layer.render(renderer);
            }
            Layer::Cached(cached_layer) => {
                cached_layer.render(renderer);
            }
        }
    }
}

impl From<CanvasLayer> for Layer {
    fn from(canvas: CanvasLayer) -> Self {
        Layer::Canvas(canvas)
    }
}

impl From<ShaderMaskLayer> for Layer {
    fn from(shader_mask: ShaderMaskLayer) -> Self {
        Layer::ShaderMask(shader_mask)
    }
}

impl From<BackdropFilterLayer> for Layer {
    fn from(backdrop_filter: BackdropFilterLayer) -> Self {
        Layer::BackdropFilter(backdrop_filter)
    }
}

impl From<CachedLayer> for Layer {
    fn from(cached: CachedLayer) -> Self {
        Layer::Cached(cached)
    }
}

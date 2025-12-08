//! Layer types - Compositor layers for advanced effects
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
//!     │
//!     │ paint() generates Canvas OR pushes Layer
//!     ▼
//! Layer (this module)
//!     │
//!     │ render() → CommandRenderer (in flui_engine)
//!     ▼
//! GPU Rendering (wgpu)
//! ```
//!
//! ## Layer Types
//!
//! - **CanvasLayer**: Contains Canvas → DisplayList → DrawCommands
//! - **ShaderMaskLayer**: Child → Offscreen Texture → Shader Mask → Composite
//! - **BackdropFilterLayer**: Capture Backdrop → Filter → Render
//! - **CachedLayer**: Wraps another layer with dirty tracking for caching

mod backdrop_filter;
mod cached;
mod canvas;
mod shader_mask;

pub use backdrop_filter::BackdropFilterLayer;
pub use cached::CachedLayer;
pub use canvas::CanvasLayer;
pub use shader_mask::ShaderMaskLayer;

use flui_types::geometry::Rect;

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
/// ```rust
/// use flui_layer::{Layer, CanvasLayer};
///
/// let canvas_layer = CanvasLayer::new();
/// let layer = Layer::Canvas(canvas_layer);
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
    /// Returns the bounds of this layer.
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            Layer::Canvas(canvas) => Some(canvas.bounds()),
            Layer::ShaderMask(shader_mask) => Some(shader_mask.bounds()),
            Layer::BackdropFilter(backdrop) => Some(backdrop.bounds()),
            Layer::Cached(cached) => cached.bounds(),
        }
    }

    /// Returns true if this layer needs compositing.
    pub fn needs_compositing(&self) -> bool {
        match self {
            Layer::Canvas(_) => false,
            Layer::ShaderMask(_) => true,
            Layer::BackdropFilter(_) => true,
            Layer::Cached(cached) => cached.is_dirty(),
        }
    }

    /// Returns true if this is a canvas layer.
    pub fn is_canvas(&self) -> bool {
        matches!(self, Layer::Canvas(_))
    }

    /// Returns true if this is a shader mask layer.
    pub fn is_shader_mask(&self) -> bool {
        matches!(self, Layer::ShaderMask(_))
    }

    /// Returns true if this is a backdrop filter layer.
    pub fn is_backdrop_filter(&self) -> bool {
        matches!(self, Layer::BackdropFilter(_))
    }

    /// Returns true if this is a cached layer.
    pub fn is_cached(&self) -> bool {
        matches!(self, Layer::Cached(_))
    }

    /// Returns the canvas layer if this is one.
    pub fn as_canvas(&self) -> Option<&CanvasLayer> {
        match self {
            Layer::Canvas(canvas) => Some(canvas),
            _ => None,
        }
    }

    /// Returns the canvas layer mutably if this is one.
    pub fn as_canvas_mut(&mut self) -> Option<&mut CanvasLayer> {
        match self {
            Layer::Canvas(canvas) => Some(canvas),
            _ => None,
        }
    }

    /// Returns the shader mask layer if this is one.
    pub fn as_shader_mask(&self) -> Option<&ShaderMaskLayer> {
        match self {
            Layer::ShaderMask(shader_mask) => Some(shader_mask),
            _ => None,
        }
    }

    /// Returns the backdrop filter layer if this is one.
    pub fn as_backdrop_filter(&self) -> Option<&BackdropFilterLayer> {
        match self {
            Layer::BackdropFilter(backdrop) => Some(backdrop),
            _ => None,
        }
    }

    /// Returns the cached layer if this is one.
    pub fn as_cached(&self) -> Option<&CachedLayer> {
        match self {
            Layer::Cached(cached) => Some(cached),
            _ => None,
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

// ============================================================================
// LAYER BOUNDS TRAIT
// ============================================================================

/// Trait for layers that have bounds.
pub trait LayerBounds {
    /// Returns the bounding rectangle of this layer.
    fn bounds(&self) -> Rect;
}

impl LayerBounds for CanvasLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for ShaderMaskLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

impl LayerBounds for BackdropFilterLayer {
    fn bounds(&self) -> Rect {
        self.bounds()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_from_canvas() {
        let canvas = CanvasLayer::new();
        let layer = Layer::from(canvas);
        assert!(layer.is_canvas());
        assert!(!layer.is_shader_mask());
    }

    #[test]
    fn test_layer_needs_compositing() {
        let canvas = CanvasLayer::new();
        let layer = Layer::Canvas(canvas);
        assert!(!layer.needs_compositing());
    }

    #[test]
    fn test_layer_as_canvas() {
        let canvas = CanvasLayer::new();
        let layer = Layer::Canvas(canvas);

        assert!(layer.as_canvas().is_some());
        assert!(layer.as_shader_mask().is_none());
    }
}

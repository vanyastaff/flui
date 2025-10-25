//! Layer system for composable rendering
//!
//! Layers represent visual elements in the scene graph. Unlike immediate-mode
//! rendering, layers build up a retained scene that can be:
//! - Composited with effects (opacity, transforms, etc)
//! - Cached and reused across frames
//! - Exported to different backends
//! - Analyzed for optimization (culling, batching)

use flui_types::Rect;
use crate::painter::Painter;

pub mod container;
pub mod opacity;
pub mod transform;
pub mod clip;
pub mod picture;

pub use container::ContainerLayer;
pub use opacity::OpacityLayer;
pub use transform::TransformLayer;
pub use clip::ClipLayer;
pub use picture::PictureLayer;

/// Backend-agnostic layer trait
///
/// Layers are the building blocks of the scene graph. They represent
/// visual elements that can be composed, transformed, and rendered.
///
/// # Layer Tree
///
/// ```text
/// ContainerLayer (root)
///   ├─ TransformLayer
///   │   └─ OpacityLayer
///   │       └─ PictureLayer (actual drawing)
///   └─ ClipLayer
///       └─ PictureLayer
/// ```
///
/// # Design Philosophy
///
/// - **Composable**: Layers can contain other layers
/// - **Backend Agnostic**: Layers don't know about specific rendering backends
/// - **Cacheable**: Layers can be cached and reused across frames
/// - **Analyzable**: Layer bounds enable culling and optimization
pub trait Layer: Send + Sync {
    /// Paint this layer using the given painter
    ///
    /// This is where the layer actually renders. The painter provides
    /// backend-specific drawing primitives.
    fn paint(&self, painter: &mut dyn Painter);

    /// Get the bounding box of this layer
    ///
    /// Used for culling and optimization. Layers outside the viewport
    /// don't need to be painted.
    fn bounds(&self) -> Rect;

    /// Check if this layer is visible
    ///
    /// Invisible layers can be skipped during painting.
    fn is_visible(&self) -> bool {
        true
    }
}

/// Type-erased layer (for dynamic dispatch)
pub type BoxedLayer = Box<dyn Layer>;

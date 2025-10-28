//! Layer system for composable rendering
//!
//! A composited layer represents a visual element in the scene graph.
//!
//! During painting, the render tree generates a tree of composited layers that
//! are uploaded into the engine and displayed by the compositor.
//!
//! ## Architecture
//!
//! ```text
//! RenderObject (flui_rendering)
//!     |
//!     | paint() generates
//!     v
//! Layer Tree (flui_engine - this module)
//!     |
//!     | compositor processes
//!     v
//! Scene (ui.Scene equivalent)
//!     |
//!     | rendered by
//!     v
//! Backend (egui/wgpu)
//! ```
//!
//! ## Layer Types
//!
//! - **ContainerLayer**: Base for layers that hold children
//! - **PictureLayer**: Leaf layer with actual drawing commands
//! - **TransformLayer**: Applies transformations
//! - **OpacityLayer**: Applies opacity
//! - **ClipRectLayer**: Clips to a rectangle
//! - **ClipRRectLayer**: Clips to a rounded rectangle
//!
//! ## Memory Management
//!
//! Layers retain resources between frames to speed up rendering. A layer will
//! retain these resources until all `LayerHandle`s referring to the layer have
//! been dropped.
//!
//! **IMPORTANT**: Layers must not be used after disposal.
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_engine::layer::{LayerHandle, ClipRectLayer};
//!
//! struct ClippingRenderObject {
//!     clip_layer: LayerHandle<ClipRectLayer>,
//! }
//!
//! impl ClippingRenderObject {
//!     fn paint(&mut self, context: &mut PaintingContext, offset: Offset) {
//!         let layer = context.push_clip_rect(
//!             self.needs_compositing,
//!             offset,
//!             Offset::ZERO & self.size,
//!             |painter| self.paint_children(painter),
//!             old_layer: self.clip_layer.take(),
//!         );
//!         self.clip_layer.set(Some(layer));
//!     }
//!
//!     fn dispose(&mut self) {
//!         self.clip_layer.clear(); // Release resources
//!     }
//! }
//! ```

use flui_types::Rect;
use crate::painter::Painter;

// Core layer infrastructure
pub mod base;
pub mod handle;

// Layer implementations
pub mod container;
pub mod opacity;
pub mod transform;
pub mod clip;
pub mod picture;

// Re-export core types
pub use base::{Layer as AbstractLayer, AnyLayer, LayerState};
pub use handle::LayerHandle;

// Re-export layer implementations
pub use container::ContainerLayer;
pub use opacity::OpacityLayer;
pub use transform::TransformLayer;
pub use clip::ClipLayer;
pub use picture::{PictureLayer, DrawCommand};

// Legacy trait for backward compatibility
/// Backend-agnostic layer trait (legacy)
///
/// **Note**: This is the old trait. New code should use `AbstractLayer` from `base.rs`
/// which includes proper lifecycle management.
///
/// Layers are the building blocks of the scene graph. They represent
/// visual elements that can be composed, transformed, and rendered.
pub trait Layer: Send + Sync {
    /// Paint this layer using the given painter
    fn paint(&self, painter: &mut dyn Painter);

    /// Get the bounding box of this layer
    fn bounds(&self) -> Rect;

    /// Check if this layer is visible
    fn is_visible(&self) -> bool {
        true
    }
}

/// Type-erased layer (for dynamic dispatch)
///
/// **Deprecated**: Use `AnyLayer` or `LayerHandle<T>` instead for better resource management.
pub type BoxedLayer = Box<dyn Layer>;

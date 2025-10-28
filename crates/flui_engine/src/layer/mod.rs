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

// Core layer infrastructure
pub mod base;
pub mod handle;

// Layer implementations
pub mod container;
pub mod opacity;
pub mod transform;
pub mod clip;
pub mod picture;

// Re-export core types - Layer is now the main trait
pub use base::{Layer, AnyLayer, LayerState};
pub use handle::LayerHandle;

// Re-export layer implementations
pub use container::ContainerLayer;
pub use opacity::OpacityLayer;
pub use transform::{TransformLayer, Transform};

// Clip layers
pub use clip::{ClipRectLayer, ClipRRectLayer};
pub use picture::{PictureLayer, DrawCommand};

/// Type-erased layer (for dynamic dispatch)
///
/// Use `Box<dyn Layer>` when you need to store layers of different types together.
/// For better resource management, consider using `AnyLayer` or `LayerHandle<T>`.
pub type BoxedLayer = Box<dyn Layer>;

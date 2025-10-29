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
//! ## Layer Types (Compositor Primitives)
//!
//! ### Basic Composition
//! - **ContainerLayer**: Holds child layers
//! - **PictureLayer**: Leaf layer with drawing commands
//! - **TransformLayer**: Matrix transformations
//! - **OpacityLayer**: Alpha blending
//! - **OffsetLayer**: Translation/positioning
//!
//! ### Clipping
//! - **ClipRectLayer**: Rectangular clipping
//! - **ClipRRectLayer**: Rounded rectangle clipping
//! - **ClipPathLayer**: Arbitrary path clipping
//! - **ClipOvalLayer**: Oval/ellipse clipping
//!
//! ### Filters (Compositor Effects)
//! - **FilterLayer**: Color filters (matrix, blend modes)
//! - **BlurLayer**: Image filters (blur, dilate, erode)
//! - **BackdropFilterLayer**: Filters content behind layer
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

// ============================================================================
// Compositor Primitive Modules
// ============================================================================

// Core infrastructure
pub mod base;
pub mod handle;
pub mod pool;

// Basic composition layers
pub mod container;
pub mod picture;
pub mod transform;
pub mod opacity;
pub mod offset;

// Clipping layers
pub mod clip;

// Filter layers (compositor effects)
pub mod filter;
pub mod blur;
pub mod backdrop_filter;

// ============================================================================
// Public API Exports
// ============================================================================

// Core types
pub use base::{AnyLayer, Layer, LayerState};
pub use handle::LayerHandle;

// Basic composition layers
pub use container::ContainerLayer;
pub use picture::{DrawCommand, PictureLayer};
pub use transform::{Transform, TransformLayer};
pub use opacity::OpacityLayer;
pub use offset::OffsetLayer;

// Clipping layers
pub use clip::{ClipOvalLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer};

// Filter layers
pub use filter::FilterLayer;
pub use blur::BlurLayer;
pub use backdrop_filter::BackdropFilterLayer;

// Re-export filter types from flui_types
pub use flui_types::painting::effects::{BlurMode, BlurQuality, ColorFilter, ColorMatrix, ImageFilter};

/// Type-erased layer (for dynamic dispatch)
///
/// Use `Box<dyn Layer>` when you need to store layers of different types together.
/// For better resource management, consider using `AnyLayer` or `LayerHandle<T>`.
pub type BoxedLayer = Box<dyn Layer>;







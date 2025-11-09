//! FLUI Rendering Engine - wgpu-only GPU rendering
//!
//! This crate provides high-performance GPU-accelerated rendering for FLUI:
//!
//! - **Layer System**: Composable scene graph nodes for complex UIs
//! - **GpuPainter**: Modern wgpu-based rendering with Lyon tessellation
//! - **Event Router**: Pointer event handling and gesture recognition
//!
//! # Architecture
//!
//! ```text
//! RenderObject.paint() → Layer Tree → GpuPainter → wgpu → GPU
//!                           │
//!                           ├─ PictureLayer (drawing commands)
//!                           ├─ ClipLayer (clipping)
//!                           ├─ TransformLayer (transformations)
//!                           ├─ OpacityLayer (transparency)
//!                           └─ ContainerLayer (grouping)
//! ```
//!
//! # Layer System
//!
//! The layer system provides a retained-mode scene graph that RenderObjects
//! paint into. Layers are composable and handle common operations:
//!
//! ```rust,ignore
//! // RenderObject creates layers
//! fn paint(&self, offset: Offset) -> BoxedLayer {
//!     let mut picture = PictureLayer::new();
//!     picture.draw_rect(rect, &Paint::fill(Color::RED));
//!
//!     // Wrap in transform
//!     TransformLayer::translate(
//!         Box::new(picture),
//!         offset
//!     )
//! }
//! ```
//!
//! # GPU Painter
//!
//! For direct GPU rendering, use `GpuPainter`:
//!
//! ```rust,ignore
//! use flui_engine::painter::{GpuPainter, Paint};
//!
//! let mut painter = GpuPainter::new(&instance, surface, 800, 600).await?;
//! painter.begin_frame()?;
//! painter.rect(rect, &Paint::solid(Color::RED))?;
//! painter.end_frame()?;
//! ```

pub mod devtools;
pub mod event_router;
pub mod layer;
pub mod painter;
pub mod text;

// Re-export commonly used layer types
pub use layer::{
    // Core layer types
    BoxedLayer,
    Layer,

    // Basic composition layers
    ContainerLayer,
    OffsetLayer,
    OpacityLayer,
    TransformLayer,
    Transform,

    // Drawing layer
    PictureLayer,
    DrawCommand,

    // Clipping layers
    ClipRectLayer,
    ClipRRectLayer,
    ClipOvalLayer,
    ClipPathLayer,

    // Effect layers
    FilterLayer,
    BlurLayer,
    BackdropFilterLayer,
    ColorFilter,
    ColorMatrix,
    ImageFilter,

    // Interaction
    PointerListenerLayer,

    // Pooled variants (for performance)
    PooledContainerLayer,
    PooledClipRectLayer,
    PooledPictureLayer,

    // Re-exports from flui_types
    BlurMode,
    BlurQuality,
};

// Re-export painter types
// Note: Two painter systems coexist:
// - compat::Painter trait (used by layer system)
// - GpuPainter struct (new direct GPU rendering)
pub use painter::{Paint, Painter, Stroke};

// Re-export event router
pub use event_router::EventRouter;

// Re-export devtools integration (when feature enabled)
#[cfg(feature = "devtools")]
pub use devtools::{
    DevToolsLayout, FramePhase, FrameStats, FrameTimelineGraph, OverlayCorner, PerformanceOverlay,
    ProfiledCompositor, UnifiedDevToolsOverlay,
};

#[cfg(all(feature = "devtools", feature = "memory-profiler"))]
pub use devtools::MemoryGraph;








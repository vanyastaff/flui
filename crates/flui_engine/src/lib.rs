//! FLUI Rendering Engine
//!
//! Backend-agnostic rendering infrastructure for FLUI. This crate provides:
//!
//! - **Layer System**: Composable scene graph nodes (Container, Opacity, Transform, Clip, Picture)
//! - **Painter Abstraction**: Backend-agnostic drawing API
//! - **Scene & Compositor**: Build and render scene from layers
//!
//! # Architecture
//!
//! ```text
//! RenderObject.paint() -> Layer
//!                          │
//!                          ▼
//!                    Scene Builder
//!                          │
//!                          ▼
//!                     Layer Tree
//!                          │
//!                          ▼
//!                     Compositor
//!                          │
//!                          ▼
//!                   Painter (backend)
//!                     │         │
//!                     ▼         ▼
//!                  egui     wgpu/skia
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! // Create a picture layer with drawing commands
//! let mut picture = PictureLayer::new();
//! picture.draw_rect(
//!     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
//!     Paint {
//!         color: [1.0, 0.0, 0.0, 1.0],  // Red
//!         ..Default::default()
//!     }
//! );
//!
//! // Wrap in opacity layer
//! let opacity = OpacityLayer::new(Box::new(picture), 0.5);
//!
//! // Wrap in transform layer
//! let transform = TransformLayer::translate(
//!     Box::new(opacity),
//!     Offset::new(10.0, 20.0)
//! );
//!
//! // Paint to backend
//! transform.paint(&mut egui_painter);
//! ```
//!
//! # Feature Flags
//!
//! - `egui` (default): Enable egui backend
//! - `wgpu`: Enable wgpu backend (future)
//! - `skia`: Enable skia backend (future)

pub mod app;
pub mod backend;
pub mod backends;
pub mod compositor;
pub mod devtools;
pub mod event_router;
pub mod layer;
pub mod paint_context;
pub mod painter;
pub mod scene;
pub mod scene_builder;
pub mod surface;










// Re-export commonly used types
pub use layer::{
    Layer, BoxedLayer,
    ContainerLayer, OpacityLayer, TransformLayer, Transform, ClipRectLayer, ClipRRectLayer, PictureLayer, DrawCommand,
};
pub use painter::{Painter, Paint, RRect};
pub use paint_context::PaintContext;
pub use scene::{Scene, SceneMetadata};
pub use scene_builder::SceneBuilder;
pub use compositor::{Compositor, CompositorOptions, CompositionStats};
pub use surface::{Surface, Frame};
pub use backend::{RenderBackend, BackendCapabilities, BackendInfo};
pub use event_router::EventRouter;

// Re-export unified app API
pub use app::{App, AppConfig, AppLogic, Backend, WindowConfig};

// Re-export devtools integration (when feature enabled)
#[cfg(feature = "devtools")]
pub use devtools::{ProfiledCompositor, PerformanceOverlay, FrameTimelineGraph, DevToolsLayout, UnifiedDevToolsOverlay, OverlayCorner, FramePhase, FrameStats};

#[cfg(all(feature = "devtools", feature = "memory-profiler"))]
pub use devtools::MemoryGraph;

// Re-export backend implementations when features are enabled
#[cfg(feature = "egui")]
pub use backends::egui::EguiPainter;

#[cfg(feature = "wgpu")]
pub use backends::wgpu::{
    WgpuPainter, WgpuRenderer,
    TextRenderer, TextCommand, TextAlign, TextRenderError,
};










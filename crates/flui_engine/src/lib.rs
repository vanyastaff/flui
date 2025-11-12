//! FLUI Rendering Engine - Modern GPU rendering
//!
//! This crate provides the core rendering engine for FLUI:
//!
//! - **WgpuPainter**: GPU-accelerated 2D rendering (wgpu + lyon + glyphon)
//! - **CanvasLayer**: Modern compositor layer with CommandRenderer
//! - **CommandRenderer**: Clean architecture command execution (visitor pattern)
//!
//! # Architecture
//!
//! ```text
//! RenderObject.paint()
//!     ↓ generates Canvas (flui_painting)
//! CanvasLayer (stores DisplayList)
//!     ↓ render() → CommandRenderer (visitor pattern)
//! WgpuRenderer → WgpuPainter
//!     ↓ tessellates & renders
//! GPU (wgpu)
//! ```
//!
//! # Modern Rendering Path
//!
//! - **CanvasLayer**: Stores Canvas → DisplayList → DrawCommands
//! - **CommandRenderer**: Abstract interface for rendering backends
//! - **WgpuRenderer**: GPU-accelerated implementation
//!
//! All layer effects (Transform, Opacity, Clip, Filter) are implemented
//! as RenderObjects in `flui_rendering`, NOT here.
//!
//! # GPU Painter
//!
//! WgpuPainter provides hardware-accelerated 2D rendering:
//!
//! ```rust,ignore
//! use flui_engine::painter::WgpuPainter;
//!
//! let mut painter = WgpuPainter::new(device, queue, surface_format, size);
//! painter.rect(rect, &Paint::fill(Color::RED));
//! painter.render(&view, &mut encoder)?;
//! ```

// DevTools integration - commented out until Compositor is implemented
// TODO: Re-enable when crate::Compositor and crate::CompositorOptions are available
// #[cfg(feature = "devtools")]
// pub mod devtools;

pub mod gpu_renderer;
pub mod layer;
pub mod painter;
pub mod renderer;
pub mod scene;
pub mod text;

// Re-export GPU renderer (high-level abstraction)
pub use gpu_renderer::{GpuRenderer, RenderError};

// Re-export modern layer type
pub use layer::CanvasLayer;

// Re-export Scene (rendering snapshot)
pub use scene::Scene;

// Re-export painter types
// Note: Two painter systems coexist:
// - compat::Painter trait (used by layer system)
// - GpuPainter struct (new direct GPU rendering)
pub use painter::{Paint, Painter};

// Re-export renderer types (Clean Architecture command execution)
pub use renderer::{CommandRenderer, RenderBackend, WgpuRenderer};

#[cfg(debug_assertions)]
pub use renderer::DebugRenderer;

// Re-export devtools integration (when feature enabled)
#[cfg(feature = "devtools")]
pub use devtools::{
    DevToolsLayout, FramePhase, FrameStats, FrameTimelineGraph, OverlayCorner, PerformanceOverlay,
    ProfiledCompositor, UnifiedDevToolsOverlay,
};

// TODO: Re-enable memory-profiler feature
// #[cfg(all(feature = "devtools", feature = "memory-profiler"))]
// pub use devtools::MemoryGraph;


//! GPU-accelerated 2D painter for FLUI
//!
//! Unified painter architecture with clean separation of concerns:
//! - **WgpuPainter**: Main painter implementation (shapes + text + transforms)
//! - **Paint**: Unified styling (colors, gradients, fill/stroke)
//! - **Vertex**: GPU vertex data structures
//! - **Tessellator**: Lyon-based path tessellation
//! - **TextRenderer**: Glyphon-based text rendering
//!
//! # Architecture (SOLID + KISS)
//!
//! ```text
//! User Code (Layers, RenderObjects)
//!     ↓
//! WgpuPainter (implements Painter trait)
//!     ├── Tessellator (complex shapes)
//!     ├── TextRenderer (glyphon text)
//!     └── Transform Stack (glam Mat4)
//!         ↓
//! wgpu (Vulkan/Metal/DX12/WebGPU)
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_engine::painter::{WgpuPainter, Painter, Paint};
//! use flui_types::{Color, Point, Rect};
//!
//! let mut painter = WgpuPainter::new(device, queue, surface_format, (800, 600));
//!
//! // Draw shapes
//! painter.rect(
//!     Rect::from_ltrb(10.0, 10.0, 100.0, 100.0),
//!     &Paint::fill(Color::RED)
//! );
//!
//! // Draw text
//! painter.text("Hello", Point::new(10.0, 120.0), 16.0, &Paint::fill(Color::BLACK));
//!
//! // Render all batched geometry
//! painter.render(&view, &mut encoder)?;
//! ```
//!
//! # Features
//!
//! - ✅ GPU-accelerated rendering with wgpu
//! - ✅ Cross-platform (Windows, macOS, Linux, Web)
//! - ✅ Shape rendering (rect, circle, rounded rect, lines)
//! - ✅ Text rendering via glyphon
//! - ✅ Transform stack (translate, rotate, scale)
//! - ✅ Lyon tessellation for complex paths
//! - ✅ Gradient support
//! - ⚠️ Clipping (TODO: scissor/stencil)

// ===== Core modules =====

pub mod buffer_pool;
pub mod effects;
pub mod effects_pipeline;
pub mod instancing;
pub mod multi_draw;
pub mod pipeline;
pub mod tessellator;
mod text;
pub mod texture_cache;
mod vertex;
pub mod wgpu_painter;

// ===== Public API =====

// Main painter
pub use wgpu_painter::{Painter, WgpuPainter};

// Re-export Paint from flui_painting (Clean Architecture - no duplication)
pub use flui_painting::Paint;

// Vertex types
pub use vertex::Vertex;

// Tessellator (public for advanced use)
pub use tessellator::Tessellator;

// Re-export RRect from flui_types
pub use flui_types::geometry::RRect;

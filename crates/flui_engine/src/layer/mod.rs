//! Layer system - Modern CanvasLayer only
//!
//! This module provides CanvasLayer for the compositor.
//! All layer effects (Transform, Opacity, Clip, etc.) are implemented
//! as RenderObjects in flui_rendering, not here.
//!
//! ## Architecture
//!
//! ```text
//! RenderObject (flui_rendering)
//!     |
//!     | paint() generates Canvas
//!     v
//! CanvasLayer (flui_engine - this module)
//!     |
//!     | render() → CommandRenderer
//!     v
//! GPU Rendering (wgpu)
//! ```
//!
//! ## Modern Rendering Path
//!
//! - **CanvasLayer**: Contains Canvas → DisplayList → DrawCommands
//! - **CommandRenderer**: Visitor pattern for rendering commands
//! - **WgpuRenderer**: GPU-accelerated rendering implementation
//!
//! All layer composition is handled by the paint pipeline in flui_core.

pub mod picture;

pub use picture::CanvasLayer;

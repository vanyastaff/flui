//! # flui_painting
//!
//! Canvas-based painting abstraction for FLUI.
//!
//! This crate provides a high-level Canvas API for recording drawing commands
//! into a DisplayList. It's a backend-agnostic layer that decouples rendering
//! logic from GPU implementation details.
//!
//! ## Architecture
//!
//! ```text
//! Widget/RenderObject (flui_rendering)
//!     ↓
//! Canvas API (flui_painting - this crate)
//!     ↓
//! DisplayList (recorded draw commands)
//!     ↓
//! GPU Backend (flui_engine - executes commands)
//! ```
//!
//! ## Key Components
//!
//! - **Canvas**: High-level drawing API with state management (save/restore, transforms)
//! - **DisplayList**: Recorded sequence of drawing commands for GPU execution
//! - **DrawCommand**: Individual drawing operations (rect, path, text, image, etc.)
//! - **Paint**: Styling information (color, stroke, blend mode, shader)
//!
//! ## Design Principles
//!
//! 1. **Pure functions**: All painting methods are pure (no state)
//! 2. **Zero allocations**: Minimal intermediate buffers
//! 3. **Type safety**: Leverage Rust's type system for correctness
//! 4. **Separation of concerns**: Data (flui_types) vs Logic (flui_painting)

#![warn(missing_docs)]

// Core modules
pub mod canvas;
pub mod display_list;
pub mod error;

// Re-export Canvas API (primary interface)
pub use canvas::Canvas;
pub use display_list::{
    BlendMode, DisplayList, DrawCommand, Paint, PaintBuilder, PaintStyle, Shader, StrokeCap,
    StrokeJoin,
};

/// Prelude module for convenient imports
pub mod prelude {
    //! Common painting types and utilities for FLUI applications.

    pub use crate::canvas::Canvas;
    pub use crate::display_list::{
        BlendMode, DisplayList, DrawCommand, Paint, PaintStyle, Shader,
    };
}



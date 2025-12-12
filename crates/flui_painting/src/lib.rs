//! High-performance Canvas API for recording 2D drawing commands.
//!
//! `flui_painting` provides a backend-agnostic drawing abstraction layer that records
//! drawing commands into an immutable [`DisplayList`] for later GPU execution, leveraging
//! Rust's type system for safety and performance.
//!
//! # Architecture
//!
//! The crate implements the **Command Pattern** to separate recording from execution:
//!
//! ```text
//! RenderObject (flui_rendering)
//!     ↓ calls paint()
//! Canvas API (this crate)
//!     ↓ records commands
//! DisplayList (immutable)
//!     ↓ sent to GPU thread
//! WgpuPainter (flui_engine)
//!     ↓ executes on GPU
//! Framebuffer
//! ```
//!
//! # Core Types
//!
//! - [`Canvas`] - Main drawing interface with state management
//! - [`DisplayList`] - Immutable sequence of recorded commands
//! - [`DrawCommand`] - Individual drawing operations
//! - [`Paint`] - Styling information (color, stroke, shader, blend mode)
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use flui_painting::prelude::*;
//! use flui_types::{Rect, Color};
//!
//! // Create canvas
//! let mut canvas = Canvas::new();
//!
//! // Draw shapes
//! canvas.draw_rect(
//!     Rect::from_ltrb(10.0, 10.0, 100.0, 100.0),
//!     &Paint::fill(Color::RED)
//! );
//!
//! // Apply transforms
//! canvas.save();
//! canvas.translate(50.0, 50.0);
//! canvas.rotate(std::f32::consts::PI / 4.0);
//! canvas.draw_circle(Point::ZERO, 20.0, &Paint::fill(Color::BLUE));
//! canvas.restore();
//!
//! // Finish and get display list
//! let display_list = canvas.finish();
//!
//! // Analyze commands
//! println!("Recorded {} commands", display_list.len());
//! for cmd in &display_list {
//!     // Process each command
//! }
//! ```
//!
//! # Features
//!
//! ## Zero-Cost Abstractions
//!
//! - All operations compile to efficient machine code
//! - Transform API accepts both high-level `Transform` and low-level `Matrix4` types
//! - Extension traits provide convenience methods with no overhead
//!
//! ## Thread Safety
//!
//! - [`Canvas`] is `Send` - can be sent across threads
//! - [`DisplayList`] is `Send + Clone` - can be shared and cached
//! - Enables parallel painting in FLUI's rendering pipeline
//!
//! ## API Design
//!
//! - Intuitive method names and behavior
//! - Consistent with common 2D graphics APIs
//! - Easy to learn and use
//!
//! # Advanced Features
//!
//! ## Shader Effects
//!
//! ```rust,ignore
//! // Apply gradient fade
//! canvas.draw_shader_mask(bounds, shader, BlendMode::SrcOver, |child| {
//!     child.draw_image(image, offset, &paint);
//! });
//! ```
//!
//! ## Backdrop Filters
//!
//! ```rust,ignore
//! // Frosted glass effect
//! canvas.draw_backdrop_filter(
//!     bounds,
//!     ImageFilter::blur(10.0),
//!     BlendMode::SrcOver,
//!     Some(|child| {
//!         child.draw_rect(panel, &frosted_paint);
//!     })
//! );
//! ```
//!
//! ## Layer Composition
//!
//! ```rust,ignore
//! // Offscreen rendering with opacity
//! canvas.save_layer_opacity(bounds, 0.5);
//! // ... drawing operations ...
//! canvas.restore(); // Composite layer
//! ```
//!
//! # Performance Tips
//!
//! 1. **Reuse DisplayLists** - Cache for repeated content
//! 2. **Batch Similar Commands** - Group by paint/transform
//! 3. **Use Scoped Operations** - `with_save`, `with_translate` for auto cleanup
//! 4. **Culling** - Check `would_be_clipped()` before drawing
//!
//! # Extension Traits
//!
//! The crate uses the extension trait pattern for modularity:
//!
//! - [`DisplayListCore`] - Core API (sealed)
//! - [`DisplayListExt`] - Convenient filtering methods (auto-implemented)
//!
//! Users can add their own extension traits for domain-specific operations.
//!
//! # See Also
//!
//! - [`prelude`] - Convenient imports
//! - [`canvas`] - Canvas implementation
//! - [`display_list`] - DisplayList and DrawCommand types
//! - [`error`] - Error types

// ===== Quality Control: Compiler & Clippy Lints =====
//
// Note: Most lints are inherited from [workspace.lints] in root Cargo.toml.
// Only crate-specific lints and exceptions are defined here.

// Documentation (additional crate-specific lints)
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::private_intra_doc_links)]
// Safety (stricter than workspace default)
#![forbid(unsafe_code)]
// Production code quality (crate-specific)
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![warn(clippy::panic)]
// Crate-specific exceptions (beyond workspace allows)
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::similar_names)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::match_same_arms)]

// Core modules
pub mod canvas;
pub mod clip_context;
pub mod display_list;
pub mod error;

// ===== Facade Pattern: Public Re-exports =====
//
// Re-export all public types at the crate root for convenient access.
// This allows changing internal module structure without breaking user code.
// Users can write `use flui_painting::Canvas` instead of `use flui_painting::canvas::Canvas`.

// Primary API types
pub use canvas::Canvas;
pub use clip_context::ClipContext;
pub use display_list::{
    DisplayList, DisplayListCore, DisplayListExt, DisplayListStats, DrawCommand, HitRegion,
    HitRegionHandler,
};
pub use error::{PaintingError, Result};

// Flutter compatibility: Picture is our DisplayList
/// A Picture is an immutable recording of drawing commands.
///
/// In FLUI, `Picture` is a type alias for [`DisplayList`]. This provides
/// compatibility with Flutter's Picture API while maintaining our internal
/// naming convention.
///
/// # Flutter Equivalence
///
/// ```dart
/// // Flutter
/// final recorder = PictureRecorder();
/// final canvas = Canvas(recorder);
/// canvas.drawRect(rect, paint);
/// final picture = recorder.endRecording();
/// ```
///
/// ```rust
/// // FLUI
/// let mut canvas = Canvas::new();
/// canvas.draw_rect(rect, &paint);
/// let picture: Picture = canvas.finish();
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use flui_painting::{Canvas, Picture};
///
/// fn record_drawing() -> Picture {
///     let mut canvas = Canvas::new();
///     canvas.draw_circle(Point::ZERO, 50.0, &Paint::fill(Color::BLUE));
///     canvas.finish() // Returns Picture (DisplayList)
/// }
/// ```
pub type Picture = DisplayList;

// Re-export essential painting types from flui_types for user convenience
// This creates a cohesive API where users don't need to import from multiple crates
pub use flui_types::painting::{
    BlendMode, Paint, PaintBuilder, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
};

pub mod prelude {
    //! Convenient re-exports for common painting types.
    //!
    //! Import everything you need with one line:
    //!
    //! ```rust
    //! use flui_painting::prelude::*;
    //! ```
    //!
    //! # What's Included
    //!
    //! - **Main Types**: [`Canvas`], [`DisplayList`], [`DrawCommand`]
    //! - **Traits**: [`DisplayListCore`], [`DisplayListExt`]
    //! - **Styling**: [`Paint`], [`BlendMode`], [`Shader`]
    //! - **Paint Properties**: [`PaintStyle`], [`StrokeCap`], [`StrokeJoin`], [`PointMode`]
    //!
    //! # Examples
    //!
    //! ```rust,ignore
    //! use flui_painting::prelude::*;
    //! use flui_types::{Rect, Color};
    //!
    //! let mut canvas = Canvas::new();
    //! canvas.draw_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0), &Paint::fill(Color::RED));
    //! let display_list = canvas.finish();
    //!
    //! // Extension traits are in scope
    //! for cmd in display_list.draw_commands() {
    //!     // ...
    //! }
    //! ```

    pub use crate::canvas::Canvas;
    pub use crate::display_list::{DisplayList, DisplayListCore, DisplayListExt, DrawCommand};
    pub use crate::Picture; // Flutter compatibility
    pub use flui_types::painting::{
        BlendMode, Paint, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
    };
}

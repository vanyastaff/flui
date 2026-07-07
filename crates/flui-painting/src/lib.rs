//! High-performance Canvas API for recording 2D drawing commands.
//!
//! `flui_painting` provides a backend-agnostic drawing abstraction layer that
//! records drawing commands into an immutable [`DisplayList`] for later GPU
//! execution, leveraging Rust's type system for safety and performance.
//!
//! # Architecture
//!
//! The crate implements the **Command Pattern** to separate recording from
//! execution:
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
//! - Transform API accepts both high-level `Transform` and low-level `Matrix4`
//!   types
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
//! 3. **Use Scoped Operations** - `with_save`, `with_translate` for auto
//!    cleanup
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

// Ship bar (wave 2): every public item is documented; keep it that way.
#![deny(missing_docs)]
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
pub mod binding;
pub mod canvas;
pub mod clip_context;
pub mod decoration;
pub mod display_list;
pub mod error;

pub mod table_border;
pub mod text_layout;
pub mod text_painter;

// Painting test harness. Compiled only for this crate's own tests
// (`cfg(test)`) or when a consumer enables the `testing` feature. Provides a
// `record` builder for `DisplayList`s plus diagnostics helpers. See
// [`testing`] for the overview.
#[cfg(any(test, feature = "testing"))]
pub mod testing;

// ===== Facade Pattern: Public Re-exports =====
//
// Re-export all public types at the crate root for convenient access.
// This allows changing internal module structure without breaking user code.
// Users can write `use flui_painting::Canvas` instead of `use
// flui_painting::canvas::Canvas`.

// Binding
pub use binding::{CachedImage, ImageCache, ImageHandle, PaintingBinding, image_cache};
// Primary API types
pub use canvas::Canvas;
pub use clip_context::ClipContext;
pub use decoration::{box_decoration_hit_test, paint_box_decoration, resolve_gradient};
pub use display_list::{
    DisplayList, DisplayListCore, DisplayListExt, DisplayListStats, DrawCommand,
};
pub use error::{PaintingError, Result};
// Re-exported so consumers can name the font system type that appears in
// [`SharedFontSystem::with_mut`]'s callback without depending on cosmic-text
// directly. Deliberate boundary decision per ADR-0016 (pins this crate's
// semver to cosmic-text, intentionally).
pub use cosmic_text::FontSystem;
pub use table_border::paint_table_border;
pub use text_layout::{
    LineInfo, SharedFontSystem, TextLayout, TextLayoutResult, detect_text_direction,
    measure_inline_span, measure_text,
};
pub use text_painter::{DEFAULT_FONT_SIZE, Invalidation, TextBaseline, TextPainter};

// Re-export essential painting types from flui_types for user convenience.
// This creates a cohesive API where users don't need to import from multiple
// crates.
//
// REVIEW_BY: 2026-09-22 — audit P-12 cadence marker.
//
// **Canonical home: `flui_types::painting`.** The types below are
// *defined* in `flui-types` and *re-exported* here. `flui_painting::
// Paint` and `flui_types::painting::Paint` are the same type; the
// re-export is a convenience facade. Diagnostic messages (`error[E0308]
// mismatched types`) print the canonical `flui_types::painting::*`
// path, which can confuse downstream consumers who imported via this
// facade — the canonical-home note is the single-source clarification
// the audit recommended (over an explicit `as Paint` alias, which adds
// no information at the use site). Drop the marker or replace it with a
// CONTRIBUTING.md cross-reference once that doc lands.
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
    //! - **Paint Properties**: [`PaintStyle`], [`StrokeCap`], [`StrokeJoin`],
    //!   [`PointMode`]
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

    pub use flui_types::painting::{
        BlendMode, Paint, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
    };

    pub use crate::{
        canvas::Canvas,
        display_list::{DisplayList, DisplayListCore, DisplayListExt, DrawCommand},
        text_layout::{TextLayoutResult, detect_text_direction, measure_inline_span, measure_text},
        text_painter::{TextBaseline, TextPainter},
    };
}

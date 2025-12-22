//! Rich context implementations for layout, hit testing, and painting.
//!
//! This module provides high-level context types that wrap the capability traits
//! and provide ergonomic APIs for common operations.
//!
//! # Context Types
//!
//! - [`PaintContext`]: Rich painting API with scoped operations and chaining
//!
//! # Architecture
//!
//! Contexts wrap the underlying capability implementations and provide:
//! - **Scoped operations**: `with_save()`, `with_translate()`, `with_opacity()`
//! - **Chaining API**: Fluent builder pattern for sequential operations
//! - **Conditional drawing**: `when()`, `when_else()`, `draw_if()`
//! - **Child painting helpers**: `paint_child()`, `paint_children()`

mod paint;

pub use paint::PaintContext;

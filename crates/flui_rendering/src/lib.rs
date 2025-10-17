//! Rendering layer for Flui framework
//!
//! This crate provides the rendering infrastructure for layout and painting:
//! - RenderObject: Base trait for rendering
//! - RenderBox: Box layout protocol
//!
//! # Three-Tree Architecture
//!
//! This is the third tree in Flutter's architecture:
//!
//! ```text
//! Widget (immutable) → Element (mutable) → RenderObject (layout & paint)
//! ```
//!
//! # Layout Protocol
//!
//! 1. Parent sets constraints on child
//! 2. Child chooses size within constraints
//! 3. Parent positions child (sets offset)
//! 4. Parent returns its own size
//!
//! # Painting Protocol
//!
//! 1. Paint yourself
//! 2. Paint children in order
//! 3. Children are painted at their offsets

#![warn(missing_docs)]

pub mod egui_ext;
pub mod render_box;
pub mod render_object;

// Re-exports
pub use render_box::{RenderBox, RenderProxyBox};
pub use render_object::RenderObject;

// Re-export types from dependencies
pub use flui_core::BoxConstraints;
pub use flui_types::{Offset, Point, Rect, Size};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::render_box::{RenderBox, RenderProxyBox};
    pub use crate::render_object::RenderObject;
    pub use flui_core::BoxConstraints;
    pub use flui_types::{Offset, Point, Rect, Size};
}

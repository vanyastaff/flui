//! Core types for Flui framework
//!
//! This crate provides fundamental types used throughout Flui:
//! - **Geometry**: Point, Rect, Size, Offset
//! - **Layout**: Axis, EdgeInsets, Alignment, MainAxisAlignment, CrossAxisAlignment, MainAxisSize
//! - **Styling**: Color, Border, Shadow (coming soon)
//!
//! This is the base crate with NO dependencies on other flui crates.

#![warn(missing_docs)]

pub mod geometry;
pub mod layout;

// Re-exports for convenience
pub use geometry::{Offset, Point, Rect, Size};
pub use layout::{
    Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
    MainAxisSize, Orientation, VerticalDirection,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::geometry::{Offset, Point, Rect, Size};
    pub use crate::layout::{
        Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
        MainAxisSize, Orientation, VerticalDirection,
    };
}

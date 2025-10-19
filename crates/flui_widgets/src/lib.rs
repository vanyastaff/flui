//! Widget implementations for Flui framework
//!
//! This crate provides high-level widget implementations built on top of
//! the RenderObject layer (flui_rendering).
//!
//! # Architecture
//!
//! Widgets in Flui follow the three-tree pattern:
//!
//! ```text
//! Widget (immutable) → Element (mutable) → RenderObject (layout & paint)
//! ```
//!
//! # Widget Categories
//!
//! - **Layout widgets:** Container, Row, Column, Stack, etc.
//! - **Single-child layouts:** SizedBox, Padding, Center, Align
//! - **Multi-child layouts:** Row, Column, Stack, Wrap
//! - **Visual effects:** Opacity, Transform, ClipRRect, DecoratedBox
//! - **Flex children:** Expanded, Flexible, Spacer
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_widgets::*;
//!
//! // Create a centered container with padding
//! Container::new()
//!     .width(200.0)
//!     .height(100.0)
//!     .padding(EdgeInsets::all(16.0))
//!     .color(Color::rgb(0, 0, 255))
//!     .build()
//! ```

#![warn(missing_docs)]

pub mod container;

// Re-exports
pub use container::Container;

// Re-export commonly used types
pub use flui_core::{BoxConstraints, BuildContext, Widget};
pub use flui_rendering::{RenderBox, RenderObject};
pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
pub use flui_types::{Alignment, Color, EdgeInsets, Matrix4, Offset, Size};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::container::Container;
    pub use flui_core::{BoxConstraints, BuildContext, Widget};
    pub use flui_types::styling::{BorderRadius, BoxDecoration, Radius};
    pub use flui_types::{Alignment, Color, EdgeInsets, Matrix4, Offset, Size};
}

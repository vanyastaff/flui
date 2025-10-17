//! Core traits and types for Flui framework
//!
//! This crate provides the fundamental building blocks for the Flui widget system:
//! - Widget: Immutable configuration
//! - Element: Mutable state holder
//! - RenderObject: Layout and painting
//! - BuildContext: Access to element tree
//!
//! # Three-Tree Architecture
//!
//! Flui uses Flutter's three-tree architecture:
//!
//! 1. **Widget Tree** (immutable) - Describes WHAT to show
//! 2. **Element Tree** (mutable) - Manages lifecycle and state
//! 3. **Render Tree** (mutable) - Performs layout and painting
//!
//! ```text
//! Widget → Element → RenderObject
//! (new)     (reused)   (reused)
//! ```

#![warn(missing_docs)]

pub mod widget;
pub mod element;
pub mod build_context;
pub mod constraints;

// Re-export types from flui_types
pub use flui_types::{
    Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
    MainAxisSize, Offset, Orientation, Point, Rect, Size, VerticalDirection,
};

// Re-exports
pub use build_context::BuildContext;
pub use constraints::BoxConstraints;
pub use element::{ComponentElement, Element, ElementId, StatefulElement};
pub use widget::{IntoWidget, State, StatefulWidget, StatelessWidget, Widget};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::build_context::BuildContext;
    pub use crate::constraints::{BoxConstraints, Size};
    pub use crate::element::{Element, ElementId};
    pub use crate::widget::{IntoWidget, StatelessWidget, Widget};
}





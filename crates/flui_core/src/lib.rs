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

// New modular structure
pub mod foundation;
pub mod error;
pub mod widget;
pub mod element;
pub mod render;
pub mod context;
pub mod tree;

// Legacy modules (backward compatibility)
pub mod constraints;













// Re-export types from flui_types
pub use flui_types::{
    Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
    MainAxisSize, Offset, Orientation, Point, Rect, Size, VerticalDirection,
};

// Re-export foundation types
pub use foundation::{ElementId, Lifecycle, Slot};
pub use error::{CoreError, Result};

// Re-export from new modular structure
pub use context::BuildContext;
pub use constraints::BoxConstraints;
pub use element::{ComponentElement, Element, RenderObjectElement, StatefulElement};
pub use element::render::{
    LeafRenderObjectElement,
    MultiChildRenderObjectElement,
    SingleChildRenderObjectElement,
};
pub use tree::{ElementTree, PipelineOwner};
pub use widget::{InheritedElement, InheritedWidget, IntoWidget, State, StatefulWidget, StatelessWidget, Widget};
pub use render::{
    RenderObject,
    parent_data::{BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData},
};
pub use render::widget::{
    LeafRenderObjectWidget,
    MultiChildRenderObjectWidget,
    RenderObjectWidget,
    SingleChildRenderObjectWidget,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::context::BuildContext;
    pub use crate::constraints::BoxConstraints;
    pub use crate::element::Element;
    pub use crate::foundation::ElementId;
    pub use crate::tree::ElementTree;
    pub use crate::widget::{IntoWidget, StatelessWidget, Widget};
    pub use crate::Size;
}

































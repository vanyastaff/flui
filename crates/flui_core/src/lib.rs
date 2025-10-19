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

pub mod build_context;
pub mod constraints;
pub mod element;
pub mod element_tree;
pub mod inherited_widget;
pub mod leaf_render_object_element;
pub mod multi_child_render_object_element;
pub mod parent_data;
pub mod pipeline_owner;
pub mod render_object;
pub mod render_object_widget;
pub mod single_child_render_object_element;
pub mod widget;












// Re-export types from flui_types
pub use flui_types::{
    Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
    MainAxisSize, Offset, Orientation, Point, Rect, Size, VerticalDirection,
};

// Re-exports
pub use build_context::BuildContext;
pub use constraints::BoxConstraints;
pub use element::{ComponentElement, Element, ElementId, RenderObjectElement, StatefulElement};
pub use element_tree::ElementTree;
pub use inherited_widget::{InheritedElement, InheritedWidget};
pub use leaf_render_object_element::LeafRenderObjectElement;
pub use multi_child_render_object_element::MultiChildRenderObjectElement;
pub use pipeline_owner::PipelineOwner;
pub use parent_data::{BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData};
pub use render_object::RenderObject;
pub use render_object_widget::{
    LeafRenderObjectWidget, MultiChildRenderObjectWidget, RenderObjectWidget,
    SingleChildRenderObjectWidget,
};
pub use single_child_render_object_element::SingleChildRenderObjectElement;
pub use widget::{IntoWidget, State, StatefulWidget, StatelessWidget, Widget};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::build_context::BuildContext;
    pub use crate::constraints::{BoxConstraints, Size};
    pub use crate::element::{Element, ElementId};
    pub use crate::element_tree::ElementTree;
    pub use crate::widget::{IntoWidget, StatelessWidget, Widget};
}




















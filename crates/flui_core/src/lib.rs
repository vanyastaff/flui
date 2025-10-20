//! Core traits and types for the Flui framework
//!
//! flui_core provides the fundamental building blocks of the Flui widget system:
//! - Widget: Immutable configuration (what to build)
//! - Element: Mutable state holder (lifecycle, mounting, updates)
//! - RenderObject: Layout and painting primitives
//! - Context: Access to the element tree
//!
//! # Three-Tree Architecture
//!
//! Flui follows the proven three-tree architecture:
//!
//! 1. Widget Tree (immutable) — describes WHAT to show
//! 2. Element Tree (mutable) — manages lifecycle and state
//! 3. Render Tree (mutable) — performs layout and painting
//!
//! ```text
//! Widget → Element → RenderObject
//! (new)     (reused)   (reused)
//! ```
//!
//! # Quick start
//!
//! Most applications will depend on higher-level crates, but when working directly
//! with flui_core you can use the prelude for convenience:
//!
//! ```rust
//! use flui_core::prelude::*;
//!
//! // Build a minimal element tree with a dummy widget
//! struct Hello;
//!
//! impl Widget for Hello {}
//!
//! let mut tree = ElementTree::new();
//!
//! // Normally widgets are mounted through framework helpers; this is just a sketch
//! let _root_id = tree.mount(Hello.into_widget());
//!
//! // Iterate over children of the root element via a Context (pseudo-example)
//! let ctx = Context::empty();
//! for child in ctx.children() {
//!     // do something with child ElementId
//!     let _ = child;
//! }
//! ```
//!
//! See individual modules for details on widgets, elements, rendering and context utilities.

// New modular structure
pub mod cache;
pub mod constraints;
pub mod context;
pub mod element;
pub mod error;
pub mod foundation;
pub mod profiling;
pub mod render;
pub mod tree;
pub mod widget;









// Re-export types from flui_types
pub use flui_types::{
    Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
    MainAxisSize, Offset, Orientation, Point, Rect, Size, VerticalDirection,
};

// Re-export foundation types
pub use foundation::{ElementId, Lifecycle, Slot};
pub use error::{CoreError, Result};

// Re-export from modular structure
pub use context::Context;
pub use constraints::BoxConstraints;
pub use element::{AnyElement, ComponentElement, Element, ElementLifecycle, InactiveElements, RenderObjectElement, StatefulElement};
pub use element::render::{
    LeafRenderObjectElement,
    MultiChildRenderObjectElement,
    SingleChildRenderObjectElement,
};
pub use tree::{BuildOwner, ElementTree, GlobalKeyId, PipelineOwner};
pub use widget::{AnyWidget, InheritedElement, InheritedWidget, IntoWidget, State, StateLifecycle, StatefulWidget, StatelessWidget, Widget};
pub use render::{
    AnyRenderObject,
    RenderObject,
    parent_data::{BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData},
};
pub use render::widget::{
    LeafRenderObjectWidget,
    MultiChildRenderObjectWidget,
    RenderObjectWidget,
    SingleChildRenderObjectWidget,
};

// Re-export cache types
pub use cache::{
    LayoutCache, LayoutCacheKey, LayoutResult,
    get_layout_cache, invalidate_layout, clear_layout_cache,
};

// Re-export string cache
pub use foundation::string_cache::{InternedString, intern, resolve};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::context::Context;
    pub use crate::constraints::BoxConstraints;
    pub use crate::element::{AnyElement, Element};
    pub use crate::foundation::ElementId;
    pub use crate::tree::ElementTree;
    pub use crate::widget::{AnyWidget, IntoWidget, StatelessWidget, Widget};
    pub use crate::Size;
    pub use crate::cache::get_layout_cache;
    pub use crate::foundation::string_cache::intern;
}


































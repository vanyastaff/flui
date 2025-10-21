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
pub mod debug; // Phase 10: Debug infrastructure
pub mod element;
pub mod error;
pub mod foundation;
pub mod hot_reload; // Phase 14: Hot reload support
pub mod notification; // Phase 11: Notification system
pub mod profiling;
pub mod render;
pub mod testing; // Phase 15: Testing infrastructure
pub mod tree;
pub mod widget;










// Re-export types from flui_types
pub use flui_types::{
    Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
    MainAxisSize, Offset, Orientation, Point, Rect, Size, VerticalDirection,
};

// Re-export foundation types
pub use foundation::{ElementId, Slot};
pub use element::ElementLifecycle;
pub use error::{CoreError, Result, KeyError}; // Phase 10: Enhanced error types (uses ElementLifecycle)

// Re-export from modular structure
pub use context::Context;
pub use constraints::BoxConstraints;
pub use element::{DynElement, ComponentElement, Element, InactiveElements, RenderObjectElement, StatefulElement};
pub use element::render::{
    LeafRenderObjectElement,
    MultiChildRenderObjectElement,
    SingleChildRenderObjectElement,
};
pub use tree::{BuildOwner, ElementPool, ElementPoolStats, ElementTree, GlobalKeyId, PipelineOwner};
pub use widget::{DynWidget, InheritedElement, InheritedWidget, InheritedModel, IntoWidget, ParentDataElement, ParentDataWidget, ProxyElement, ProxyWidget, State, StateLifecycle, StatefulWidget, StatelessWidget, Widget, ErrorWidget, ErrorDetails, ErrorWidgetBuilder}; // Phase 3.3: ErrorWidget + builder, Phase 5.2: InheritedModel
pub use render::{
    DynRenderObject,
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
    layout_cache, invalidate_layout, clear_layout_cache,
};

// Re-export string cache
pub use foundation::string_cache::{capacity, get, intern, is_empty, len, resolve, InternedString};

// ========== Type Aliases for Common Patterns ==========

/// Boxed widget trait object
///
/// Commonly used for heterogeneous collections of widgets.
///
/// # Example
///
/// ```rust
/// use flui_core::BoxedWidget;
///
/// let widgets: Vec<BoxedWidget> = vec![
///     // Box::new(Text::new("Hello")),
///     // Box::new(Container::new()),
/// ];
/// ```
pub type BoxedWidget = Box<dyn DynWidget>;

/// Boxed element trait object
///
/// Commonly used for heterogeneous collections of elements.
pub type BoxedElement = Box<dyn DynElement>;

/// Boxed render object trait object
///
/// Commonly used for heterogeneous collections of render objects.
pub type BoxedRenderObject = Box<dyn DynRenderObject>;

/// Prelude module for convenient imports
///
/// This module re-exports the most commonly used types and traits for building UI.
/// Import everything with:
///
/// ```rust
/// use flui_core::prelude::*;
/// ```
pub mod prelude {
    // Core types
    pub use crate::{
        Context, BoxConstraints, Size, ElementId, ElementTree,
        DynWidget, DynElement, Widget, Element,
        StatelessWidget, StatefulWidget, State,
        IntoWidget,
    };

    // Keys (very commonly used)
    pub use crate::foundation::{
        Key, GlobalKey, ValueKey, UniqueKey, ObjectKey, WidgetKey,
        Slot,
    };

    // Lifecycle enums
    pub use crate::{ElementLifecycle, StateLifecycle};

    // Errors and Results
    pub use crate::{CoreError, Result, KeyError};

    // Common widget types
    pub use crate::{
        InheritedWidget, InheritedElement,
        ParentDataWidget, ParentDataElement,
        ProxyWidget, ProxyElement,
        ErrorWidget,
    };

    // Render types
    pub use crate::{
        DynRenderObject, RenderObject,
        LeafRenderObjectWidget, SingleChildRenderObjectWidget, MultiChildRenderObjectWidget,
    };

    // Geometry types from flui_types
    pub use crate::{
        Offset, Point, Rect, Alignment, EdgeInsets,
        Axis, AxisDirection, Orientation,
        MainAxisAlignment, CrossAxisAlignment, MainAxisSize,
        VerticalDirection,
    };

    // Utilities
    pub use crate::cache::layout_cache;
    pub use crate::foundation::string_cache::intern;

    // Type aliases
    pub use crate::{BoxedWidget, BoxedElement, BoxedRenderObject};
}



































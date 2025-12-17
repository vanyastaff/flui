//! FLUI Core - Reactive UI framework for Rust
//!
//! FLUI is a declarative UI framework inspired by Flutter, built for Rust.
//! It provides a powerful View system with efficient reactivity and
//! high-performance rendering.
//!
//! # Architecture
//!
//! FLUI uses a three-tree architecture:
//!
//! ```text
//! View Tree            Element Tree         Render Tree
//! (immutable)          (mutable state)      (layout/paint)
//!     ↓                      ↓                    ↓
//! Configuration  ←→  State Management  ←→  Visual Output
//! ```
//!
//! This crate contains the core implementation and re-exports types from
//! specialized crates for convenience.

#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
#![deny(unsafe_op_in_unsafe_fn)]

// =============================================================================
// Internal Modules
// =============================================================================

pub mod error_handling;
pub mod pipeline;
pub mod prelude;

// =============================================================================
// Re-exports from flui-types
// =============================================================================

pub use flui_types::{Offset, Size};

// =============================================================================
// Re-exports from flui-foundation crate
// =============================================================================

pub use flui_foundation::{ElementId, Key, Slot};

// =============================================================================
// Re-exports from flui-view crate (Flutter-like View/Element system)
// =============================================================================

// Core View traits
pub use flui_view::{
    BoxedView, InheritedView, IntoView, ParentDataView, ProxyView, RenderView, StatefulView,
    StatelessView, View, ViewExt, ViewState,
};

// Element types
pub use flui_view::{
    ElementBase, InheritedElement, Lifecycle, ParentDataElement, ProxyElement, RenderElement,
    StatefulElement, StatelessElement,
};

// BuildContext
pub use flui_view::{BuildContext, BuildContextExt};

// BuildOwner
pub use flui_view::BuildOwner;

// Tree types
pub use flui_view::{reconcile_children, ElementNode, ElementTree};

// Key types
pub use flui_view::{GlobalKey, GlobalKeyId, ObjectKey, ValueKey, ViewKey};

// Child helpers
pub use flui_view::{Child, Children};

// Notification system
pub use flui_view::{
    BoxedNotification, DragEndNotification, DragStartNotification, FocusNotification,
    KeepAliveNotification, LayoutChangedNotification, NotifiableElement, Notification,
    NotificationCallback, NotificationHandler, NotificationNode, ScrollNotification,
    SizeChangedNotification,
};

// Root element
pub use flui_view::{RootElement, RootElementImpl};

// Slot types
pub use flui_view::{ElementSlot, IndexedSlot, IndexedSlotBuilder};

/// View module for convenient imports.
pub mod view {
    pub use flui_view::{
        BoxedView, Child, Children, InheritedView, IntoView, ParentDataView, ProxyView, RenderView,
        StatefulView, StatelessView, View, ViewExt, ViewKey, ViewState,
    };
}

/// Element module for convenient imports.
pub mod element {
    pub use flui_foundation::ElementId;
    pub use flui_view::{
        ElementBase, ElementNode, ElementSlot, ElementTree, IndexedSlot, IndexedSlotBuilder,
        InheritedElement, Lifecycle, ParentDataElement, ProxyElement, RenderElement,
        StatefulElement, StatelessElement,
    };
}

/// Context module for convenient imports.
pub mod context {
    pub use flui_view::{BuildContext, BuildContextExt, BuildOwner};
}

// =============================================================================
// Re-exports from flui-rendering
// =============================================================================

pub use flui_rendering::{Arity, RenderBox, RuntimeArity};
// Re-export UpdateResult from flui-view
pub use flui_view::view::UpdateResult;

/// Render module for backward compatibility with widgets.
///
/// Re-exports rendering types from `flui_rendering` crate including:
/// - RenderBox trait and render objects
/// - Layout/paint/hit-test contexts
/// - RenderState types
pub mod render {
    pub use flui_rendering::prelude::*;
    // Additional re-exports for convenience
    pub use flui_rendering::{
        BoxHitTestContext, BoxLayoutContext, BoxPaintContext, HitTestContext, LayoutContext,
        PaintContext, SliverHitTestContext, SliverLayoutContext, SliverPaintContext,
    };
    // RenderState types
    pub use flui_rendering::{BoxRenderState, RenderState, SliverRenderState};
}

// =============================================================================
// Re-exports from pipeline module
// =============================================================================

pub use pipeline::PipelineBuildContext;

// =============================================================================
// Re-exports from this crate's pipeline module
// =============================================================================

pub use pipeline::{PipelineBuilder, PipelineOwner};

// =============================================================================
// Version info
// =============================================================================

/// FLUI version string
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// FLUI major version
pub const VERSION_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");

/// FLUI minor version
pub const VERSION_MINOR: &str = env!("CARGO_PKG_VERSION_MINOR");

/// FLUI patch version
pub const VERSION_PATCH: &str = env!("CARGO_PKG_VERSION_PATCH");

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert!(!VERSION.is_empty());
        assert!(!VERSION_MAJOR.is_empty());
        assert!(!VERSION_MINOR.is_empty());
        assert!(!VERSION_PATCH.is_empty());
    }

    #[test]
    fn test_reexports_available() {
        // Test that key types are available
        let _key: Option<Key> = None;
        let _id: Option<ElementId> = None;
    }
}

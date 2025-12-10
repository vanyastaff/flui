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
pub use flui_view::ViewMode;

// =============================================================================
// Re-exports from flui-element crate
// =============================================================================

pub use flui_element::{Element, ElementLifecycle, ElementTree};

/// Element module for backward compatibility with widgets.
///
/// Re-exports core element types from `flui-element` crate.
pub mod element {
    pub use flui_element::{Element, ElementLifecycle, ElementTree};
    pub use flui_foundation::ElementId;
}

// =============================================================================
// Re-exports from flui-view crate
// =============================================================================

pub use flui_element::IntoElement;
pub use flui_view::{BuildContext, ViewObject};

/// View traits module for backward compatibility with widgets.
///
/// Re-exports view traits and types from `flui-view` crate.
pub mod view {
    pub use flui_element::IntoElement;
    pub use flui_view::{
        children, AnimatedView, BuildContext, Child, Children, Listenable, ProviderView, ProxyView,
        StatefulView, StatelessView, ViewMode, ViewState,
    };
}

// =============================================================================
// Re-exports from flui-rendering
// =============================================================================

pub use flui_rendering::{Arity, RenderBox, RuntimeArity};
// Re-export RenderView trait and UpdateResult from flui-view
pub use flui_view::{RenderView, UpdateResult};

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

// NOTE: Cannot implement IntoElement → IntoView bridge due to orphan rule
// (cannot impl foreign trait for type parameter without local type).
//
// Instead, all widgets should implement IntoElement directly (like Text does).
// Composite widgets can call .into_element() on child widgets.

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
        let _mode = ViewMode::Stateless;
    }
}

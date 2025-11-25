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
// Internal Modules (not yet migrated to separate crates)
// =============================================================================

pub mod pipeline;
pub mod prelude;
pub mod view;

// =============================================================================
// Re-exports from flui-types
// =============================================================================

pub use flui_types::{Offset, Size};

// =============================================================================
// Re-exports from flui-foundation crate
// =============================================================================

pub use flui_foundation::{ElementId, Key, Slot, ViewMode};

// =============================================================================
// Re-exports from flui-element crate
// =============================================================================

pub use flui_element::{Element, ElementLifecycle, ElementTree};

// =============================================================================
// Re-exports from flui-view crate
// =============================================================================

pub use flui_view::{BuildContext, IntoElement, ViewObject};

// =============================================================================
// Re-exports from flui-rendering
// =============================================================================

pub use flui_rendering::{
    core::{Arity, RenderBox, RenderState, RuntimeArity},
    view::{RenderObjectWrapper, RenderView, RenderViewWrapper, UpdateResult},
};

// =============================================================================
// Re-exports from flui-pipeline
// =============================================================================

pub use flui_pipeline::context::PipelineBuildContext;

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
        let _mode = ViewMode::Stateless;
    }
}

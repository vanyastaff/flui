//! # FLUI Tree
//!
//! Tree abstraction traits for the FLUI UI framework. This crate provides
//! trait definitions that enable clean separation of concerns between
//! element management (`flui-element`) and rendering (`flui-rendering`).
//!
//! ## Problem Solved
//!
//! In UI frameworks, element trees and render operations are tightly coupled.
//! This creates circular dependencies:
//!
//! ```text
//! ❌ Before: element → render → pipeline → element (CIRCULAR!)
//! ```
//!
//! `flui-tree` breaks this cycle by defining abstract interfaces:
//!
//! ```text
//! ✅ After:
//!                     flui-foundation
//!                           │
//!            ┌──────────────┼──────────────┐
//!            │              │              │
//!            ▼              ▼              ▼
//!       flui-tree     flui-element   flui-rendering
//!            │              │              │
//!            └──────────────┴──────────────┘
//!                           │
//!                           ▼
//!                    flui-pipeline
//!                  (implements traits)
//! ```
//!
//! ## Core Traits
//!
//! ### Tree Access (Read/Write/Navigate)
//!
//! - [`TreeRead`] - Immutable access to tree nodes
//! - [`TreeWrite`] - Mutable operations (insert, remove)
//! - [`TreeNav`] - Navigation (parent, children, ancestors)
//!
//! ### Render Operations
//!
//! - [`RenderTreeAccess`] - Access to `RenderObject` and `RenderState`
//! - [`DirtyTracking`] - Layout/paint dirty flag management
//!
//! ## Iterators
//!
//! The crate provides zero-allocation iterators for tree traversal:
//!
//! - [`Ancestors`] - Iterate from node to root
//! - [`Descendants`] - Pre-order depth-first traversal
//! - [`DepthFirstIter`] - Configurable DFS with pre/post order
//! - [`BreadthFirstIter`] - Level-order traversal
//! - [`RenderAncestors`] - Skip non-render nodes
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeRead, TreeNav, TreeWrite};
//! use flui_foundation::ElementId;
//!
//! // Implement traits for your tree type
//! impl TreeRead for MyTree {
//!     type Node = MyNode;
//!
//!     fn get(&self, id: ElementId) -> Option<&Self::Node> {
//!         self.nodes.get(id.index())
//!     }
//!
//!     fn contains(&self, id: ElementId) -> bool {
//!         self.nodes.contains(id.index())
//!     }
//!
//!     fn len(&self) -> usize {
//!         self.nodes.len()
//!     }
//! }
//!
//! // Use generic functions that work with any tree
//! fn find_root<T: TreeNav>(tree: &T, start: ElementId) -> ElementId {
//!     tree.ancestors(start).last().unwrap_or(start)
//! }
//! ```
//!
//! ## Design Principles
//!
//! 1. **Minimal Dependencies** - Only `flui-foundation` required
//! 2. **Zero-Cost Abstractions** - Iterators optimized for no allocation
//! 3. **Trait Composition** - Small, focused traits that compose well
//! 4. **Thread Safety** - All traits require `Send + Sync`
//! 5. **Flutter-Inspired** - Architecture mirrors Flutter's design
//!
//! ## Feature Flags
//!
//! - `serde` - Enable serialization for tree-related types
//! - `full` - Enable all optional features

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::all,
    clippy::pedantic
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// ============================================================================
// MODULES
// ============================================================================

pub mod error;
pub mod iter;
pub mod traits;
pub mod visitor;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Core traits
pub use traits::{
    // Pipeline traits
    hit_test_with_callback,
    layout_with_callback,
    paint_with_callback,
    AtomicDirtyFlags,

    DirtyTracking,
    DirtyTrackingExt,
    // Combined traits
    FullTreeAccess,

    HitTestVisitable,
    HitTestVisitableExt,
    LayoutVisitable,
    LayoutVisitableExt,
    PaintVisitable,
    PaintVisitableExt,
    PipelinePhaseCoordinator,
    // Render access
    RenderTreeAccess,
    RenderTreeAccessExt,
    RenderTreeExt,
    SimpleTreeVisitor as PipelineSimpleVisitor,
    TreeMut,

    TreeNav,
    TreeNavDyn,
    TreeOperation,
    // Tree access
    TreeRead,
    // Object-safe variants
    TreeReadDyn,
    TreeVisitor as PipelineTreeVisitor,

    TreeWrite,
    TreeWriteNav,
};

// Iterators
pub use iter::{
    // Utility functions
    collect_render_children,
    count_render_children,
    count_render_elements,
    find_render_ancestor,
    find_render_root,
    first_render_child,
    has_render_children,
    is_render_descendant,
    is_render_leaf,
    last_render_child,
    lowest_common_render_ancestor,
    nth_render_child,
    render_depth,
    render_parent,
    Ancestors,
    AncestorsWithDepth,
    BreadthFirstIter,
    DepthFirstIter,
    DepthFirstOrder,
    Descendants,
    DescendantsWithDepth,
    RenderAncestors,
    RenderChildren,
    RenderChildrenWithIndex,
    RenderDescendants,
    RenderLeaves,
    RenderPath,
    RenderSiblings,
    RenderSubtree,
    RenderSubtreeItem,
    SiblingDirection,
    Siblings,
    SiblingsDirection,
};

// Visitor pattern
pub use visitor::{
    visit_breadth_first, visit_depth_first, TreeVisitor, TreeVisitorMut, VisitorResult,
};

// Errors
pub use error::{TreeError, TreeResult};

// Re-export ElementId for convenience
pub use flui_foundation::ElementId;

// ============================================================================
// PRELUDE
// ============================================================================

/// The tree prelude - commonly used types and traits.
///
/// ```rust
/// use flui_tree::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        // Iterators
        Ancestors,
        AtomicDirtyFlags,

        Descendants,

        DirtyTracking,
        DirtyTrackingExt,
        FullTreeAccess,
        RenderTreeAccess,
        RenderTreeAccessExt,
        // Types
        TreeError,
        TreeMut,
        TreeNav,
        // Core traits
        TreeRead,
        TreeResult,
        // Visitor
        TreeVisitor,
        TreeVisitorMut,
        TreeWrite,
        TreeWriteNav,
        VisitorResult,
    };

    pub use flui_foundation::ElementId;
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-tree crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns a summary of enabled features.
pub fn feature_summary() -> &'static str {
    #[cfg(feature = "serde")]
    {
        "serde"
    }

    #[cfg(not(feature = "serde"))]
    {
        "minimal"
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(VERSION, "0.1.0");
    }

    #[test]
    fn test_feature_summary() {
        let summary = feature_summary();
        assert!(!summary.is_empty());
    }
}

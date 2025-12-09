//! # FLUI Tree - Pure Tree Abstractions
//!
//! Generic tree abstraction traits for the FLUI UI framework.
//! This crate provides ONLY pure tree abstractions - domain-specific
//! implementations live in their respective crates.
//!
//! ## Design Philosophy
//!
//! flui-tree defines abstract interfaces that break circular dependencies:
//!
//! ```text
//!                     flui-foundation
//!                           │
//!            ┌──────────────┼──────────────┐
//!            │              │              │
//!            ▼              ▼              ▼
//!       flui-tree     flui-element   flui_rendering
//!     (abstractions)       │              │
//!            │              │              │
//!            └──────────────┴──────────────┘
//!                           │
//!                           ▼
//!                      flui_core
//! ```
//!
//! ## What's in flui-tree
//!
//! - **Core Traits**: `TreeRead`, `TreeNav`, `TreeWrite`
//! - **Generic Iterators**: Ancestors, Descendants, Siblings, DFS, BFS
//! - **Arity System**: Compile-time child count validation
//! - **Visitor Pattern**: Generic tree traversal
//!
//! ## What's NOT in flui-tree
//!
//! Domain-specific code lives in its own crate:
//!
//! - **flui_rendering**: RenderTree, DirtyTracking, render iterators
//! - **flui-element**: ElementTree, lifecycle, reconciliation
//! - **flui-view**: ViewTree, snapshots
//!
//! ## Core Traits
//!
//! ```rust,ignore
//! use flui_tree::{TreeRead, TreeNav, TreeWrite};
//!
//! // Read-only access
//! fn count_nodes<T: TreeRead>(tree: &T) -> usize {
//!     tree.len()
//! }
//!
//! // Navigation
//! fn find_root<T: TreeNav>(tree: &T, id: ElementId) -> ElementId {
//!     tree.ancestors(id).last().unwrap_or(id)
//! }
//! ```
//!
//! ## Iterators
//!
//! ```rust,ignore
//! use flui_tree::{Ancestors, Descendants, DepthFirstIter};
//!
//! // Find all ancestors
//! let path: Vec<_> = tree.ancestors(node).collect();
//!
//! // DFS traversal
//! for id in tree.descendants(root) {
//!     process(id);
//! }
//! ```
//!
//! ## Arity System
//!
//! ```rust,ignore
//! use flui_tree::arity::{Leaf, Single, Optional, Variable};
//!
//! // Compile-time child count validation
//! struct PaddingBox;  // Single child
//! struct Container;   // Variable children
//! struct Text;        // Leaf (no children)
//! ```

#![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]
#![allow(
    dead_code,
    unused_variables,
    missing_docs,
    missing_debug_implementations,
    unreachable_pub,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines,
    clippy::if_not_else,
    clippy::match_same_arms
)]

// ============================================================================
// MODULES
// ============================================================================

pub mod arity;
pub mod error;
pub mod iter;
pub mod state;
pub mod traits;
pub mod visitor;

// ============================================================================
// RE-EXPORTS - Core Traits
// ============================================================================

pub use traits::{TreeNav, TreeRead, TreeWrite, TreeWriteNav};

// ============================================================================
// RE-EXPORTS - Arity System
// ============================================================================

pub use arity::{
    Arity, ArityError, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, Range, RuntimeArity, Single, SliceChildren, Variable,
};

// ============================================================================
// RE-EXPORTS - State System
// ============================================================================

pub use state::{
    Dirty, Mountable, Mounted, NodeState, Reassembling, StateMarker, TreeInfo, Unmountable,
    Unmounted,
};

// ============================================================================
// RE-EXPORTS - Iterators
// ============================================================================

pub use iter::{
    AllSiblings, Ancestors, AncestorsWithDepth, BreadthFirstIter, DepthFirstIter, DepthFirstOrder,
    Descendants, DescendantsWithDepth, Siblings, SiblingsDirection,
};

// ============================================================================
// RE-EXPORTS - Visitor Pattern
// ============================================================================

pub use visitor::{
    collect_all, count_all, find_first, for_each, max_depth, visit_breadth_first,
    visit_depth_first, CollectVisitor, CountVisitor, FindVisitor, ForEachVisitor, MaxDepthVisitor,
    TreeVisitor, TreeVisitorMut, VisitorResult,
};

// ============================================================================
// RE-EXPORTS - Errors
// ============================================================================

pub use error::{TreeError, TreeResult};

// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================

pub use flui_foundation::{ElementId, Identifier};

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
        // Convenience functions
        collect_all,
        count_all,
        find_first,
        for_each,
        max_depth,
        // Iterators
        Ancestors,
        // Arity
        Arity,
        ChildrenAccess,
        Descendants,
        // Core traits
        Identifier,
        Leaf,
        // State types
        Dirty,
        Mountable,
        Mounted,
        NodeState,
        Optional,
        Reassembling,
        Single,
        StateMarker,
        TreeError,
        TreeInfo,
        // Types
        TreeNav,
        TreeRead,
        TreeResult,
        TreeVisitor,
        TreeVisitorMut,
        TreeWrite,
        TreeWriteNav,
        Unmountable,
        Unmounted,
        Variable,
        VisitorResult,
    };

    pub use flui_foundation::ElementId;
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-tree crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns a summary of what this crate provides.
pub fn crate_summary() -> &'static str {
    "Pure tree abstractions: TreeRead, TreeNav, TreeWrite, iterators, arity system, typestate markers"
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
    fn test_summary() {
        let summary = crate_summary();
        assert!(summary.contains("tree abstractions"));
    }
}

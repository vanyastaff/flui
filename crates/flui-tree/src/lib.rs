//! # FLUI Tree - Pure Tree Abstractions
//!
//! Generic tree abstraction traits for the FLUI UI framework.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Typestate Pattern                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  NodeState trait with two implementations:                  │
//! │  • Unmounted - Node not in tree, can be mounted             │
//! │  • Mounted   - Node in tree, has position info              │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Each tree type (Element, RenderObject) implements          │
//! │  depth, parent, owner fields directly - no shared base.     │
//! └─────────────────────────────────────────────────────────────┘
//!
//! Typestate transitions:
//!   Unmounted ──mount()──► Mounted ──unmount()──► Unmounted
//! ```
//!
//! ## Core Traits
//!
//! - `TreeRead<I>` - Read-only access to nodes
//! - `TreeNav<I>` - Navigation (parent, children, ancestors)
//! - `TreeWrite<I>` - Mutations (insert, remove)
//! - `NodeState` - Typestate marker trait (Mounted/Unmounted)
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_tree::{Mounted, Unmounted, NodeState, Depth};
//! use std::marker::PhantomData;
//!
//! struct MyNode<S: NodeState> {
//!     depth: Depth,
//!     parent: Option<NodeId>,
//!     _state: PhantomData<S>,
//! }
//!
//! impl MyNode<Unmounted> {
//!     fn new() -> Self { /* ... */ }
//!     fn mount(self, parent: Option<NodeId>, depth: Depth) -> MyNode<Mounted> { /* ... */ }
//! }
//!
//! impl MyNode<Mounted> {
//!     fn parent(&self) -> Option<NodeId> { self.parent }
//!     fn depth(&self) -> Depth { self.depth }
//!     fn unmount(self) -> MyNode<Unmounted> { /* ... */ }
//! }
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
pub mod depth;
pub mod diff;
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
// RE-EXPORTS - Node State & Lifecycle (Typestate)
// ============================================================================

pub use state::{Mountable, MountableExt, Mounted, NodeState, Unmountable, Unmounted};

// ============================================================================
// RE-EXPORTS - Arity System
// ============================================================================

pub use arity::{
    Arity, ArityError, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, Range, RuntimeArity, Single, SliceChildren, Variable,
};

// ============================================================================
// RE-EXPORTS - Node System
// ============================================================================

pub use traits::{Node, NodeExt, NodeTypeInfo};

// ============================================================================
// RE-EXPORTS - Depth System
// ============================================================================

pub use depth::{AtomicDepth, Depth, DepthAware, DepthError, MAX_TREE_DEPTH, ROOT_DEPTH};

// ============================================================================
// RE-EXPORTS - Slot System
// ============================================================================

pub use iter::{IndexedSlot, Slot, SlotBuilder, SlotIter};

// ============================================================================
// RE-EXPORTS - Path System
// ============================================================================

pub use iter::{IndexPath, TreeNavPathExt, TreePath};

// ============================================================================
// RE-EXPORTS - Cursor System
// ============================================================================

pub use iter::TreeCursor;

// ============================================================================
// RE-EXPORTS - Diff System
// ============================================================================

pub use diff::{ChildDiff, ChildOp, DiffOp, DiffStats, TreeDiff};

// ============================================================================
// RE-EXPORTS - Children System
// ============================================================================

pub use arity::{
    ArityStorage,
    ArityStorageView,
    ChildrenStorage,
    ChildrenStorageExt,
    // Type aliases for common cases
    LeafStorage,
    OptionalChildStorage,
    SingleChildStorage,
    VariableChildrenStorage,
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
        // Arity types
        Arity,
        ArityStorage,
        ArityStorageView,
        // Depth system
        AtomicDepth,
        // Diff system
        ChildDiff,
        ChildOp,
        // Children storage
        ChildrenAccess,
        ChildrenStorage,
        ChildrenStorageExt,
        Depth,
        DepthAware,
        Descendants,
        DiffOp,
        DiffStats,
        // Core traits
        Identifier,
        // Path system
        IndexPath,
        // Slot system
        IndexedSlot,
        Leaf,
        LeafStorage,
        // State types & lifecycle (typestate)
        Mountable,
        MountableExt,
        Mounted,
        // Node system
        Node,
        NodeExt,
        NodeState,
        NodeTypeInfo,
        Optional,
        OptionalChildStorage,
        Single,
        SingleChildStorage,
        Slot,
        SlotBuilder,
        // Tree traits
        TreeCursor,
        TreeDiff,
        TreeError,
        TreeNav,
        TreeNavPathExt,
        TreePath,
        TreeRead,
        TreeResult,
        TreeVisitor,
        TreeVisitorMut,
        TreeWrite,
        TreeWriteNav,
        Unmountable,
        Unmounted,
        Variable,
        VariableChildrenStorage,
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
    "Tree abstractions with typestate: NodeState (Mounted/Unmounted), TreeRead, TreeNav, TreeWrite"
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
        assert!(summary.to_lowercase().contains("tree"));
    }
}

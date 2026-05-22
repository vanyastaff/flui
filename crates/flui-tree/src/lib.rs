//! # FLUI Tree - Pure Tree Abstractions
//!
//! Generic tree abstraction traits for the FLUI UI framework.
//!
//! ## Core Traits
//!
//! - `TreeRead<I>` - Read-only access to nodes (`get`, `contains`, `len`)
//! - `TreeNav<I>` - Navigation (parent, children, ancestors, descendants)
//! - `TreeWrite<I>` - Mutations (`insert`, `remove` cascade-by-default,
//!   `remove_shallow` opt-out, `add_child` / `remove_child`)
//!
//! Each concrete tree type (`LayerTree`, `SemanticsTree`, `RenderTree`,
//! `ElementTree`, `ViewTree`) implements the trio. Per memory
//! `flui-tree-unified-interface-intent`, this trio is the canonical
//! API for cross-tree algorithms.
//!
//! ## Example
//!
//! See the concrete tree implementations (`flui-layer::LayerTree`,
//! `flui-semantics::SemanticsTree`, etc.) for `TreeRead` + `TreeNav` +
//! `TreeWrite` adopters with end-to-end test coverage. The audit cycle 3
//! removed the standalone `Mountable` / `Unmountable` typestate
//! machinery (`state.rs`, 616 LOC) — zero in-workspace consumers, and
//! the cycle 2 PR #100 work on `LayerNode::disposed: AtomicBool` +
//! `Drop` proved the lifecycle contract belongs on the concrete node
//! type, not behind a generic typestate.

#![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions, clippy::too_many_lines)]

// ============================================================================
// MODULES
// ============================================================================

pub mod arity;
pub mod depth;
pub mod diff;
pub mod error;
pub mod iter;
pub mod traits;
pub mod visitor;

// ============================================================================
// RE-EXPORTS - Core Traits
// ============================================================================

// ============================================================================
// RE-EXPORTS - Arity System
// ============================================================================
pub use arity::{
    Arity, ArityError, AtLeast, ChildrenAccess, Exact, FixedChildren, Leaf, NoChildren, Optional,
    OptionalChild, Range, RuntimeArity, Single, SliceChildren, Variable,
};
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
// RE-EXPORTS - Depth System
// ============================================================================
pub use depth::{AtomicDepth, Depth, DepthAware, DepthError, MAX_TREE_DEPTH, ROOT_DEPTH};
// ============================================================================
// RE-EXPORTS - Diff System
// ============================================================================
pub use diff::{ChildDiff, ChildOp, DiffOp, DiffStats, TreeDiff};
// ============================================================================
// RE-EXPORTS - Errors
// ============================================================================
pub use error::{TreeError, TreeResult};
// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================
pub use flui_foundation::{ElementId, Identifier};
// ============================================================================
// RE-EXPORTS - Cursor System
// ============================================================================
pub use iter::TreeCursor;
// ============================================================================
// RE-EXPORTS - Iterators
// ============================================================================
pub use iter::{
    AllSiblings, Ancestors, AncestorsWithDepth, BreadthFirstIter, DepthFirstIter, DepthFirstOrder,
    Descendants, DescendantsWithDepth, Siblings, SiblingsDirection,
};
// ============================================================================
// RE-EXPORTS - Path System
// ============================================================================
pub use iter::{IndexPath, TreeNavPathExt, TreePath};
// ============================================================================
// RE-EXPORTS - Slot System
// ============================================================================
pub use iter::{IndexedSlot, Slot, SlotBuilder, SlotIter};
// ============================================================================
// RE-EXPORTS - Node System
// ============================================================================
pub use traits::{
    Node, NodeExt, NodePredicate, NodeTypeInfo, NodeVisitor, collect_matching_nodes,
    count_matching_nodes,
};
pub use traits::{TreeNav, TreeNavExt, TreeRead, TreeReadExt, TreeWrite, TreeWriteNav};
// ============================================================================
// RE-EXPORTS - Visitor Pattern
// ============================================================================
pub use visitor::{
    CollectVisitor, CountVisitor, FindVisitor, ForEachVisitor, MaxDepthVisitor, TreeVisitor,
    TreeVisitorMut, VisitorResult, collect_all, count_all, find_first, for_each, max_depth,
    visit_breadth_first, visit_depth_first,
};

// ============================================================================
// PRELUDE
// ============================================================================

/// The tree prelude - commonly used types and traits.
///
/// ```rust
/// use flui_tree::prelude::*;
/// ```
pub mod prelude {
    pub use flui_foundation::ElementId;

    pub use crate::{
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
        // Node system
        Node,
        NodeExt,
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
        // Extension traits
        TreeNavExt,
        TreeNavPathExt,
        TreePath,
        TreeRead,
        TreeReadExt,
        TreeResult,
        TreeVisitor,
        TreeVisitorMut,
        TreeWrite,
        TreeWriteNav,
        Variable,
        VariableChildrenStorage,
        VisitorResult,
        // Convenience functions
        collect_all,
        count_all,
        find_first,
        for_each,
        max_depth,
    };
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-tree crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns a summary of what this crate provides.
#[must_use]
pub fn crate_summary() -> &'static str {
    "Tree abstractions: TreeRead, TreeNav, TreeWrite (cascade-by-default)"
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

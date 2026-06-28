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
#![allow(clippy::module_name_repetitions)]

// ============================================================================
// MODULES
// ============================================================================

pub mod arity;
pub mod depth;
pub mod error;
pub mod iter;
pub mod traits;

// Cycle 3 T-4..T-8: `visitor` + `diff` modules deleted (10k LOC
// zombie surface, zero in-workspace consumers per audit
// Appendix A.2). Same disposition for `iter::cursor`, `iter::path`,
// `iter::breadth_first`, `iter::depth_first`, `traits::node`,
// `arity::accessors`, `arity::arity_storage`, `arity::storage`,
// `arity::runtime`, `arity::aliases`. Future devtools / advanced
// visitor needs port from git history rather than carry maintenance
// burden of speculative scaffolding.

// ============================================================================
// RE-EXPORTS - Arity System (markers only — storage machinery deleted)
// ============================================================================
pub use arity::{
    Arity, ArityError, AtLeast, Exact, Leaf, Never, Optional, Range, Single, Variable,
};
// ============================================================================
// RE-EXPORTS - Depth System
// ============================================================================
pub use depth::{
    AtomicDepth, Depth, DepthAware, DepthError, INLINE_TREE_DEPTH, MAX_TREE_DEPTH, ROOT_DEPTH,
};
// ============================================================================
// RE-EXPORTS - Errors
// ============================================================================
pub use error::{TreeError, TreeResult};
// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================
pub use flui_foundation::{ElementId, Identifier, TreeId};
// ============================================================================
// RE-EXPORTS - Iterators (ancestor / descendant / sibling only)
// ============================================================================
pub use iter::{
    AllSiblings, Ancestors, AncestorsWithDepth, Descendants, DescendantsWithDepth, Siblings,
    SiblingsDirection,
};
// ============================================================================
// RE-EXPORTS - Slot System
// ============================================================================
pub use iter::{IndexedSlot, Slot, SlotBuilder, SlotIter};
// ============================================================================
// RE-EXPORTS - Tree Traits
// ============================================================================
pub use traits::{
    NodePredicate, NodeVisitor, TreeNav, TreeNavExt, TreeRead, TreeReadExt, TreeWrite,
    TreeWriteNav, collect_matching_nodes, count_matching_nodes,
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
        // Arity markers
        Arity,
        // Depth system
        AtomicDepth,
        Depth,
        DepthAware,
        Descendants,
        // Core traits
        Identifier,
        // Slot system
        IndexedSlot,
        Leaf,
        Optional,
        Single,
        Slot,
        SlotBuilder,
        // Tree traits
        TreeError,
        TreeId,
        TreeNav,
        // Extension traits
        TreeNavExt,
        TreeRead,
        TreeReadExt,
        TreeResult,
        TreeWrite,
        TreeWriteNav,
        Variable,
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
        // `VERSION` is wired from the package version (`env!("CARGO_PKG_VERSION")`);
        // assert its shape, not a pinned literal — a hardcoded value breaks on
        // every workspace version bump (it broke at the 0.1.0 -> 0.2.0 bump).
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "VERSION should be semver `major.minor.patch`, got {VERSION:?}",
        );
        assert!(
            parts.iter().all(|part| part.parse::<u64>().is_ok()),
            "VERSION components should be numeric, got {VERSION:?}",
        );
    }

    #[test]
    fn test_summary() {
        let summary = crate_summary();
        assert!(summary.to_lowercase().contains("tree"));
    }
}

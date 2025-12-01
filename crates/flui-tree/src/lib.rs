//! # FLUI Tree - Advanced Type System Edition
//!
//! Tree abstraction traits for the FLUI UI framework using cutting-edge Rust
//! type system features. This crate provides trait definitions that enable
//! clean separation of concerns with advanced compile-time guarantees.
//!
//! ## Advanced Type System Features
//!
//! This crate leverages the most advanced Rust type system capabilities:
//!
//! - **GAT (Generic Associated Types)** - Flexible iterators and accessors
//! - **HRTB (Higher-Rank Trait Bounds)** - Universal predicates and visitors
//! - **Associated Constants** - Performance tuning and optimization hints
//! - **Const Generics** - Compile-time size optimization
//! - **Sealed Traits** - Safe abstraction boundaries
//! - **Typestate Pattern** - Compile-time state verification
//! - **Never Type (`!`)** - Impossible operation safety
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
//! `flui-tree` breaks this cycle by defining abstract interfaces with
//! advanced type safety:
//!
//! ```text
//! ✅ After (with advanced types):
//!                     flui-foundation
//!                           │
//!            ┌──────────────┼──────────────┐
//!            │              │              │
//!            ▼              ▼              ▼
//!       flui-tree     flui-element   flui-rendering
//!    (GAT + HRTB)         │              │
//!            │              │              │
//!            └──────────────┴──────────────┘
//!                           │
//!                           ▼
//!                    flui-pipeline
//!              (implements with type safety)
//! ```
//!
//! ## Core Traits with Advanced Features
//!
//! ### Tree Access (Enhanced with GAT)
//!
//! - [`TreeRead`] - Immutable access with GAT iterators and HRTB predicates
//! - [`TreeWrite`] - Mutable operations with const generic optimization
//! - [`TreeNav`] - Navigation with flexible iterator types via GAT
//! - [`TreeReadExt`] - Extension trait with HRTB-based operations
//! - [`TreeNavExt`] - Extension trait with advanced traversal methods
//!
//! ### Render Operations (Type-Safe)
//!
//! - [`RenderTreeAccess`] - Access with compile-time guarantees
//! - [`DirtyTracking`] - Atomic flag management with const optimization
//! - [`RenderTreeExt`] - HRTB-compatible render operations
//!
//! ### Visitor Pattern (HRTB + GAT)
//!
//! - [`TreeVisitor`] - Basic visitor with HRTB support
//! - [`TreeVisitorMut`] - Mutable visitor with GAT return types
//! - [`TypedVisitor`] - Flexible result collection using GAT
//! - [`StatefulVisitor`] - Typestate pattern for compile-time safety
//!
//! ## Advanced Iterators
//!
//! The crate provides GAT-based iterators with const generic optimization:
//!
//! - [`Ancestors`] - GAT-based ancestor iteration with stack allocation
//! - [`Descendants`] - Pre-order DFS with configurable buffering
//! - [`DepthFirstIter`] - Const generic stack optimization
//! - [`BreadthFirstIter`] - Configurable queue size via const generics
//! - [`RenderAncestors`] - HRTB-compatible render-only traversal
//!
//! ## Enhanced Arity System
//!
//! Advanced compile-time arity validation with const generics:
//!
//! - [`Leaf`] - 0 children with never type for impossible operations
//! - [`Optional`] - 0-1 children with Option-like API
//! - [`Exact<N>`] - Exactly N children with const generic validation
//! - [`AtLeast<N>`] - N+ children with HRTB predicate support
//! - [`Variable`] - Any number with performance hints
//! - [`Range<MIN, MAX>`] - Bounded ranges with compile-time limits
//!
//! ## HRTB Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeNav, TreeReadExt, find_first};
//! use flui_foundation::ElementId;
//!
//! // HRTB predicate that works with any lifetime
//! fn find_matching_node<T: TreeNav + TreeReadExt>(
//!     tree: &T,
//!     root: ElementId
//! ) -> Option<ElementId>
//! where
//!     T::Node: HasName, // Hypothetical trait
//! {
//!     // This predicate works with any lifetime thanks to HRTB
//!     tree.find_node_where(|node| node.name().contains("button"))
//! }
//!
//! // GAT-based flexible iteration
//! impl TreeNav for MyTree {
//!     type ChildrenIter<'a> = impl Iterator<Item = ElementId> + 'a where Self: 'a;
//!
//!     fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
//!         // Return optimized iterator type based on internal storage
//!         self.get_children(id).iter().copied()
//!     }
//! }
//! ```
//!
//! ## Const Generic Example
//!
//! ```rust,ignore
//! use flui_tree::{visit_depth_first, CollectVisitor};
//!
//! // Const generic optimization for typical tree depths
//! fn traverse_optimized<T: TreeNav, const STACK_SIZE: usize = 64>(
//!     tree: &T,
//!     root: ElementId
//! ) -> Vec<ElementId> {
//!     let mut visitor = CollectVisitor::<32>::new(); // 32 inline elements
//!     visit_depth_first::<T, _, STACK_SIZE>(tree, root, &mut visitor);
//!     visitor.into_inner().into_vec()
//! }
//! ```
//!
//! ## Design Principles Enhanced
//!
//! 1. **Advanced Type Safety** - GAT, HRTB, sealed traits for correctness
//! 2. **Zero-Cost Abstractions** - Const generics and associated constants
//! 3. **Flexible Composition** - HRTB-compatible traits that compose well
//! 4. **Thread Safety** - All traits require `Send + Sync` with atomic operations
//! 5. **Compile-Time Optimization** - Const generics and typestate patterns
//! 6. **Performance Tuning** - Associated constants for implementation hints
//!
//! ## Feature Flags
//!
//! - `serde` - Enable serialization with GAT support
//! - `full` - Enable all optional advanced features

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

pub mod arity;
pub mod error;
pub mod iter;
pub mod traits;
pub mod visitor;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Core traits with advanced type features
pub use traits::{
    // Tree diffing
    find_common_subtrees,
    // Tree validation
    find_orphans,
    has_cycles,
    // Pipeline traits with HRTB support
    hit_test_with_callback,
    layout_with_callback,
    paint_with_callback,
    tree_edit_distance,
    validate_tree,
    // TreeContext traits
    AncestorLookup,
    // Tree views
    AncestorView,
    AtomicDirtyFlags,
    // Reconciliation traits
    CanUpdate,
    ChangeTracker,
    // InheritedElement support
    Dependencies,
    DependencyTracker,
    DepthLimitedView,
    // Element lifecycle traits
    DepthTracking,
    DiffOptions,
    DiffSummary,
    DirtyTracking,
    DirtyTrackingExt,
    ElementTreeOps,
    FilteredView,
    // Combined traits with GAT
    FullTreeAccess,
    GlobalKeyRegistry,
    HitTestVisitable,
    HitTestVisitableExt,
    IdMatcher,
    InheritedData,
    InheritedElement,
    InheritedLookup,
    InheritedRegistry,
    InheritedScope,
    InheritedState,
    InsertAction,
    LayoutVisitable,
    LayoutVisitableExt,
    Lifecycle,
    LinearReconciler,
    MoveAction,
    // Multi alias for Variable (backwards compatibility)
    Multi,
    NodeMatcher,
    NotificationPolicy,
    OwnerTracking,
    PaintVisitable,
    PaintVisitableExt,
    PipelinePhaseCoordinator,
    PredicateMatcher,
    RebuildPriority,
    RebuildScheduler,
    Reconciler,
    ReconciliationResult,
    // Render child accessor (Type State pattern using unified Arity)
    RenderChildAccessor,
    // Render access with GAT
    RenderTreeAccess,
    RenderTreeAccessExt,
    RenderTreeExt,
    SiblingView,
    SimpleTreeVisitor as PipelineSimpleVisitor,
    SnapshotDiff,
    SubtreeView,
    TreeDiff,
    TreeDiffResult,
    TreeMut,
    TreeNav,
    TreeNavDyn,
    TreeOperation,
    // Tree access with GAT and HRTB
    TreeRead,
    // Object-safe variants
    TreeReadDyn,
    TreeSnapshot,
    TreeValidator,
    TreeViewExt,
    TreeVisitor as PipelineTreeVisitor,
    TreeWrite,
    TreeWriteNav,
    UpdateAction,
    ValidationIssue,
    ValidationIssues,
    ValidationOptions,
    ValidationReport,
};

// Sealed trait markers for external implementations
pub use traits::sealed;

// Enhanced Arity system with advanced type features
pub use arity::{
    // Core trait with GAT and HRTB
    Arity,
    // Arity markers with const generic support
    AtLeast,
    // Enhanced accessors with GAT
    BoundedChildren, // New: Bounded range accessor
    ChildrenAccess,
    Copied,
    Exact,
    FixedChildren,
    Leaf,
    Never, // Never type for impossible operations
    NoChildren,
    Optional,
    OptionalChild,
    PerformanceHint, // New: Performance optimization hints
    // New: Advanced arity types
    Range, // Bounded range with const generics
    RuntimeArity,
    Single,
    SliceChildren,
    SmartChildren, // New: Smart allocation strategy
    TypedChildren, // New: Type-aware accessor
    Variable,
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
    // Arity-aware collection
    RenderChildrenCollector,
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

// Advanced visitor pattern with HRTB and GAT
pub use visitor::{
    // Convenience functions with HRTB
    collect_all,
    // Statistics visitors
    collect_statistics,
    compare_statistics,
    count_all,
    count_with_limit,
    find_first, // Enhanced with HRTB
    for_each,   // Enhanced with HRTB
    max_depth,
    max_depth_with_threshold,
    // Visitor states for typestate pattern
    states,
    tree_summary,
    // Fallible visitors
    try_collect,
    try_for_each,
    validate_depth,
    // Enhanced traversal functions with const generics
    visit_breadth_first,
    visit_depth_first,
    visit_depth_first_typed, // New: GAT-based typed visitor
    visit_fallible,
    visit_fallible_breadth_first,
    visit_fallible_with_path,
    visit_stateful, // New: Typestate pattern visitor
    // Enhanced built-in visitors
    CollectVisitor, // Now with const generics
    // Visitor composition
    ComposedVisitor,
    ConditionalVisitor,
    CountVisitor, // Enhanced with limits
    DepthLimitExceeded,
    DepthLimitVisitor,
    DynVisitor,
    FallibleVisitor,
    FallibleVisitorMut,
    FindVisitor,    // Now with HRTB support
    ForEachVisitor, // Enhanced with HRTB
    IterationHint,  // New: Performance optimization hints
    MappedVisitor,
    MaxDepthVisitor, // Enhanced with early termination
    StatefulVisitor, // New: Typestate pattern
    StatisticsComparison,
    StatisticsVisitor,
    StatisticsVisitorMut,
    TreeStatistics,
    // Visitor traits with advanced features
    TreeVisitor,
    TreeVisitorMut,
    TripleComposedVisitor,
    TryCollectVisitor,
    TryForEachVisitor,
    TypedVisitor, // New: GAT-based visitor
    VisitorError,
    VisitorExt,
    VisitorResult,
    VisitorVec,
};

// Errors
pub use error::{TreeError, TreeResult};

// Re-export ElementId for convenience
pub use flui_foundation::ElementId;

// Re-export geometry types used in traits
pub use flui_types::{Offset, Size};

// ============================================================================
// PRELUDE
// ============================================================================

/// The tree prelude - commonly used types and traits with advanced features.
///
/// ```rust
/// use flui_tree::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        // Convenience functions with HRTB
        collect_all,
        count_all,
        find_first, // Enhanced with HRTB
        for_each,   // Enhanced with HRTB
        max_depth,
        // Advanced visitor functions
        visit_depth_first_typed, // New: GAT-based traversal
        visit_stateful,          // New: Typestate pattern
        // Enhanced iterators with GAT
        Ancestors,
        // Enhanced Arity system with const generics
        Arity,
        AtLeast,
        AtomicDirtyFlags,
        ChildrenAccess,
        Descendants,
        DirtyTracking,
        DirtyTrackingExt,
        Exact,
        FullTreeAccess,
        IterationHint, // New: Performance hints
        Leaf,
        Never, // New: Never type
        Optional,
        PerformanceHint, // New: Performance optimization
        Range,           // New: Bounded range
        RenderTreeAccess,
        RenderTreeAccessExt,
        RuntimeArity,
        Single,
        // Types with advanced features
        TreeError,
        TreeMut,
        TreeNav,
        // Core traits with GAT and HRTB
        TreeRead,
        TreeResult,
        // Enhanced visitor pattern
        TreeVisitor,
        TreeVisitorMut,
        TreeWrite,
        TreeWriteNav,
        TypedVisitor, // New: GAT-based visitor
        Variable,
        VisitorResult,
    };

    pub use flui_foundation::ElementId;
    pub use flui_types::{Offset, Size};
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-tree crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns a summary of enabled features with advanced type information.
pub fn feature_summary() -> &'static str {
    #[cfg(feature = "serde")]
    {
        "serde + GAT + HRTB + const_generics + sealed_traits"
    }

    #[cfg(not(feature = "serde"))]
    {
        "GAT + HRTB + const_generics + sealed_traits"
    }
}

/// Returns information about advanced type system features used.
pub fn type_system_features() -> &'static str {
    concat!(
        "GAT (Generic Associated Types), ",
        "HRTB (Higher-Rank Trait Bounds), ",
        "Const Generics, ",
        "Associated Constants, ",
        "Sealed Traits, ",
        "Typestate Pattern, ",
        "Never Type (!)"
    )
}

/// Performance characteristics of this implementation.
pub fn performance_info() -> &'static str {
    "Zero-cost abstractions with compile-time optimization via const generics and GAT"
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
        assert!(summary.contains("GAT"));
        assert!(summary.contains("HRTB"));
    }

    #[test]
    fn test_advanced_features() {
        let type_features = type_system_features();
        assert!(type_features.contains("GAT"));
        assert!(type_features.contains("HRTB"));
        assert!(type_features.contains("Const Generics"));
        assert!(type_features.contains("Never Type"));

        let perf_info = performance_info();
        assert!(perf_info.contains("Zero-cost"));
        assert!(perf_info.contains("compile-time"));
    }
}

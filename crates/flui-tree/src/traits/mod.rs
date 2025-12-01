//! Core traits for tree operations.
//!
//! This module defines the fundamental traits that enable abstraction
//! over tree implementations. These traits are designed to be:
//!
//! - **Minimal**: Each trait has a single responsibility
//! - **Composable**: Traits can be combined for richer functionality
//! - **Thread-Safe**: All traits require `Send + Sync`
//!
//! # Trait Hierarchy
//!
//! ```text
//! TreeRead (immutable access)
//!     │
//!     ├── TreeNav (navigation) ─────────┐
//!     │                                 │
//!     └── TreeWrite (mutations) ────────┤
//!                                       │
//!                                       ▼
//!                                   TreeMut
//!                               (full access)
//!                                       │
//!                                       ▼
//!                              FullTreeAccess
//!                          (+ render operations)
//! ```
//!
//! # Render-Specific Traits
//!
//! For render operations, additional traits are provided:
//!
//! - [`RenderTreeAccess`] - Access `RenderObject` and `RenderState`
//! - [`RenderTreeExt`] - Extended render tree operations with iterators
//! - [`DirtyTracking`] - Manage layout/paint dirty flags
//!
//! # Pipeline Traits
//!
//! Abstract patterns for layout, paint, and hit-test operations:
//!
//! - [`LayoutVisitable`] / [`LayoutVisitableExt`] - Layout operations
//! - [`PaintVisitable`] / [`PaintVisitableExt`] - Paint operations
//! - [`HitTestVisitable`] / [`HitTestVisitableExt`] - Hit test operations
//! - [`PipelinePhaseCoordinator`] - Phase coordination
//!
//! These traits are designed to be implemented by `ElementTree` in
//! `flui-pipeline`, enabling `flui-rendering` to depend only on
//! abstract interfaces.

mod combined;
mod context;
mod diff;
mod dirty;
mod inherited;
mod lifecycle;
mod nav;
mod pipeline;
mod read;
mod reconciliation;
mod render;
mod validation;
mod view;
mod write;

/// Sealed trait markers for implementing core tree traits.
///
/// This module exports the internal sealed traits that are required
/// to implement `TreeRead`, `TreeNav`, etc.
///
/// # Usage
///
/// To implement `TreeRead` for your type:
///
/// ```rust,ignore
/// use flui_tree::sealed;
///
/// impl sealed::TreeReadSealed for MyTree {}
/// impl sealed::TreeNavSealed for MyTree {}
/// ```
pub mod sealed {
    pub use super::nav::sealed::Sealed as TreeNavSealed;
    pub use super::read::sealed::Sealed as TreeReadSealed;
}

pub use combined::{FullTreeAccess, TreeMut, TreeNavDyn, TreeReadDyn};
pub use context::{
    AncestorLookup, DescendantLookup, FullTreeContext, NavigationContext, OwnerContext,
    ReadOnlyContext, RenderContext, TreeContext,
};
pub use diff::{
    find_common_subtrees, tree_edit_distance, ChangeTracker, DiffOptions, DiffSummary, IdMatcher,
    NodeMatcher, PredicateMatcher, TreeDiff, TreeDiffResult,
};
pub use dirty::{AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt};
pub use inherited::{
    Dependencies, DependencyTracker, InheritedData, InheritedElement, InheritedLookup,
    InheritedRegistry, InheritedScope, InheritedState, NotificationPolicy,
};
pub use lifecycle::{
    DepthTracking, ElementTreeOps, Lifecycle, OwnerTracking, RebuildPriority, RebuildScheduler,
};
pub use nav::TreeNav;
pub use pipeline::{
    hit_test_with_callback, layout_with_callback, paint_with_callback, HitTestVisitable,
    HitTestVisitableExt, LayoutVisitable, LayoutVisitableExt, PaintVisitable, PaintVisitableExt,
    PipelinePhaseCoordinator, SimpleTreeVisitor, TreeOperation, TreeVisitor,
};
pub use read::TreeRead;
pub use reconciliation::{
    CanUpdate, GlobalKeyRegistry, InsertAction, LinearReconciler, MoveAction, Reconciler,
    ReconciliationResult, UpdateAction,
};
pub use render::{
    Multi, RenderChildAccessor, RenderTreeAccess, RenderTreeAccessExt, RenderTreeExt,
};
pub use validation::{
    find_orphans, has_cycles, validate_tree, TreeValidator, ValidationIssue, ValidationIssues,
    ValidationOptions, ValidationReport,
};
pub use view::{
    AncestorView, DepthLimitedView, FilteredView, SiblingView, SnapshotDiff, SubtreeView,
    TreeSnapshot, TreeViewExt,
};
pub use write::{TreeWrite, TreeWriteNav};

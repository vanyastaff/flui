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
mod dirty;
mod nav;
mod pipeline;
mod read;
mod render;
mod write;

pub use combined::{FullTreeAccess, TreeMut, TreeNavDyn, TreeReadDyn};
pub use dirty::{AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt};
pub use nav::TreeNav;
pub use pipeline::{
    hit_test_with_callback, layout_with_callback, paint_with_callback, HitTestVisitable,
    HitTestVisitableExt, LayoutVisitable, LayoutVisitableExt, PaintVisitable, PaintVisitableExt,
    PipelinePhaseCoordinator, SimpleTreeVisitor, TreeOperation, TreeVisitor,
};
pub use read::TreeRead;
pub use render::{RenderTreeAccess, RenderTreeAccessExt, RenderTreeExt};
pub use write::{TreeWrite, TreeWriteNav};

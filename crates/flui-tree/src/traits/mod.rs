//! Core traits for tree operations.
//!
//! This module defines the fundamental traits that enable abstraction
//! over tree implementations. These traits are designed to be:
//!
//! - **Minimal**: Each trait has a single responsibility
//! - **Composable**: Traits can be combined for richer functionality
//! - **Thread-Safe**: All traits require `Send + Sync`
//! - **Generic over ID**: All traits use `I: TreeId` generic parameter
//!
//! # Trait Hierarchy
//!
//! ```text
//! TreeRead<I> (immutable access)
//!     │
//!     ├── TreeNav<I> (navigation)
//!     │
//!     └── TreeWrite<I> (mutations)
//!             │
//!             └── TreeWriteNav<I> (combined)
//! ```
//!
//! # Design Philosophy
//!
//! flui-tree provides ONLY pure tree abstractions. Domain-specific
//! implementations live in their respective crates:
//!
//! - **`flui_rendering`**: `RenderTree`, `DirtyTracking`, render iterators
//! - **flui-element**: `ElementTree`, lifecycle, reconciliation
//! - **flui-view**: `ViewTree`, snapshots
//!
//! # Generic ID Parameter
//!
//! All traits use `I: TreeId` as a generic parameter, allowing:
//! - Clean trait bounds: `T: TreeNav<ElementId>`
//! - Composable traits: `trait DirtyTracking<I>: TreeNav<I>`
//! - Different ID types for different tree implementations

mod nav;
mod read;
mod write;

// Cycle 3 T-8: `pub mod node` deleted (305 LOC, zero external impls).
// The `Node`/`NodeExt`/`NodeTypeInfo` triad was a speculative
// abstraction without consumers.

pub use nav::{TreeNav, TreeNavExt};
pub use read::{
    NodePredicate, NodeVisitor, TreeRead, TreeReadExt, collect_matching_nodes, count_matching_nodes,
};
pub use write::{TreeWrite, TreeWriteNav};

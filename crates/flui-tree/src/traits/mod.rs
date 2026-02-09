//! Core traits for tree operations.
//!
//! This module defines the fundamental traits that enable abstraction
//! over tree implementations. These traits are designed to be:
//!
//! - **Minimal**: Each trait has a single responsibility
//! - **Composable**: Traits can be combined for richer functionality
//! - **Thread-Safe**: All traits require `Send + Sync`
//! - **Generic over ID**: All traits use `I: Identifier` generic parameter
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
//! All traits use `I: Identifier` as a generic parameter, allowing:
//! - Clean trait bounds: `T: TreeNav<ElementId>`
//! - Composable traits: `trait DirtyTracking<I>: TreeNav<I>`
//! - Different ID types for different tree implementations

mod nav;
pub mod node;
mod read;
mod write;

pub use nav::{TreeNav, TreeNavExt};
pub use node::{Node, NodeExt, NodeTypeInfo};
pub use read::{
    collect_matching_nodes, count_matching_nodes, NodePredicate, NodeVisitor, TreeRead, TreeReadExt,
};
pub use write::{TreeWrite, TreeWriteNav};

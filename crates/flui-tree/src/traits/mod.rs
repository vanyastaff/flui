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
//!     ├── TreeNav (navigation)
//!     │
//!     └── TreeWrite (mutations)
//!             │
//!             └── TreeWriteNav (combined)
//! ```
//!
//! # Design Philosophy
//!
//! flui-tree provides ONLY pure tree abstractions. Domain-specific
//! implementations live in their respective crates:
//!
//! - **flui_rendering**: RenderTree, DirtyTracking, render iterators
//! - **flui-element**: ElementTree, lifecycle, reconciliation
//! - **flui-view**: ViewTree, snapshots

mod nav;
mod read;
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

pub use nav::TreeNav;
pub use read::TreeRead;
pub use write::{TreeWrite, TreeWriteNav};

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
//! - [`RenderTreeAccess`] - Access RenderObject and RenderState
//! - [`DirtyTracking`] - Manage layout/paint dirty flags
//!
//! These traits are designed to be implemented by `ElementTree` in
//! `flui-pipeline`, enabling `flui-rendering` to depend only on
//! abstract interfaces.

mod combined;
mod dirty;
mod nav;
mod read;
mod render;
mod write;

pub use combined::{FullTreeAccess, TreeMut, TreeNavDyn, TreeReadDyn};
pub use dirty::{AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt};
pub use nav::TreeNav;
pub use read::TreeRead;
pub use render::{RenderTreeAccess, RenderTreeAccessExt};
pub use write::{TreeWrite, TreeWriteNav};

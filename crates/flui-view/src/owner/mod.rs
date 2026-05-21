//! Build phase management.
//!
//! This module provides:
//! - [`BuildOwner`] - Manages dirty elements and build scheduling
//! - [`ElementOwner`] - Split-borrow handle threaded through Element
//!   lifecycle (mount/unmount/update) so per-frame registries
//!   (GlobalKey, dirty heap, inactive queue) are updated without a
//!   blanket `&mut BuildOwner` borrow across the recursive traversal.

mod build_owner;
mod element_owner;

pub use build_owner::BuildOwner;
pub use element_owner::ElementOwner;

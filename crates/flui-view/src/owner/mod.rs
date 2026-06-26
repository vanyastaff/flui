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
// Internal scheduling handle — `pub(crate)`: captured by `ElementCore` at mount,
// no public consumer. See `ExternalBuildScheduler`.
pub(crate) use build_owner::ExternalBuildScheduler;
pub use element_owner::ElementOwner;
// Build-time live-tree handle carried on `ElementOwner` during a
// `build_scope` drain (PR-K). Crate-internal.
pub(crate) use element_owner::BuildHandle;

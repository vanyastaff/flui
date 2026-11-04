//! Type-safe identifiers for the scheduler subsystem.
//!
//! This module re-exports foundation ID types (`Id<T>`, markers) and the
//! [`AtomicIdMint`] they are issued from. The scheduler holds no counter of
//! its own: every id a scheduler mints is minted by the foundation `IdMint`
//! family, whose refusal rule is sticky — an id is never issued twice, and
//! an exhausted mint refuses, typed, forever.
//!
//! ## Foundation Unification
//!
//! The callback and task ID types (`TaskId`, `CallbackId`) are
//! aliases for `flui_foundation::Id<T>` with the corresponding marker from
//! `flui_foundation::markers`, and only a mint issues them.
//!
//! ## Example
//!
//! ```rust
//! use std::num::NonZeroU64;
//! use flui_foundation::{AtomicIdMint, TaskId};
//!
//! // Task identities are issued from any thread the scheduler shares
//! // the mint across; a mint that ran out refuses, typed, forever.
//! let mint = AtomicIdMint::<TaskId>::new();
//! let first = mint.next().expect("a fresh mint has ids to issue");
//! let second = mint.next().expect("a fresh mint has ids to issue");
//! assert!(first < second);
//! ```

// =============================================================================
// Re-exports from flui-foundation
// =============================================================================

pub use flui_foundation::{AtomicIdMint, FrameCallbackId, Id, Identifier, Marker, TaskId, markers};

/// UpdateScheduler callback ID - alias for `FrameCallbackId` from foundation.
///
/// Identifies callbacks (transient, persistent, post-frame) in the scheduler.
pub type CallbackId = FrameCallbackId;

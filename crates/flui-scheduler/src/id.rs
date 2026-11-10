//! Identifiers for the scheduler subsystem.
//!
//! The callback and task ids (`CallbackId`, `TaskId`) are opaque foundation
//! identities: a private `NonZeroU64`, issued only by the foundation `IdMint`
//! family and never built from a number. The scheduler holds no counter of
//! its own: a scheduler's callback ids come from its owner-local `IdMint`,
//! and task ids from the process's `AtomicIdMint`, because tasks are created
//! on whichever thread asks. Refusal is sticky: an id is never issued twice,
//! and an exhausted mint refuses, typed, forever.
//!
//! ## Example
//!
//! ```rust
//! use flui_scheduler::UpdateScheduler;
//!
//! // A callback id comes from the scheduler that accepted the callback.
//! let scheduler = UpdateScheduler::new();
//! let first = scheduler.schedule_frame_callback(Box::new(|_| {}));
//! let second = scheduler.schedule_frame_callback(Box::new(|_| {}));
//! assert!(first < second);
//! ```

pub use flui_foundation::{FrameCallbackId, TaskId};

/// UpdateScheduler callback ID - alias for `FrameCallbackId` from foundation.
///
/// Identifies callbacks (transient, persistent, post-frame) in the scheduler.
pub type CallbackId = FrameCallbackId;

//! Identifiers for the scheduler subsystem.
//!
//! The callback id (`CallbackId`) is an opaque foundation
//! identity: a private `NonZeroU64`, issued only by the foundation `IdMint`
//! family and never built from a number. The scheduler holds no counter of
//! its own: a scheduler's callback ids come from its owner-local `IdMint`.
//! Refusal is sticky: an id is never issued twice,
//! and an exhausted mint refuses, typed, forever.

pub use flui_foundation::FrameCallbackId;

/// UpdateScheduler callback ID - alias for `FrameCallbackId` from foundation.
///
/// Identifies post-frame callbacks and lifecycle listeners in the scheduler.
pub type CallbackId = FrameCallbackId;

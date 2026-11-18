//! # FLUI UpdateScheduler
//!
//! Owner-local frame execution, post-frame callbacks, lifecycle and async
//! tasks for FLUI.
//!
//! `UpdateScheduler` owns *logical* time only (the phase machine, frame
//! demand and the callbacks that observe a frame) and makes no refresh-rate,
//! display or surface assumption of its own; a caller supplies the frame's
//! vsync timestamp to the owner frame through the composition-root seam
//! [`__runtime::DriveFrame`] (physical pacing is a presentation-owned concern,
//! split out from this crate).
//!
//! ## Architecture
//!
//! ```text
//! Application
//!     ↓
//! OwnerFrame (executes complete frame and background turns)
//!     └─ UpdateScheduler (admits work and observes logical frame state)
//!
//! Frame Timeline:
//! Idle → Frame (async poll, pipeline) → PostFrame (post-frame callbacks) → Idle
//! ```
//!
//! ### Frame telemetry
//! What a frame cost and when it reached the display is measured in
//! [`std::time::Duration`] and recorded per frame as a
//! [`frame_telemetry::FrameSnapshot`]; [`frame_histogram`]
//! summarizes a run of them.
//!
//! ## Example Usage
//!
//! ```rust
//! use flui_scheduler::__runtime::DriveFrame as _;
//! use flui_scheduler::{OwnerFrame, UpdateScheduler};
//!
//! let scheduler = UpdateScheduler::new();
//! // The owner thread's frame state: owner-local post-frame callbacks and
//! // async tasks and complete execution. A UI runtime owns one.
//! let owner = OwnerFrame::try_new(&scheduler).expect("the scheduler has no live owner frame");
//!
//! // Execute frame (called by event loop)
//! let now = flui_scheduler::Instant::now();
//! owner
//!     .drive_frame(now, || {}, || {})
//!     .expect("the owner accepts the frame");
//! ```
//!
//! ## Feature Flags
//!
//! - **`serde`** - Enable serialization support for the frame and lifecycle
//!   enums. Adds `Serialize` and `Deserialize` derives to them.
//! - **`testing`** - Test-only lock-discipline probes (e.g.
//!   `AsyncDriver::is_unlocked`) for a consumer crate's own reentrancy
//!   tests. Never enabled in production; auto-enabled for this crate's own
//!   test builds via `cfg(test)`, opted into by a downstream crate through a
//!   `dev-dependencies` edge (see `crates/flui-view/Cargo.toml`).
//!
//! ```toml
//! [dependencies]
//! flui-scheduler = { version = "0.1", features = ["serde"] }
//! ```
//!
//! ## Platform Support
//!
//! This crate uses [`web_time`] for cross-platform time handling, supporting:
//! - Native platforms (Windows, macOS, Linux) via `std::time`
//! - WebAssembly via `performance.now()`
//!
//! [`UpdateScheduler`] and callback storage belong to one UI owner. Callbacks
//! and async tasks can capture `Rc`/`RefCell` state. [`FrameWaker`] and task
//! wakers provide separate [`Send`] + [`Sync`] wake capabilities without
//! retaining UI callback storage. [`AsyncDriver`] and [`PostFrameHandle`]
//! address the exact owner lifetime.
//!
//! [`web_time`]: https://docs.rs/web-time

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// `[workspace.lints]` supplies the baseline; this crate already holds the
// stricter missing-docs bar.
#![deny(missing_docs)]

// Core modules
pub mod frame;
// The composition-root frame-drive seam (ADR-0081 §4).
#[doc(hidden)]
pub mod __runtime;
pub mod frame_clock;
pub mod frame_histogram;
pub mod frame_telemetry;
pub mod scheduler;

// Type-safe primitives
pub mod async_driver;
pub mod id;
pub mod timer;

// Re-exports - Core types
pub use async_driver::{AsyncDriver, BoxedTask, TaskToken};
/// [`FrameSnapshot::address`]'s type and the two ids it pairs — re-exported
/// so a consumer of this crate's frame telemetry (e.g. `flui-devtools`'
/// `timeline` feature) can name them without an extra, redundant direct
/// dependency on `flui-foundation` just to construct/match one of this
/// crate's own public field types.
pub use flui_foundation::{PresentationAddress, PresentationId, UiRuntimeId};
pub use frame_clock::{
    ClockSource, DemandKind, DemandMask, FrameClock, FrameTimes, FramesInFlight, PollDecision,
    SkipReason,
};
pub use frame_histogram::{
    FRAME_RUN_GAP, FramePace, LatencyHistogram, frame_interval_histogram, frame_pace,
    input_to_present_histogram, produce_to_present_histogram,
};
pub use frame_telemetry::{
    FRAME_HISTORY_CAPACITY, FramePhaseDurations, FrameSnapshot, FrameTelemetryCut, InputEpoch,
    InputEpochId, InputEpochs, MAX_COALESCED_INPUT_EPOCHS, PresentOutcome,
};
pub use post_frame::{
    OwnerFrame, OwnerFrameError, PostFrameHandle, PostFrameRejection, PostFrameScheduleError,
    PresentationFrameError, PresentationFrameScope, PresentationScopeFactory,
};
pub use scheduler::execution::ExecutionError;
pub use timer::{Sleep, TimerHandle};
mod post_frame;

mod completion_wake;
mod wake_delivery;

#[cfg(test)]
mod table_test;

pub use frame::{
    AppLifecycleState, FrameId, FramePhase, FrameTiming, LifecycleStateCallback, PostFrameCallback,
    SchedulerPhase,
};
/// The instant type the frame clock is stamped with. `std::time::Instant` on
/// native, a `performance.now()` shim on wasm32 — re-exported so a binding can
/// name the frame drive's timestamp without depending on `web_time`.
pub use web_time::Instant;
// Re-exports - ID types (opaque foundation identities)
pub use id::CallbackId;
pub use scheduler::{
    FrameCompletionFuture, FrameOutcome, FrameWaker, SchedulerBuilder, SchedulerClosed,
    UpdateScheduler, WeakUpdateScheduler,
};

// Keep standalone prose examples inside the existing workspace doctest gate.
// Context-dependent fragments in these files are explicitly `rust,ignore`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

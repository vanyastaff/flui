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
//! [`std::time::Duration`] and recorded per frame as a [`FrameSnapshot`];
//! [`frame_pace`] and the latency histograms ([`LatencyHistogram`]) summarize
//! a run of them.
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

// Each public item has one path: the crate root. The modules are private, so
// a name never has a second spelling through its module.
mod async_driver;
mod completion_wake;
mod frame;
mod frame_clock;
mod frame_histogram;
mod frame_telemetry;
mod post_frame;
mod scheduler;
mod timer;
mod wake_delivery;

// The composition-root frame-drive seam (ADR-0081 §4).
#[doc(hidden)]
pub mod __runtime;

#[cfg(test)]
mod table_test;

pub use async_driver::{AsyncDriver, BoxedTask, TaskToken};
pub use frame::{
    AppLifecycleState, FrameId, FramePhase, FrameTiming, PostFrameCallback, SchedulerPhase,
};
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
pub use scheduler::{
    FrameCompletionFuture, FrameOutcome, FrameWaker, SchedulerBuilder, SchedulerClosed,
    UpdateScheduler, WeakUpdateScheduler,
};
pub use timer::{Sleep, TimerHandle};
/// The instant type the frame clock is stamped with. `std::time::Instant` on
/// native, a `performance.now()` shim on wasm32 — re-exported so a binding can
/// name the frame drive's timestamp without depending on `web_time`.
pub use web_time::Instant;

// Keep standalone prose examples inside the existing workspace doctest gate.
// Context-dependent fragments in these files are explicitly `rust,ignore`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

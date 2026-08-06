//! # FLUI UpdateScheduler
//!
//! Frame scheduling, task prioritization, and animation coordination for FLUI.
//!
//! `UpdateScheduler` owns *logical* time only — the phase machine, callback
//! queues, and the priority task queue — and makes no refresh-rate, display,
//! or surface assumption of its own; a caller supplies the frame's vsync
//! timestamp and an Idle-slice deadline to [`UpdateScheduler::drive_frame`]
//! (physical pacing is a presentation-owned concern, split out from this
//! crate).
//!
//! ## Architecture
//!
//! ```text
//! Application
//!     ↓
//! UpdateScheduler (orchestrates frames)
//!     ├─ TaskQueue (priority-based execution)
//!     ├─ TickerProvider (animation tickers)
//!     └─ FrameBudget (phase-duration stats)
//!
//! Frame Timeline:
//! BeginFrame → Tasks (Build/Layout/Paint) → EndFrame → Present
//! ```
//!
//! ## Key Components
//!
//! ### UpdateScheduler
//! Manages the frame lifecycle's phase machine:
//! - Schedule frame callbacks
//! - Post-frame callbacks
//! - Deadline-bounded Idle-priority task slice (Animation/Build always run)
//!
//! ### TaskQueue
//! Priority-based task execution:
//! - **Priority::UserInput** - Immediate (mouse, keyboard)
//! - **Priority::Animation** - High (smooth 60fps)
//! - **Priority::Build** - Normal (widget rebuilds)
//! - **Priority::Idle** - Low (background work)
//!
//! ### Ticker
//! Drives animations with frame-perfect timing:
//! ```rust
//! use flui_scheduler::Ticker;
//!
//! let mut ticker = Ticker::new();
//! ticker.start(|elapsed| {
//!     // Update animation
//! });
//! ```
//!
//! ### FrameBudget
//! Per-phase timing statistics against a caller-chosen target framerate:
//! - Tracks time spent in each phase
//! - Reports jank / over-budget statistics
//! - Provides frame skip policies
//!
//! ## Type-Safe Duration Wrappers
//!
//! ```rust
//! use flui_scheduler::duration::{FrameDuration, Milliseconds};
//!
//! let elapsed = Milliseconds::new(10.0); // 10ms elapsed
//! let budget = FrameDuration::try_from_fps(60).expect("fps > 0"); // ~60fps budget
//! assert!(!budget.is_over_budget(elapsed)); // Still under budget!
//! ```
//!
//! ## Type-Safe IDs
//!
//! Foundation markers prevent ID type confusion:
//! ```rust
//! use flui_scheduler::id::IdGenerator;
//! use flui_foundation::markers;
//!
//! let frame_gen = IdGenerator::<markers::Frame>::new();
//! let task_gen = IdGenerator::<markers::Task>::new();
//! let frame_id = frame_gen.next();
//! let task_id = task_gen.next();
//! // These are different types - can't be mixed!
//! ```
//!
//! ## Example Usage
//!
//! ```rust
//! use flui_scheduler::{FrameBudget, Priority, UpdateScheduler};
//!
//! let scheduler = UpdateScheduler::new();
//!
//! // Schedule a frame
//! scheduler.schedule_frame(Box::new(|frame_time| {
//!     println!("Frame start: {:?}", frame_time);
//! }));
//!
//! // Add task
//! scheduler.add_task(Priority::Animation, || {
//!     // Perform animation work
//! });
//!
//! // Execute frame (called by event loop)
//! scheduler.execute_frame();
//! ```
//!
//! ## Feature Flags
//!
//! - **`serde`** - Enable serialization support for duration types, priorities,
//!   and statistics. Adds `Serialize` and `Deserialize` derives to data types.
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
//! Cross-thread scheduler capabilities are [`Send`] + [`Sync`]. Bindings may
//! additionally own a deliberately owner-affine `LocalPostFrameLane` for UI
//! callbacks that capture `Rc`/`RefCell` state.
//!
//! ## Prelude
//!
//! ```rust
//! use flui_scheduler::prelude::*;
//!
//! let scheduler = UpdateScheduler::new();
//! let budget = FrameBudget::new(60); // 60 FPS target
//! ```
//!
//! [`web_time`]: https://docs.rs/web-time

// `[workspace.lints]` supplies the baseline; this crate already holds the
// stricter missing-docs bar.
#![deny(missing_docs)]

// Core modules
pub mod budget;
pub mod config;
pub mod frame;
pub mod frame_clock;
pub mod frame_histogram;
pub mod frame_telemetry;
pub mod scheduler;
pub mod task;
pub mod ticker;

// Type-safe primitives
pub mod async_driver;
pub mod duration;
pub mod id;

// Re-exports - Core types
pub use async_driver::{AsyncDriver, BoxedTask, TaskToken};
pub use budget::{
    AllPhaseStats, BudgetPolicy, FrameBudget, FrameBudgetBuilder, PhaseStats, SharedBudget,
};
pub use config::{
    PerformanceMode, PerformanceModeRequestHandle, SERVICE_EXT_TIME_DILATION, TimingsCallback,
    set_time_dilation, time_dilation,
};
/// [`FrameSnapshot::presentation`]'s type — re-exported so a consumer of
/// this crate's frame telemetry (e.g. `flui-devtools`' `timeline` feature)
/// can name it without an extra, redundant direct dependency on
/// `flui-foundation` just to construct/match one of this crate's own public
/// field types.
pub use flui_foundation::PresentationId;
pub use frame_clock::{ClockSource, DemandKind, DemandMask, FrameClock, PollDecision, SkipReason};
pub use frame_histogram::{
    LatencyHistogram, frame_interval_histogram, input_to_present_histogram,
    produce_to_present_histogram,
};
pub use frame_telemetry::{
    FRAME_HISTORY_CAPACITY, FrameSnapshot, InputEpoch, InputEpochId, InputEpochs,
    MAX_COALESCED_INPUT_EPOCHS, PresentOutcome,
};
pub use post_frame::{
    LocalPostFrameHandle, LocalPostFrameLane, LocalPostFrameScheduleError, PostFrameHandle,
};
/// The instant type the frame clock is stamped with. `std::time::Instant` on
/// native, a `performance.now()` shim on wasm32 — re-exported so a binding can
/// name `UpdateScheduler::drive_frame`'s `vsync_time` without depending on `web_time`.
mod post_frame;

pub use web_time::Instant;
// Re-exports - Duration types
pub use duration::{FrameDuration, Microseconds, Milliseconds, Percentage, Seconds};
pub use frame::{
    AppLifecycleState, FrameCallback, FrameId, FramePhase, FrameTiming, FrameTimingBuilder,
    LifecycleStateCallback, OneShotFrameCallback, PostFrameCallback, RecurringFrameCallback,
    SchedulerPhase,
};
// Re-exports - ID types (unified with flui-foundation)
pub use id::{CallbackId, Id, IdGenerator, Marker, markers};
pub use scheduler::{
    FrameCompletionFuture, IdleDeadline, SchedulerBuilder, UpdateScheduler, WeakUpdateScheduler,
};
pub use task::{Priority, PriorityCount, Task, TaskId, TaskQueue};
pub use ticker::{
    Ticker, TickerCallback, TickerCanceled, TickerFuture, TickerFutureOrCancel, TickerGroup,
    TickerId, TickerProvider, TickerState,
};

/// Prelude for common scheduler types
pub mod prelude {
    pub use crate::{
        BudgetPolicy, FrameBudget, FrameId, FramePhase, FrameTiming, OneShotFrameCallback,
        Priority, SchedulerPhase, Task, TaskId, TaskQueue, Ticker, TickerProvider, TickerState,
        UpdateScheduler,
        duration::{FrameDuration, Milliseconds, Percentage, Seconds},
    };
}

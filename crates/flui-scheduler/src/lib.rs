//! # FLUI Scheduler
//!
//! Frame scheduling, task prioritization, and animation coordination for FLUI.
//!
//! ## Architecture
//!
//! ```text
//! Application
//!     ↓
//! Scheduler (orchestrates frames)
//!     ├─ FrameScheduler (vsync coordination)
//!     ├─ TaskQueue (priority-based execution)
//!     ├─ TickerProvider (animation tickers)
//!     └─ FrameBudget (time management)
//!
//! Frame Timeline:
//! VSync → BeginFrame → Tasks (Build/Layout/Paint) → EndFrame → Present
//! ```
//!
//! ## Key Components
//!
//! ### FrameScheduler
//! Manages frame lifecycle and vsync coordination:
//! - Schedule frame callbacks
//! - Post-frame callbacks
//! - VSync integration
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
//! Enforces frame time limits (16.67ms for 60fps):
//! - Tracks time spent in each phase
//! - Cancels low-priority work if over budget
//! - Provides frame skip policies
//!
//! ## Type-Safe Duration Wrappers
//!
//! ```rust
//! use flui_scheduler::duration::{FrameDuration, Milliseconds};
//!
//! let elapsed = Milliseconds::new(10.0); // 10ms elapsed
//! let budget = FrameDuration::try_from_fps(60).expect("fps > 0"); // ~16.67ms budget
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
//! use flui_scheduler::{FrameBudget, Priority, Scheduler};
//!
//! let scheduler = Scheduler::new();
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
//! All types are [`Send`] + [`Sync`] and safe for use in multi-threaded
//! contexts.
//!
//! ## Prelude
//!
//! ```rust
//! use flui_scheduler::prelude::*;
//!
//! let scheduler = Scheduler::new();
//! let budget = FrameBudget::new(60); // 60 FPS target
//! ```
//!
//! [`web_time`]: https://docs.rs/web-time

#![deny(missing_docs)]
#![warn(clippy::all)]

// Core modules
pub mod budget;
pub mod config;
pub mod frame;
pub mod scheduler;
pub mod task;
pub mod ticker;
pub mod vsync;

// Type-safe primitives
pub mod duration;
pub mod id;

// Re-exports - Core types
pub use budget::{
    AllPhaseStats, BudgetPolicy, FrameBudget, FrameBudgetBuilder, PhaseStats, SharedBudget,
};
pub use config::{
    PerformanceMode, PerformanceModeRequestHandle, SERVICE_EXT_TIME_DILATION, SchedulingStrategy,
    TimingsCallback, default_scheduling_strategy, set_time_dilation, time_dilation,
};
// Re-exports - Duration types
pub use duration::{FrameDuration, Microseconds, Milliseconds, Percentage, Seconds};
// Re-export from flui-foundation for binding pattern
pub use flui_foundation::{BindingBase, HasInstance};
pub use frame::{
    AppLifecycleState, FrameCallback, FrameId, FramePhase, FrameTiming, FrameTimingBuilder,
    LifecycleStateCallback, OneShotFrameCallback, PostFrameCallback, RecurringFrameCallback,
    SchedulerPhase,
};
// Re-exports - ID types (unified with flui-foundation)
pub use id::{CallbackId, Id, IdGenerator, Marker, markers};
pub use scheduler::{FrameCompletionFuture, FrameSkipPolicy, Scheduler, SchedulerBuilder};
pub use task::{Priority, PriorityCount, Task, TaskId, TaskQueue};
pub use ticker::{
    Ticker, TickerCallback, TickerCanceled, TickerFuture, TickerFutureOrCancel, TickerGroup,
    TickerId, TickerProvider, TickerState,
};
pub use vsync::{VsyncCallback, VsyncMode, VsyncScheduler, VsyncStats};

/// Prelude for common scheduler types
pub mod prelude {
    pub use crate::{
        BudgetPolicy, FrameBudget, FrameId, FramePhase, FrameTiming, OneShotFrameCallback,
        Priority, Scheduler, SchedulerPhase, Task, TaskId, TaskQueue, Ticker, TickerProvider,
        TickerState,
        duration::{FrameDuration, Milliseconds, Percentage, Seconds},
    };
}

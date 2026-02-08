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
//! ## Advanced Type System Features
//!
//! This crate leverages Rust's advanced type system for zero-cost safety:
//!
//! ### Typestate Pattern
//! Compile-time state machine for tickers:
//! ```rust
//! use flui_scheduler::typestate::{TypestateTicker, Idle, Active};
//!
//! let ticker: TypestateTicker<Idle> = TypestateTicker::new();
//! let ticker: TypestateTicker<Active> = ticker.start(|elapsed| {});
//! ticker.tick(); // Only available in Active state!
//! let ticker: TypestateTicker<Idle> = ticker.stop();
//! ```
//!
//! ### Newtype Pattern
//! Type-safe duration wrappers prevent unit mixing:
//! ```rust
//! use flui_scheduler::duration::{Milliseconds, FrameDuration};
//!
//! let elapsed = Milliseconds::new(10.0); // 10ms elapsed
//! let budget = FrameDuration::from_fps(60); // ~16.67ms budget
//! assert!(!budget.is_over_budget(elapsed)); // Still under budget!
//! ```
//!
//! ### Type-Safe IDs
//! PhantomData markers prevent ID type confusion:
//! ```rust
//! use flui_scheduler::frame::FrameId;
//! use flui_scheduler::task::TaskId;
//!
//! let frame_id = FrameId::new();
//! let task_id = TaskId::new();
//! // These are different types - can't be mixed!
//! ```
//!
//! ### Typed Tasks
//! Compile-time priority checking:
//! ```rust
//! use flui_scheduler::task::TypedTask;
//! use flui_scheduler::traits::UserInputPriority;
//!
//! let task = TypedTask::<UserInputPriority>::new(|| {
//!     println!("High priority!");
//! });
//! ```
//!
//! ## Example Usage
//!
//! ```rust
//! use flui_scheduler::{Scheduler, Priority, FrameBudget};
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
//!   and statistics. Adds `Serialize` and `Deserialize` derives
//!   to data types.
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
//! All types are [`Send`] + [`Sync`] and safe for use in multi-threaded contexts.
//!
//! ## Preludes
//!
//! Two prelude modules are provided for convenient imports:
//!
//! - [`prelude`] - Core types for basic scheduling
//! - [`prelude_advanced`] - Includes typestate tickers, typed IDs, and typed tasks
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

// Advanced type system modules
pub mod duration;
pub mod id;
pub mod traits;
pub mod typestate;

// Re-exports - Core types
pub use budget::{
    AllPhaseStats, BudgetPolicy, FrameBudget, FrameBudgetBuilder, PhaseStats, SharedBudget,
};
pub use config::{
    default_scheduling_strategy, set_time_dilation, time_dilation, PerformanceMode,
    PerformanceModeRequestHandle, SchedulingStrategy, TimingsCallback, SERVICE_EXT_TIME_DILATION,
};
pub use frame::{
    AppLifecycleState, FrameCallback, FrameId, FramePhase, FrameTiming, FrameTimingBuilder,
    LifecycleStateCallback, OneShotFrameCallback, PostFrameCallback, RecurringFrameCallback,
    SchedulerPhase,
};
pub use scheduler::{
    CallbackId, FrameCompletionFuture, FrameSkipPolicy, Scheduler, SchedulerBuilder,
};

// Re-export from flui-foundation for binding pattern
pub use flui_foundation::{BindingBase, HasInstance};
pub use task::{Priority, PriorityCount, Task, TaskId, TaskQueue, TypedTask};
pub use ticker::{
    ScheduledTicker, ScheduledTickerCallback, Ticker, TickerCallback, TickerCanceled, TickerFuture,
    TickerFutureOrCancel, TickerGroup, TickerId, TickerProvider, TickerState,
};
pub use vsync::{VsyncCallback, VsyncDrivenScheduler, VsyncMode, VsyncScheduler, VsyncStats};

// Re-exports - Duration types
pub use duration::{FrameDuration, Microseconds, Milliseconds, Percentage, Seconds};

// Re-exports - ID types
pub use id::{
    CallbackIdMarker, FrameHandle, FrameIdMarker, Handle, IdGenerator, IdMarker, TaskHandle,
    TaskIdMarker, TickerIdMarker, TypedId,
};

// Re-exports - Trait types
pub use traits::{
    AnimationPriority, BuildPriority, FrameBudgetExt, FrameTimingExt, IdlePriority, PriorityExt,
    PriorityLevel, ToMilliseconds, ToSeconds, UserInputPriority,
};

// Re-exports - Typestate types
pub use typestate::{
    Active, Idle, Muted, Stopped, TickerState as TypestateTickerState, TypestateTicker,
};

/// Prelude for common scheduler types
pub mod prelude {
    // Core types
    pub use crate::{
        BudgetPolicy, FrameBudget, FrameId, FramePhase, FrameTiming, OneShotFrameCallback,
        Priority, ScheduledTicker, Scheduler, SchedulerPhase, Task, TaskId, TaskQueue, Ticker,
        TickerProvider, TickerState,
    };

    // Duration types
    pub use crate::duration::{FrameDuration, Milliseconds, Percentage, Seconds};

    // Extension traits for convenient method access
    pub use crate::traits::{
        FrameBudgetExt, FrameTimingExt, PriorityExt, ToMilliseconds, ToSeconds,
    };
}

/// Advanced prelude with typestate and type-safe IDs
pub mod prelude_advanced {
    pub use crate::prelude::*;

    // Typestate ticker
    pub use crate::typestate::{Active, Idle, Muted, Stopped, TickerState, TypestateTicker};

    // Typed tasks
    pub use crate::task::TypedTask;

    // Type-safe IDs
    pub use crate::id::{FrameHandle, Handle, IdGenerator, TypedId};

    // Type-level priorities
    pub use crate::traits::{
        AnimationPriority, BuildPriority, IdlePriority, PriorityLevel, UserInputPriority,
    };

    // Additional types
    pub use crate::{
        AllPhaseStats, FrameBudgetBuilder, FrameTimingBuilder, PhaseStats, PriorityCount,
        SchedulerBuilder, TickerGroup, VsyncMode, VsyncStats,
    };
}

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
//! ## Example Usage
//!
//! ```rust
//! use flui_scheduler::{Scheduler, Priority, FrameBudget};
//!
//! let mut scheduler = Scheduler::new();
//! scheduler.set_target_fps(60);
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

#![deny(missing_docs)]
#![warn(clippy::all)]

pub mod budget;
pub mod frame;
pub mod scheduler;
pub mod task;
pub mod ticker;
pub mod vsync;

// Re-exports
pub use budget::{BudgetPolicy, FrameBudget, PhaseStats};
pub use frame::{FrameCallback, FrameId, FramePhase, FrameTiming};
pub use scheduler::{Scheduler, SchedulerBinding};
pub use task::{Priority, Task, TaskId, TaskQueue};
pub use ticker::{Ticker, TickerCallback, TickerProvider};
pub use vsync::{VsyncCallback, VsyncScheduler};

/// Prelude for common scheduler types
pub mod prelude {
    pub use crate::{
        BudgetPolicy, FrameBudget, FramePhase, FrameTiming, Priority, Scheduler, SchedulerBinding,
        Task, TaskQueue, Ticker, TickerProvider,
    };
}

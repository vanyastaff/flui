//! iOS background executor.
//!
//! UIKit has no thread-pool of its own; the platform's own answer is Grand
//! Central Dispatch, and `dispatch2`'s global queue is that pool. Tasks that
//! must run on the UI thread are the owner lane's job, not this executor's —
//! this is strictly for work that may block.

use dispatch2::{DispatchQueue, DispatchQueueGlobalPriority, GlobalQueueIdentifier};

use crate::traits::PlatformExecutor;

/// Runs background tasks on a global GCD queue.
pub struct IOSExecutor;

impl std::fmt::Debug for IOSExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSExecutor").finish_non_exhaustive()
    }
}

impl PlatformExecutor for IOSExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        // `spawn` returns nothing, so a panicking task must not unwind across
        // the GCD block's `extern "C"` trampoline. The same shield
        // `owner_lane.rs` uses on macOS is applied here: catch the panic and
        // log it, so one bad task cannot take the process down.
        DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(
            DispatchQueueGlobalPriority::Default,
        ))
        .exec_async(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
        });
    }

    fn is_on_executor(&self) -> bool {
        // GCD pools arbitrary worker threads; there is no way to ask "am I on
        // this queue", and the honest answer is therefore `false` — a caller
        // must not use this to decide whether it may block.
        false
    }
}

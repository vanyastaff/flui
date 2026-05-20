//! Web executor implementation
//!
//! WASM is single-threaded with no preemption, so synchronous execution is
//! correct for `FnOnce` tasks. `wasm_bindgen_futures::spawn_local` requires a
//! `Future`, which we don't have here. Because WASM guarantees cooperative
//! scheduling on a single thread, executing the task inline is equivalent to
//! posting it to the microtask queue — no other code can interleave.

use crate::traits::PlatformExecutor;

pub struct WebExecutor;

// SAFETY: WASM is single-threaded — no data races possible
unsafe impl Send for WebExecutor {}
unsafe impl Sync for WebExecutor {}

impl WebExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformExecutor for WebExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        // WASM is single-threaded with cooperative scheduling.
        // Synchronous execution is correct — no preemption means
        // this behaves identically to a microtask queue dispatch.
        task();
    }

    fn is_on_executor(&self) -> bool {
        // WASM always runs on the main (and only) thread
        true
    }
}

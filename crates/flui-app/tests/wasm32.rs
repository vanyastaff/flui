//! The public execution API, EXECUTED on wasm32.
//!
//! `flui-app`'s execution layer documents that "on wasm32 the same API is
//! sequential" — compute jobs run inline and IO is driven without a thread
//! pool. Until issue #985 nothing in this workspace ever ran wasm output, so
//! that sentence was backed by `cargo check` and nothing else.
//!
//! Scope, stated so a green here is not read as more than it is. This exercises
//! the **public** seam an embedder actually touches — `DeterministicExecutors`
//! and the `HostComputePool` / `HostIoPool` traits behind
//! `AppConfig::with_executors`. It does NOT reach `ExecutionServices`' own
//! `#[cfg(target_arch = "wasm32")]` branches, which are `pub(crate)` and need a
//! unit test; flui-app's lib-test target does not build for wasm32 yet (14
//! errors, all of them test code reaching `cfg(not(wasm32))`-gated APIs). That
//! half is tracked in #985, and #557's wasm criterion is not closed by this
//! file alone.
//!
//! Run with `just wasm-test`.

#![cfg(target_arch = "wasm32")]

use std::sync::{Arc, Mutex};

use flui_app::{DeterministicExecutors, HostComputePool, HostIoPool};
use wasm_bindgen_test::wasm_bindgen_test;

/// Compute work submitted through the host seam runs, and runs **in submission
/// order on the calling thread** — which is the whole content of "sequential
/// where necessary". A threaded native pool could complete the same three jobs
/// in any order, so recording the order (rather than only that all three ran)
/// is what makes this assertion about sequencing at all.
///
/// The assertion *before* `run_until_idle` is load-bearing too: without it the
/// test would pass equally against a pool that ran each job eagerly at submit
/// time, which is a different contract.
#[wasm_bindgen_test]
fn compute_jobs_run_in_submission_order_on_the_calling_thread() {
    let executors = DeterministicExecutors::new();
    let order: Arc<Mutex<Vec<char>>> = Arc::new(Mutex::new(Vec::new()));

    for label in ['a', 'b', 'c'] {
        let sink = Arc::clone(&order);
        executors
            .spawn_job(Box::new(move || sink.lock().unwrap().push(label)))
            .expect("the deterministic pool refuses nothing");
    }

    assert!(
        order.lock().unwrap().is_empty(),
        "queued work must not run before the driver does"
    );

    let progressed = executors.run_until_idle();
    assert_eq!(progressed, 3, "every queued job should have run");
    assert_eq!(*order.lock().unwrap(), vec!['a', 'b', 'c']);
}

/// An IO future submitted through the host seam is polled to completion by that
/// same single-threaded driver. There is no executor thread on wasm32 to fall
/// back on, so a future that is queued and never polled would simply never
/// resolve — with nothing to observe but work that quietly does not happen.
#[wasm_bindgen_test]
fn an_io_future_is_driven_to_completion_without_a_thread_pool() {
    let executors = DeterministicExecutors::new();
    let resolved = Arc::new(Mutex::new(false));

    let flag = Arc::clone(&resolved);
    executors
        .spawn_future(Box::pin(async move {
            *flag.lock().unwrap() = true;
        }))
        .expect("the deterministic pool refuses nothing");

    assert!(
        !*resolved.lock().unwrap(),
        "the future must not be polled before the driver runs"
    );
    executors.run_until_idle();
    assert!(
        *resolved.lock().unwrap(),
        "the future never resolved -- nothing polled it"
    );
}

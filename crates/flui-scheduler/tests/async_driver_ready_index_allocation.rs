//! The actual complexity proof for issue #1056's ready-index rewrite:
//! [`OwnerFrame::poll_ready`] must cost allocations proportional to *ready*
//! work, not resident tasks. `cargo xtask ci` has no bench step, so this test — not
//! `benches/async_driver_pump.rs` — is the merge-blocking gate.
//!
//! A dedicated integration-test binary (never the crate's `--lib` unit
//! tests, and never a criterion bench) specifically so installing a
//! counting `#[global_allocator]` here cannot affect any other test binary
//! in this workspace — each `tests/*.rs` file compiles to its own process,
//! and `#[global_allocator]` is process-wide. Same pattern as
//! `frame_telemetry_allocation.rs` in this same crate.
//!
//! Each task retains one waker from spawn through retirement. Polling clones
//! that waker without allocating; the ping-pong ready buffers likewise reuse
//! capacity after warm-up. Both idle and steadily ready pumps must therefore
//! allocate nothing, including self-wakes during a poll.
//!
//! # One test, not several, and why
//!
//! This file's global allocator counters are process-wide, and this file
//! has exactly ONE `#[test]` for exactly that reason: under bare `cargo
//! test` (in-process thread pool by default, unlike this workspace's
//! standard `cargo nextest run`, one process per test), two tests in this
//! binary could run concurrently on separate threads, and one's real
//! allocations could land inside the other's measured window, intermittently
//! reporting a false allocation. Folding every claim into one test removes
//! the possibility of cross-test interference structurally.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::task::Poll;

use flui_scheduler::{OwnerFrame, UpdateScheduler};

// Per-thread, not process-global: a `GlobalAlloc` sees every thread's
// allocations, and libtest runs a test on a spawned thread while its own
// harness thread lives on, so a process-wide counter would charge any other
// thread's work to the measured window. `Cell<usize>` in a const-initialised
// thread-local has no drop glue and no lazy init, so reading it cannot
// itself allocate or run during TLS teardown.
thread_local! {
    static ALLOC_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Add to the counter without allocating or panicking during TLS teardown.
fn bump(by: usize) {
    let _ = ALLOC_COUNT.try_with(|c| c.set(c.get().wrapping_add(by)));
}

/// Read the counter; `0` if this thread's TLS is already gone.
fn read() -> usize {
    ALLOC_COUNT.try_with(Cell::get).unwrap_or(0)
}

struct CountingAllocator;

// SAFETY: `alloc`/`dealloc` forward unchanged to `System`, so every pointer
// is freed by the allocator that produced it. `realloc`/`alloc_zeroed` are
// deliberately NOT overridden: `GlobalAlloc`'s defaults decompose them into
// `self.alloc` + copy + `self.dealloc`, which still lands in `System` and
// still counts through this wrapper's `alloc` override — conservative for
// this harness (an in-place `System::realloc` growth is counted as an
// alloc), never a miss. The measured region is single-threaded (the test
// thread drives everything), so the `Relaxed`-equivalent `Cell` counter is
// sound: nothing else touches it concurrently.
#[expect(
    unsafe_code,
    reason = "test-only GlobalAlloc wrapper forwarding to System, matching frame_telemetry_allocation.rs's pattern"
)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump(1);
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` are the caller's own, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Dormant tasks and steady self-waking tasks both allocate nothing once warm.
#[test]
fn poll_ready_costs_zero_extra_allocations_once_warm() {
    // (a) R=0 at N in {0, 100_000}: dormant tasks must never be scanned.
    for dormant in [0usize, 100_000] {
        let scheduler = UpdateScheduler::new();
        let frame = OwnerFrame::new(&scheduler);
        let driver = frame.async_driver();
        driver.set_request_frame(|| {});
        let mut tokens = Vec::with_capacity(dormant);
        for _ in 0..dormant {
            tokens.push(driver.spawn_local(Box::pin(std::future::pending::<()>())));
        }

        // Warm-up: the first pump polls every freshly spawned task once
        // (spawn seeds `ready`), making all of them dormant. Excluded from
        // the measured window, same convention as frame_telemetry's.
        assert_eq!(frame.poll_ready(), dormant, "the warm-up pump");
        assert_eq!(
            frame.ready_task_count(),
            0,
            "every task is dormant after warm-up"
        );

        let count_before = read();
        const EMPTY_PUMPS: usize = 20;
        for _ in 0..EMPTY_PUMPS {
            assert_eq!(frame.poll_ready(), 0, "R=0 must poll nothing");
        }
        let allocations = read() - count_before;

        eprintln!(
            "async_driver_ready_index_allocation: N={dormant} dormant, R=0: \
             {allocations} allocations across {EMPTY_PUMPS} empty pumps"
        );
        assert_eq!(
            allocations, 0,
            "N={dormant} dormant tasks: {allocations} allocations across {EMPTY_PUMPS} empty \
             pumps -- poll_ready must scale with ready work, not resident tasks (issue #1056)"
        );

        drop(tokens);
    }

    // (b) Steady R=64 self-re-waking tasks: zero allocations after the first pump.
    const READY_TASKS: usize = 64;
    const STEADY_PUMPS: usize = 4;

    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler);
    let driver = frame.async_driver();
    driver.set_request_frame(|| {});
    let mut tokens = Vec::with_capacity(READY_TASKS);
    for _ in 0..READY_TASKS {
        tokens.push(driver.spawn_local(Box::pin(std::future::poll_fn(|cx| {
            // Self-wake on every poll, forever: R stays 64 pump after pump.
            cx.waker().wake_by_ref();
            Poll::<()>::Pending
        }))));
    }

    // Warm-up settles the ready index capacity; spawn already created each waker.
    assert_eq!(frame.poll_ready(), READY_TASKS, "the warm-up pump");

    let count_before = read();
    for _ in 0..STEADY_PUMPS {
        assert_eq!(
            frame.poll_ready(),
            READY_TASKS,
            "every task re-wakes itself every pump"
        );
    }
    let allocations = read() - count_before;

    eprintln!(
        "async_driver_ready_index_allocation: R={READY_TASKS} steady self-re-waking tasks: \
         {allocations} allocations across {STEADY_PUMPS} pumps after warm-up"
    );
    assert_eq!(
        allocations, 0,
        "steady self-waking tasks must reuse both their wakers and the ready index"
    );

    drop(tokens);
}

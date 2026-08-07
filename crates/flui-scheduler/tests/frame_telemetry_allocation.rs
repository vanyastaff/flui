//! Profiling evidence for issue #556's frame-telemetry seam
//! (`crate::frame_telemetry`, `FrameClock::stamp_input_epoch`/
//! `record_frame`): the hot input-stamp -> produce -> record path must not
//! allocate per frame, even with telemetry enabled on every pump.
//!
//! A dedicated integration-test binary (never the crate's `--lib` unit
//! tests, and never a criterion bench) specifically so installing a
//! counting `#[global_allocator]` here cannot affect any other test binary
//! in this workspace — each `tests/*.rs` file compiles to its own process,
//! and `#[global_allocator]` is process-wide. Same pattern as
//! `flui-engine/tests/raster_backpressure_allocation.rs`, which proved the
//! equivalent claim for the raster in-flight accounting path this issue
//! also added.
//!
//! # One test, not two, and why
//!
//! This file's global allocator counters are process-wide, and this file
//! has exactly ONE `#[test]` below for exactly that reason: an EARLIER
//! version of this file split the positive claim (the frame path allocates
//! nothing) and its negative control (`frames_since` DOES allocate, so the
//! positive claim is not simply vacuous) into two separate tests. Under
//! `cargo nextest run` (this workspace's standard runner, one process per
//! test) that was fine; under bare `cargo test` (in-process thread pool by
//! default) the two tests could run concurrently on separate threads, and
//! the negative control's own real allocation could land inside the
//! positive test's measured window, intermittently reporting a false
//! allocation. A test binary that is only correct under ONE runner is a
//! trap for anyone who reaches for `cargo test` locally instead of
//! `nextest` — folding both claims into one test removes the possibility
//! of cross-test interference structurally (nothing else in this binary
//! can run concurrently with itself), so the binary is correct under
//! either runner, not just the one this workspace standardizes on.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_scheduler::{
    ClockSource, DemandKind, FrameClock, PollDecision, PresentOutcome, PresentationId,
};

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

// SAFETY: `alloc`/`dealloc` forward unchanged to `System`, so every pointer
// is freed by the allocator that produced it. `realloc`/`alloc_zeroed` are
// deliberately NOT overridden: `GlobalAlloc`'s defaults decompose them into
// `self.alloc` + copy + `self.dealloc`, which still lands in `System` and
// still counts through this wrapper's `alloc` override — conservative for
// this harness (an in-place `System::realloc` growth is counted as an
// alloc), never a miss. The measured region is single-threaded (the test
// thread drives everything), so the `Relaxed` counters are sound: program
// order plus per-location coherence make the post-loop load observe every
// bump.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
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

/// Both halves of this harness's claim, pinned in ONE test so they can
/// never interleave across threads regardless of runner (see this file's
/// own module doc's "One test, not two, and why"):
///
/// 1. **The measured claim:** after warmup, every further
///    stamp-input -> poll(Produce) -> record_frame cycle allocates exactly
///    zero bytes — even though telemetry (input-epoch stamping and the
///    fixed-capacity history ring) runs on every one of them.
/// 2. **The negative control the claim above depends on:** `frames_since`
///    (the consumer PULL side, never claimed to be zero-allocation — see
///    `crate::frame_telemetry`'s own module doc) DOES allocate. Without
///    this, a broken counting allocator (or a no-op-by-accident harness)
///    would make claim 1 vacuous. Run AFTER claim 1's own loop, on the
///    SAME clock, so nothing about it can retroactively taint the
///    already-completed measurement above.
#[test]
fn frame_path_allocates_nothing_while_frames_since_pull_side_does() {
    let clock = FrameClock::with_source(ClockSource::Platform);

    // Warmup: settles this process's first-allocation costs (e.g. any
    // one-time lazy static / TLS init in `web_time` or `std`) that are not
    // part of the per-frame path this issue added.
    clock.mark_demand(DemandKind::Dirty);
    let now = clock.now();
    assert_eq!(clock.poll(now), PollDecision::Produce, "warmup produce");
    let _ = clock.record_frame(
        PresentationId::new(1),
        now,
        now,
        now,
        now,
        PresentOutcome::Presented,
    );

    const CYCLES: usize = 10_000;
    let mut cycles_with_allocation = 0usize;
    let mut worst_cycle_bytes = 0usize;
    let mut frame_path_alloc_calls = 0usize;
    let mut frame_path_alloc_bytes = 0usize;

    for _ in 0..CYCLES {
        let count_before = ALLOC_COUNT.load(Ordering::Relaxed);
        let bytes_before = ALLOC_BYTES.load(Ordering::Relaxed);

        let arrival = clock.now();
        let _epoch_id = clock.stamp_input_epoch(arrival);
        clock.mark_demand(DemandKind::Dirty);
        let now = clock.now();
        let decision = clock.poll(now);
        assert_eq!(decision, PollDecision::Produce, "every cycle must produce");
        let _snapshot = clock.record_frame(
            PresentationId::new(1),
            now,
            now,
            now,
            now,
            PresentOutcome::Presented,
        );

        let calls_this_cycle = ALLOC_COUNT.load(Ordering::Relaxed) - count_before;
        let bytes_this_cycle = ALLOC_BYTES.load(Ordering::Relaxed) - bytes_before;
        frame_path_alloc_calls += calls_this_cycle;
        frame_path_alloc_bytes += bytes_this_cycle;
        if calls_this_cycle != 0 {
            cycles_with_allocation += 1;
            worst_cycle_bytes = worst_cycle_bytes.max(bytes_this_cycle);
        }
    }

    eprintln!(
        "frame_telemetry_allocation: {CYCLES} stamp+poll+record cycles -- frame path (the \
         claim under test): {frame_path_alloc_calls} allocating calls, \
         {frame_path_alloc_bytes} bytes total, {worst_cycle_bytes} bytes on the worst single \
         cycle, {cycles_with_allocation} cycles allocated at least once"
    );

    assert_eq!(
        cycles_with_allocation, 0,
        "{cycles_with_allocation} of {CYCLES} stamp+poll+record cycles allocated at least \
         once (worst cycle: {worst_cycle_bytes} bytes) — the frame-telemetry path this issue \
         added (InputEpochs, FrameHistory) must be zero-allocation on every steady-state cycle"
    );

    // Negative control: same clock, right after the measured loop above --
    // if this did NOT allocate, the zero-allocation claim just pinned would
    // be proving nothing (a no-op counting allocator would pass it too).
    let bytes_before_pull = ALLOC_BYTES.load(Ordering::Relaxed);
    let pulled = clock.frames_since(None);
    let bytes_after_pull = ALLOC_BYTES.load(Ordering::Relaxed);

    assert!(!pulled.is_empty());
    assert!(
        bytes_after_pull > bytes_before_pull,
        "frames_since must allocate its returned Vec — if it didn't, the frame-path \
         zero-allocation claim above would be proving nothing"
    );
}

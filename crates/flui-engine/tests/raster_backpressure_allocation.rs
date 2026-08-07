//! Profiling evidence for issue #556's raster-backpressure accounting seam:
//! the hot `RasterHandle::submit` → `RasterOwner::pump` → retire path this
//! issue added (`InFlightTicket` increment/decrement, the wake-hook lookup)
//! must not allocate per frame.
//!
//! A dedicated integration-test binary (never the crate's `--lib` unit
//! tests, and never the criterion bench) specifically so installing a
//! counting `#[global_allocator]` here cannot affect any other test binary
//! in this workspace — each `tests/*.rs` file compiles to its own process,
//! and `#[global_allocator]` is process-wide.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use flui_engine::{
    CanvasLayer, DamageRegion, EngineError, Layer, RasterBackend, RasterOwner, Scene, SceneSnapshot,
};
use flui_foundation::{
    FrameEpoch, PresentationAddress, PresentationId, RealmId, SurfaceGeneration,
};
use flui_types::Size;
use flui_types::geometry::{Pixels, Rect};

// Per-thread, not process-global: a `GlobalAlloc` sees every thread's
// allocations, so process-wide counters charge any other thread's work to the
// measured window. libtest runs a test on a spawned thread while its own
// harness thread lives on, which made the measurement load-sensitive and
// intermittently wrong. `Cell<usize>` in a const-initialised thread-local has
// no drop glue and no lazy init, so reading it cannot itself allocate or run
// during TLS teardown.
thread_local! {
    static ALLOC_COUNT: Cell<usize> = const { Cell::new(0) };
    static ALLOC_BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Add to a counter without allocating or panicking during TLS teardown.
fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>, by: usize) {
    let _ = counter.try_with(|c| c.set(c.get().wrapping_add(by)));
}

/// Read a counter; `0` if this thread's TLS is already gone.
fn read(counter: &'static std::thread::LocalKey<Cell<usize>>) -> usize {
    counter.try_with(Cell::get).unwrap_or(0)
}

struct CountingAllocator;

// SAFETY: `alloc` and `dealloc` forward their arguments unchanged to `System`,
// the platform default allocator, so a pointer is always freed by the
// allocator that produced it. `realloc` and `alloc_zeroed` are deliberately
// NOT overridden: `GlobalAlloc`'s defaults decompose them into `self.alloc` +
// copy + `self.dealloc`, which lands in `System` too and, as a side effect,
// counts an in-place `System::realloc` growth as an alloc — conservative for
// this harness, never a miss. Overriding either one later MUST keep the
// counter bumps, or every `Vec` growth becomes invisible and the
// zero-allocation assertion silently turns into a false pass. (The workspace's
// other counting allocator, in `flui-interaction`'s pointer-route test,
// overrides all four and forwards each to `System` — do not reconcile the two
// by copying its shape without carrying the counters across.)
//
// The `Relaxed` counters are sound because the measured region is
// single-threaded: `submit` and `pump` both run on the test thread, so program
// order plus per-location coherence make the post-loop load observe every
// bump. Driving the threaded owner (`run_until_shutdown` on a spawned thread)
// from this harness would break that precondition and require acquire/release.
// `System` itself upholds `GlobalAlloc`'s contract; this wrapper adds no new
// invariant for the caller to uphold.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump(&ALLOC_COUNT, 1);
        bump(&ALLOC_BYTES, layout.size());
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

/// The minimum possible per-frame work — isolates the accounting/mailbox
/// overhead this test measures from any real GPU cost.
#[derive(Default)]
struct NoOpBackend;

impl RasterBackend for NoOpBackend {
    fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
        Ok(true)
    }

    fn resize(&mut self, _width: u32, _height: u32) {}

    fn is_device_lost(&self) -> bool {
        false
    }

    fn mark_dirty(&mut self, _rect: Rect<Pixels>) {}

    fn mark_full_repaint(&mut self) {}

    fn has_damage(&self) -> bool {
        true
    }

    fn size(&self) -> (u32, u32) {
        (0, 0)
    }

    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}

fn address() -> PresentationAddress {
    PresentationAddress {
        realm_id: RealmId::new(1),
        presentation_id: PresentationId::new(1),
    }
}

fn frame(epoch: FrameEpoch) -> SceneSnapshot {
    SceneSnapshot::new(
        address(),
        epoch,
        SurfaceGeneration::ZERO,
        DamageRegion::Full,
        Scene::from_layer(Size::ZERO, Layer::from(CanvasLayer::new()), 0),
    )
}

/// The measured claim: after warmup (the owner's own one-time
/// `Arc<RasterMailbox>`/channel construction, and the first `tracing`
/// callsite's one-time interest registration, are NOT part of the per-frame
/// accounting path this issue added), every further submit→pump→retire
/// cycle allocates exactly zero bytes.
///
/// `frame(epoch)` — building the `SceneSnapshot` itself — is deliberately
/// measured OUTSIDE the checked region: constructing the caller's own scene
/// is not this issue's accounting path, and asserting zero allocation for
/// arbitrary scene construction would be a false claim about a mechanism
/// this test does not touch.
#[test]
fn submit_pump_retire_cycle_allocates_nothing_on_the_accounting_path() {
    let (mut owner, handle, _ack_rx, _shutdown_complete_rx) =
        RasterOwner::new(NoOpBackend, address());

    // Warmup: settles the owner's one-time construction cost and the first
    // `tracing::instrument`d call's one-time callsite-interest caching,
    // neither of which is part of the steady-state accounting path below.
    handle
        .submit(frame(FrameEpoch::ZERO.next()))
        .expect("warmup submit");
    let _ = owner.pump();

    const CYCLES: usize = 10_000;
    let mut epoch = FrameEpoch::ZERO.next();
    let mut cycles_with_allocation = 0usize;
    let mut worst_cycle_bytes = 0usize;
    // Accumulated strictly from the per-cycle submit+pump deltas below —
    // NOT from a whole-loop before/after snapshot, which would also count
    // every `frame(epoch)` call's own (excluded, undisputed) allocation.
    let mut accounting_path_alloc_calls = 0usize;
    let mut accounting_path_alloc_bytes = 0usize;
    let mut frame_construction_alloc_bytes = 0usize;

    for _ in 0..CYCLES {
        epoch = epoch.next();

        let bytes_before_frame_construction = read(&ALLOC_BYTES);
        let snapshot = frame(epoch); // outside the checked region -- see doc above
        frame_construction_alloc_bytes += read(&ALLOC_BYTES) - bytes_before_frame_construction;

        let count_before = read(&ALLOC_COUNT);
        let bytes_before = read(&ALLOC_BYTES);
        handle.submit(snapshot).expect("submit");
        let _ = owner.pump();
        let calls_this_cycle = read(&ALLOC_COUNT) - count_before;
        let bytes_this_cycle = read(&ALLOC_BYTES) - bytes_before;
        accounting_path_alloc_calls += calls_this_cycle;
        accounting_path_alloc_bytes += bytes_this_cycle;
        if calls_this_cycle != 0 {
            cycles_with_allocation += 1;
            worst_cycle_bytes = worst_cycle_bytes.max(bytes_this_cycle);
        }
    }

    // Reported unconditionally (not just on failure) — the plan calls for
    // numbers, not an assertion alone. The two totals are kept separate on
    // purpose: `frame_construction_alloc_bytes` is `SceneSnapshot`/`Scene`/
    // `Layer` construction cost this test does NOT claim anything about;
    // `accounting_path_alloc_*` is the number the assertion below is about.
    eprintln!(
        "raster_backpressure_allocation: {CYCLES} submit+pump+retire cycles -- \
         accounting path (submit+pump, the claim under test): {accounting_path_alloc_calls} \
         allocating calls, {accounting_path_alloc_bytes} bytes total, \
         {worst_cycle_bytes} bytes on the worst single cycle, {cycles_with_allocation} \
         cycles allocated at least once; frame(epoch) construction (excluded from the \
         claim, reported for context only): {frame_construction_alloc_bytes} bytes total"
    );

    assert_eq!(
        cycles_with_allocation, 0,
        "{cycles_with_allocation} of {CYCLES} submit+pump+retire cycles allocated \
         at least once (worst cycle: {worst_cycle_bytes} bytes) — the accounting \
         path this issue added (InFlightTicket, the wake-hook lookup) must be \
         zero-allocation on every steady-state cycle"
    );
}

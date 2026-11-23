//! What a test thread asks of the heap, counted by a global allocator.
//!
//! A test target that counts allocations installs [`CountingAllocator`] as
//! its own global allocator; the kit cannot install it, because a
//! `#[global_allocator]` belongs to the binary:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOCATOR: flui_testing::heap::CountingAllocator =
//!     flui_testing::heap::CountingAllocator;
//! ```
//!
//! The counters are per thread, so libtest's harness thread and any
//! background thread a host starts add nothing to a test's figures, and two
//! readings of [`HeapCounts::now`] on the test thread bracket exactly the
//! work between them. In a binary that does not install the allocator every
//! reading is zero.

pub mod sites;

use std::alloc::{GlobalAlloc, Layout};

/// The allocator [`CountingAllocator`] forwards to: `dhat::Alloc`, which records the stack
/// of each allocation while a [`sites::SiteProfiler`] runs, with the `dhat` feature, and
/// [`std::alloc::System`] without it.
#[cfg(feature = "dhat")]
const INNER: dhat::Alloc = dhat::Alloc;
#[cfg(not(feature = "dhat"))]
const INNER: std::alloc::System = std::alloc::System;
use std::cell::Cell;

// `Cell<u64>` in a const-initialised thread-local has no drop glue and no
// lazy init, so the allocator reading it cannot itself allocate or run during
// TLS teardown.
thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static REALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<u64> = const { Cell::new(0) };
    static LIVE_BYTES: Cell<u64> = const { Cell::new(0) };
}

/// What this thread has asked of the heap so far; zeros once its TLS is gone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeapCounts {
    /// Allocations, a growth or shrink by `realloc` included.
    pub allocations: u64,
    /// The `realloc`s among them: a buffer that outgrew its capacity.
    pub reallocations: u64,
    /// The bytes those allocations asked for.
    pub allocated_bytes: u64,
    /// Allocated minus freed bytes, wrapping: only the difference of two
    /// readings means anything, which [`since`](Self::since) takes.
    pub live_bytes: u64,
}

impl HeapCounts {
    /// This thread's counters now.
    #[must_use]
    pub fn now() -> Self {
        let read = |counter: &'static std::thread::LocalKey<Cell<u64>>| {
            counter.try_with(Cell::get).unwrap_or(0)
        };
        Self {
            allocations: read(&ALLOCATIONS),
            reallocations: read(&REALLOCATIONS),
            allocated_bytes: read(&ALLOCATED_BYTES),
            live_bytes: read(&LIVE_BYTES),
        }
    }

    /// What happened between `earlier` and `self`: the heap asked of in
    /// between, and the bytes still live of what was allocated in between
    /// (net of what was freed).
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            allocations: self.allocations.saturating_sub(earlier.allocations),
            reallocations: self.reallocations.saturating_sub(earlier.reallocations),
            allocated_bytes: self.allocated_bytes.saturating_sub(earlier.allocated_bytes),
            live_bytes: self.live_bytes.wrapping_sub(earlier.live_bytes),
        }
    }
}

/// A global allocator that forwards to `INNER` ([`std::alloc::System`], or `dhat::Alloc`
/// with the `dhat` feature) and counts, per thread,
/// what it is asked: see the module docs for how a test target installs it.
#[derive(Clone, Copy, Debug, Default)]
pub struct CountingAllocator;

fn bump(counter: &'static std::thread::LocalKey<Cell<u64>>, by: u64) {
    let _ = counter.try_with(|count| count.set(count.get().saturating_add(by)));
}

thread_local! {
    /// Set while this thread is inside `INNER`. DHAT's allocator allocates for its own
    /// records (the stack it captures, its tables) through the global allocator, so those
    /// requests come back here nested; they are DHAT's, not the program's, and are not counted.
    static IN_INNER: Cell<bool> = const { Cell::new(false) };
}

/// The outermost allocator call of this thread, which counts; `enter` answers `None` for a
/// call nested inside `INNER`, and when the thread's TLS is gone.
struct InnerCall;

impl InnerCall {
    fn enter() -> Option<Self> {
        // A nested call builds no guard: its `Drop` would clear the outer call's flag.
        let outermost = IN_INNER.try_with(|inside| !inside.replace(true));
        if outermost == Ok(true) {
            Some(Self)
        } else {
            None
        }
    }

    /// Whether this thread is inside `INNER` now.
    fn nested() -> bool {
        IN_INNER.try_with(Cell::get).unwrap_or(true)
    }
}

impl Drop for InnerCall {
    fn drop(&mut self) {
        let _ = IN_INNER.try_with(|inside| inside.set(false));
    }
}

// SAFETY: `alloc`/`dealloc` forward unchanged to `INNER`, so every pointer
// is freed by the allocator that produced it. `realloc` is `GlobalAlloc`'s
// default spelled out so it can be counted: `self.alloc` + copy +
// `self.dealloc`, still `INNER`. `alloc_zeroed` keeps the default, through
// `self.alloc`.
#[expect(unsafe_code, reason = "a counting allocator forwards to System or DHAT")]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Held across `INNER`, so the allocations DHAT makes inside it are not counted.
        let Some(_inner) = InnerCall::enter() else {
            // SAFETY: `layout` is the caller's own, forwarded unchanged.
            return unsafe { INNER.alloc(layout) };
        };
        let size = layout.size() as u64;
        bump(&ALLOCATIONS, 1);
        bump(&ALLOCATED_BYTES, size);
        let _ = LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_add(size)));
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { INNER.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Held across `INNER`, so the frees DHAT makes inside it are not counted.
        let inner = InnerCall::enter();
        if inner.is_some() {
            let _ =
                LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_sub(layout.size() as u64)));
        }
        // SAFETY: `ptr`/`layout` are the caller's own, forwarded unchanged.
        unsafe { INNER.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if !InnerCall::nested() {
            bump(&REALLOCATIONS, 1);
        }
        // SAFETY: `realloc`'s contract makes `new_size` with `layout.align()`
        // a valid layout (non-zero, not overflowing `isize` once rounded).
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        // SAFETY: `new_layout` is valid, as above.
        let new_ptr = unsafe { self.alloc(new_layout) };
        if !new_ptr.is_null() {
            // SAFETY: both blocks are live and distinct, each at least
            // `min(old, new)` bytes long; `ptr` was allocated here with
            // `layout`, as `realloc`'s contract requires.
            unsafe {
                std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
        }
        new_ptr
    }
}

/// Rounds [`assert_rounds_plateau`] runs before it starts comparing: the first ones fill what a
/// process keeps for good (a font system, shaped-text and type tables, thread-local pools).
const PLATEAU_WARM_UP: usize = 3;
/// Rounds [`assert_rounds_plateau`] compares.
const PLATEAU_ROUNDS: usize = 8;
// The comparison is between the last warm-up round and the last compared one.
const _: () = assert!(PLATEAU_WARM_UP >= 1 && PLATEAU_ROUNDS >= 1);

/// Runs `round` again and again on this thread and panics when its live heap keeps growing:
/// the last round has to leave no more bytes live than the last warm-up round did.
///
/// A round mounts an application, drives it and drops it. Everything an owner thread holds is
/// `Rc` (ADR-0323 §1), so a cycle between an element, its state, a listener and a signal frees
/// nothing, and no tool reports it: each round leaves one more tree behind.
///
/// The counters are per thread: a block counts for the thread that allocated it and against
/// the thread that freed it. The assertion therefore holds for a round that does its work on
/// this thread, which is where an application's tree lives. A round that frees on another
/// thread what this one allocated reads as growth and fails with nothing leaked; a round that
/// frees here what another thread allocated reads as a fall, which hides a leak of that size.
///
/// # Panics
///
/// When the rounds leave more bytes live each time, and when no round allocated anything on
/// this thread: the test target has not installed [`CountingAllocator`], or the round does
/// its work elsewhere, and a flat row of readings would say nothing.
#[track_caller]
pub fn assert_rounds_plateau(mut round: impl FnMut()) {
    // An array, so the readings are no allocation of this function's own.
    let mut live = [0_i64; PLATEAU_WARM_UP + PLATEAU_ROUNDS];
    let start = HeapCounts::now();
    for reading in &mut live {
        round();
        *reading = HeapCounts::now().since(start).live_bytes.cast_signed();
    }
    assert!(
        HeapCounts::now().since(start).allocations > 0,
        "no round allocated on this thread: the test target does not install \
         `CountingAllocator`, or the round works on another thread"
    );
    let (first, last) = (
        live[PLATEAU_WARM_UP - 1],
        live[PLATEAU_WARM_UP + PLATEAU_ROUNDS - 1],
    );
    assert!(
        last <= first,
        "the last {PLATEAU_ROUNDS} rounds left {} more bytes live; after each round, against \
         the start: {live:?}",
        last - first
    );
}

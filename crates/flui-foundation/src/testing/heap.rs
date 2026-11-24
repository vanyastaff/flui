//! What a test thread asks of the heap, counted by the one counting global allocator.
//!
//! A test target that counts allocations installs [`CountingAllocator`] as its global
//! allocator. No library can install it, because a `#[global_allocator]` belongs to the
//! binary:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOCATOR: flui_foundation::testing::heap::CountingAllocator =
//!     flui_foundation::testing::heap::CountingAllocator;
//! ```
//!
//! The counters are per thread, so libtest's harness thread and any background thread a
//! host starts add nothing to a test's figures, and two readings of [`HeapCounts::now`] on
//! the test thread bracket exactly the work between them. In a binary that does not install
//! the allocator every reading is zero, and [`CountingAllocator::is_installed`] says so.
//!
//! With the `dhat` feature the allocator forwards to `dhat::Alloc`, which records the stack
//! of each allocation while a profiler runs (`flui_testing::heap::sites`); without it, to
//! [`std::alloc::System`].

use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;

/// The allocator [`CountingAllocator`] forwards to.
#[cfg(feature = "dhat")]
const INNER: dhat::Alloc = dhat::Alloc;
#[cfg(not(feature = "dhat"))]
const INNER: std::alloc::System = std::alloc::System;

// `Cell<u64>` in a const-initialised thread-local has no drop glue and no lazy init, so
// the allocator reading it cannot itself allocate or run during TLS teardown.
thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static REALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<u64> = const { Cell::new(0) };
    static LIVE_BYTES: Cell<u64> = const { Cell::new(0) };
    /// The largest block asked for since the innermost [`largest_allocation_during`] began.
    static LARGEST: Cell<u64> = const { Cell::new(0) };
    /// Set while this thread is inside `INNER`. DHAT's allocator allocates for its own
    /// records (the stack it captures, its tables) through the global allocator, so those
    /// requests come back here nested; they are DHAT's, not the program's, and are not
    /// counted.
    static IN_INNER: Cell<bool> = const { Cell::new(false) };
}

/// What this thread has asked of the heap so far; zeros once its TLS is gone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeapCounts {
    /// Allocations, a growth or shrink by `realloc` included.
    pub allocations: u64,
    /// The `realloc`s among them: a buffer that outgrew its capacity.
    pub reallocations: u64,
    /// The bytes those allocations asked for; a `realloc` asks for its new size.
    pub allocated_bytes: u64,
    /// Allocated minus freed bytes, wrapping: only the difference of two readings means
    /// anything, which [`since`](Self::since) takes.
    pub live_bytes: u64,
}

impl HeapCounts {
    /// This thread's counters now.
    #[must_use]
    pub fn now() -> Self {
        Self {
            allocations: read(&ALLOCATIONS),
            reallocations: read(&REALLOCATIONS),
            allocated_bytes: read(&ALLOCATED_BYTES),
            live_bytes: read(&LIVE_BYTES),
        }
    }

    /// What happened between `earlier` and `self`: the heap asked of in between, and the
    /// bytes still live of what was allocated in between (net of what was freed).
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            allocations: self.allocations.saturating_sub(earlier.allocations),
            reallocations: self.reallocations.saturating_sub(earlier.reallocations),
            allocated_bytes: self.allocated_bytes.saturating_sub(earlier.allocated_bytes),
            live_bytes: self.live_bytes.wrapping_sub(earlier.live_bytes),
        }
    }

    /// What `work` asked of this thread's heap, with what it returned.
    pub fn during<R>(work: impl FnOnce() -> R) -> (Self, R) {
        let before = Self::now();
        let result = work();
        (Self::now().since(before), result)
    }
}

/// The largest block `work` asked for on this thread, in bytes, with what it returned; zero
/// when it asked for none or the allocator is not installed.
///
/// A call inside another reports only its own blocks, and the outer call still sees them.
pub fn largest_allocation_during<R>(work: impl FnOnce() -> R) -> (u64, R) {
    /// Puts back the enclosing window's maximum, raised by this one's, on every exit.
    struct Window {
        enclosing: u64,
    }
    impl Drop for Window {
        fn drop(&mut self) {
            let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(self.enclosing)));
        }
    }
    let window = Window {
        enclosing: LARGEST.try_with(Cell::take).unwrap_or(0),
    };
    let result = work();
    let largest = read(&LARGEST);
    drop(window);
    (largest, result)
}

fn read(counter: &'static std::thread::LocalKey<Cell<u64>>) -> u64 {
    counter.try_with(Cell::get).unwrap_or(0)
}

fn bump(counter: &'static std::thread::LocalKey<Cell<u64>>, by: u64) {
    let _ = counter.try_with(|count| count.set(count.get().saturating_add(by)));
}

/// Counts one block of `size` bytes asked for, a `realloc`'s new size included.
fn count_block(size: u64) {
    bump(&ALLOCATIONS, 1);
    bump(&ALLOCATED_BYTES, size);
    let _ = LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_add(size)));
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
}

fn count_free(size: u64) {
    let _ = LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_sub(size)));
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
}

impl Drop for InnerCall {
    fn drop(&mut self) {
        let _ = IN_INNER.try_with(|inside| inside.set(false));
    }
}

/// A global allocator that forwards to `INNER` ([`std::alloc::System`], or `dhat::Alloc`
/// with the `dhat` feature) and counts, per thread, what it is asked: see the module docs
/// for how a test target installs it.
#[derive(Clone, Copy, Debug, Default)]
pub struct CountingAllocator;

impl CountingAllocator {
    /// Whether this binary's global allocator counts: one block is asked for and the
    /// thread's count read on each side. A test that asserts on counts first asserts this,
    /// since every count reads zero in a binary that does not install the allocator.
    #[must_use]
    pub fn is_installed() -> bool {
        let (counts, probe) =
            HeapCounts::during(|| std::hint::black_box(Vec::<u8>::with_capacity(1)));
        drop(probe);
        counts.allocations > 0
    }
}

#[expect(
    unsafe_code,
    reason = "a global allocator that counts, then forwards to System or DHAT"
)]
// SAFETY: every method forwards its arguments unchanged to the same method of `INNER`, so
// each pointer is resized or freed by the allocator that produced it; the counting touches
// only const-initialised thread-locals, which neither allocate nor run during TLS teardown.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Held across `INNER`, so the allocations DHAT makes inside it are not counted.
        let inner = InnerCall::enter();
        if inner.is_some() {
            count_block(layout.size() as u64);
        }
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { INNER.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let inner = InnerCall::enter();
        if inner.is_some() {
            count_block(layout.size() as u64);
        }
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { INNER.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Held across `INNER`, so the frees DHAT makes inside it are not counted.
        let inner = InnerCall::enter();
        if inner.is_some() {
            count_free(layout.size() as u64);
        }
        // SAFETY: `ptr`/`layout` are the caller's own, forwarded unchanged.
        unsafe { INNER.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let inner = InnerCall::enter();
        // SAFETY: `ptr`, `layout` and `new_size` are the caller's own, forwarded unchanged.
        let new_ptr = unsafe { INNER.realloc(ptr, layout, new_size) };
        // A failed `realloc` leaves the old block in place, so it is counted only on success:
        // one block of the new size asked for, the old one freed.
        if inner.is_some() && !new_ptr.is_null() {
            bump(&REALLOCATIONS, 1);
            count_block(new_size as u64);
            count_free(layout.size() as u64);
        }
        new_ptr
    }
}

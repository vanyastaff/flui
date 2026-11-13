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

use std::alloc::{GlobalAlloc, Layout, System};
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

/// A global allocator that forwards to [`System`] and counts, per thread,
/// what it is asked: see the module docs for how a test target installs it.
#[derive(Clone, Copy, Debug, Default)]
pub struct CountingAllocator;

fn bump(counter: &'static std::thread::LocalKey<Cell<u64>>, by: u64) {
    let _ = counter.try_with(|count| count.set(count.get().saturating_add(by)));
}

// SAFETY: `alloc`/`dealloc` forward unchanged to `System`, so every pointer
// is freed by the allocator that produced it. `realloc` is `GlobalAlloc`'s
// default spelled out so it can be counted: `self.alloc` + copy +
// `self.dealloc`, still `System`. `alloc_zeroed` keeps the default, through
// `self.alloc`.
#[expect(unsafe_code, reason = "a counting global allocator forwards to System")]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size() as u64;
        bump(&ALLOCATIONS, 1);
        bump(&ALLOCATED_BYTES, size);
        let _ = LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_add(size)));
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_sub(layout.size() as u64)));
        // SAFETY: `ptr`/`layout` are the caller's own, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        bump(&REALLOCATIONS, 1);
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

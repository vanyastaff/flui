//! What `Image::decode` owes bytes nobody vouches for. The property test of
//! `image_decode.rs` draws the bytes at random on every run; the fuzz target
//! `fuzz/image_decode.rs` lets libFuzzer mutate them toward code no input has
//! reached yet. Both binaries install [`LargestAllocation`], so the property
//! sees the largest block a decode asked for, which no assertion on the
//! decoded image can.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use flui_painting::paint::{Image, ImageDecodeError, MAX_DECODED_IMAGE_BYTES};

// `Cell<usize>` in a const-initialised thread-local has no drop glue and no
// lazy init, so the allocator reading it cannot itself allocate or run during
// TLS teardown. Per thread, so a test running beside the property adds
// nothing to its reading.
thread_local! {
    static LARGEST: Cell<usize> = const { Cell::new(0) };
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

/// The global allocator of a binary that holds the property: it forwards to
/// [`System`] and keeps, per thread, the largest block asked for and how many
/// blocks were asked for, a reallocation counting as one.
pub(crate) struct LargestAllocation;

fn note(size: usize) {
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

/// How many blocks `work` asked for on this thread, with what it returned.
/// Panics when the binary does not install [`LargestAllocation`].
#[allow(
    dead_code,
    reason = "the fuzz target includes this file and counts no allocations"
)]
pub(crate) fn allocations_during<R>(work: impl FnOnce() -> R) -> (u64, R) {
    let before = ALLOCATIONS.with(Cell::get);
    drop(std::hint::black_box(Vec::<u8>::with_capacity(16)));
    let probed = ALLOCATIONS.with(Cell::get);
    assert!(
        probed > before,
        "the binary does not install `LargestAllocation` as its global allocator"
    );
    let result = work();
    (ALLOCATIONS.with(Cell::get) - probed, result)
}

#[expect(
    unsafe_code,
    reason = "a global allocator that notes block sizes and forwards to System"
)]
// SAFETY: every method forwards its arguments unchanged to the same method of
// `System`, so each pointer is freed or resized by the allocator that made it.
unsafe impl GlobalAlloc for LargestAllocation {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: the caller's `layout`, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: the caller's `layout`, forwarded unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's `ptr` and `layout`, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        // SAFETY: the caller's `ptr`, `layout` and `new_size`, forwarded unchanged.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Decodes `bytes` and panics unless the result is an image within the limit
/// or a typed error, a refusal for size names a size past the limit, and no
/// block the decode asked for was larger than the limit: the decoder is given
/// [`MAX_DECODED_IMAGE_BYTES`] as its allocation limit, so an image that
/// compresses small but decodes huge is refused instead of allocating.
/// Returns the largest block the decode asked for.
pub(crate) fn decodes_within_the_limit_or_fails_typed(bytes: &[u8]) -> u64 {
    let probe = std::hint::black_box(Vec::<u8>::with_capacity(16));
    assert!(
        LARGEST.with(Cell::take) >= probe.capacity(),
        "the binary does not install `LargestAllocation` as its global allocator"
    );
    drop(probe);
    let decoded = Image::decode(bytes);
    let largest = u64::try_from(LARGEST.with(Cell::get)).expect("BUG: a block size fits u64");
    assert!(
        largest <= MAX_DECODED_IMAGE_BYTES,
        "the decode asked for a block of {largest} bytes, past the limit of \
         {MAX_DECODED_IMAGE_BYTES}"
    );
    match decoded {
        Ok(image) => {
            let decoded = u64::try_from(image.data().len()).expect("BUG: a length fits u64");
            assert!(
                decoded <= MAX_DECODED_IMAGE_BYTES,
                "{decoded} decoded bytes are past the limit"
            );
        }
        Err(ImageDecodeError::TooLarge { requested, limit }) => {
            assert!(
                requested > limit,
                "{requested} bytes were refused under a limit of {limit}"
            );
        }
        Err(_) => {}
    }
    largest
}

//! What `Image::decode` owes bytes nobody vouches for. The property test of
//! `image_decode.rs` draws the bytes at random on every run; the fuzz target
//! `fuzz/image_decode.rs` lets libFuzzer mutate them toward code no input has
//! reached yet. Both binaries install the foundation layer's
//! [`CountingAllocator`], so the property sees the largest block a decode
//! asked for, which no assertion on the decoded image can.

use flui_foundation::testing::heap::{CountingAllocator, HeapCounts, largest_allocation_during};
use flui_painting::paint::{Image, ImageDecodeError, MAX_DECODED_IMAGE_BYTES};

/// How many blocks `work` asked for on this thread, a reallocation counting
/// as one, with what it returned. Panics when the binary does not install
/// [`CountingAllocator`].
#[allow(
    dead_code,
    reason = "the fuzz target includes this file and counts no allocations"
)]
pub(crate) fn allocations_during<R>(work: impl FnOnce() -> R) -> (u64, R) {
    assert!(
        CountingAllocator::is_installed(),
        "the binary does not install `CountingAllocator` as its global allocator"
    );
    let (counts, result) = HeapCounts::during(work);
    (counts.allocations, result)
}

/// Decodes `bytes` and panics unless the result is an image within the limit
/// or a typed error, a refusal for size names a size past the limit, and no
/// block the decode asked for was larger than the limit: the decoder is given
/// [`MAX_DECODED_IMAGE_BYTES`] as its allocation limit, so an image that
/// compresses small but decodes huge is refused instead of allocating.
/// Returns the largest block the decode asked for.
pub(crate) fn decodes_within_the_limit_or_fails_typed(bytes: &[u8]) -> u64 {
    assert!(
        CountingAllocator::is_installed(),
        "the binary does not install `CountingAllocator` as its global allocator"
    );
    let (largest, decoded) = largest_allocation_during(|| Image::decode(bytes));
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

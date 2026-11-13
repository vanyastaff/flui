//! What `Image::decode` owes bytes nobody vouches for. The property test of
//! `image_decode.rs` draws the bytes at random on every run; the fuzz target
//! `fuzz/image_decode.rs` lets libFuzzer mutate them toward code no input has
//! reached yet.

use flui_painting::paint::{Image, ImageDecodeError, MAX_DECODED_IMAGE_BYTES};

/// Decodes `bytes` and panics unless the result is an image within the limit
/// or a typed error; a refusal for size names a size past the limit.
pub(crate) fn decodes_within_the_limit_or_fails_typed(bytes: &[u8]) {
    match Image::decode(bytes) {
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
}

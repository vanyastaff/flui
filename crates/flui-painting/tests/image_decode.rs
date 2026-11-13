//! `Image::decode` on bytes nobody vouches for: a header that asks for more
//! pixels than the limit is refused before a pixel buffer exists, and any
//! other bytes decode or come back as a typed error, never a panic.

use flui_painting::paint::{Image, ImageDecodeError, MAX_DECODED_IMAGE_BYTES};
use proptest::prelude::*;

use crate::foreign_image::decodes_within_the_limit_or_fails_typed;

/// A 4x2 PNG.
const TINY_PNG: &[u8] = include_bytes!("fixtures/tiny.png");

/// [`TINY_PNG`] with its header declaring `width` by `height`, the header's
/// checksum restated so the decoder reads past it.
fn png_declaring(width: u32, height: u32) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0_u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    let mut png = TINY_PNG.to_vec();
    // Signature (8), IHDR length (4), "IHDR" (4), then width and height.
    assert_eq!(&png[12..16], b"IHDR", "the fixture starts with its header");
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}

/// A well-formed image of each format this build decodes comes out at its own
/// size, every pixel present. The same three files seed the fuzz target.
pub(crate) fn each_format_decodes_to_its_pixels() {
    let fixtures: [(&str, &[u8]); 3] = [
        ("PNG", TINY_PNG),
        ("JPEG", include_bytes!("fixtures/tiny.jpg")),
        ("GIF", include_bytes!("fixtures/tiny.gif")),
    ];
    for (format, bytes) in fixtures {
        let image = Image::decode(bytes).expect("the fixture decodes");
        assert_eq!((image.width(), image.height()), (4, 2), "{format}");
        assert_eq!(image.data().len(), 4 * 2 * 4, "{format}");
    }
}

/// A 75-byte PNG whose header declares 60000x60000 (14.4 GB of RGBA8) is
/// refused with the size its header asked for. That figure comes only from
/// the check of the header's dimensions, which runs before the decoder
/// allocates its pixels: a refusal from the decoder's own allocation limit
/// cannot name it.
pub(crate) fn a_header_past_the_limit_is_refused_before_decoding() {
    let refused = Image::decode(&png_declaring(60_000, 60_000));
    assert_eq!(
        refused.expect_err("a 60000x60000 header is refused"),
        ImageDecodeError::TooLarge {
            requested: 60_000 * 60_000 * 4,
            limit: MAX_DECODED_IMAGE_BYTES,
        }
    );
    // The largest dimensions a header can write saturate rather than wrap.
    assert!(matches!(
        Image::decode(&png_declaring(u32::MAX >> 1, u32::MAX >> 1)),
        Err(ImageDecodeError::TooLarge { .. } | ImageDecodeError::Malformed(_))
    ));
}

/// An RGBA8 header that declares exactly [`MAX_DECODED_IMAGE_BYTES`] is
/// within the limit: its decoder asks for one block of that size, and the
/// property takes it. The fuzzer reaches this header in a few bytes, so a
/// bound off by one at the limit would fail every night on a legal image.
pub(crate) fn a_header_at_the_limit_asks_for_one_block_of_the_limit() {
    let width = 8192_u32;
    let height = u32::try_from(MAX_DECODED_IMAGE_BYTES / (4 * u64::from(width)))
        .expect("BUG: the limit's rows fit u32");
    assert_eq!(
        u64::from(width) * u64::from(height) * 4,
        MAX_DECODED_IMAGE_BYTES
    );
    decodes_within_the_limit_or_fails_typed(&png_declaring(width, height));
}

/// Arbitrary bytes, and the fixture with arbitrary bytes overwritten, decode
/// to an image within the limit or to a typed error; nothing panics. The fuzz
/// target `fuzz/image_decode.rs` holds the same property under libFuzzer.
pub(crate) fn arbitrary_bytes_decode_or_fail_typed() {
    proptest!(|(bytes in proptest::collection::vec(any::<u8>(), 0..512))| {
        decodes_within_the_limit_or_fails_typed(&bytes);
    });
    proptest!(|(
        edits in proptest::collection::vec((0..TINY_PNG.len(), any::<u8>()), 1..8),
        cut in 0..=TINY_PNG.len(),
    )| {
        let mut png = TINY_PNG.to_vec();
        for (at, byte) in edits {
            png[at] = byte;
        }
        png.truncate(cut);
        decodes_within_the_limit_or_fails_typed(&png);
    });
}

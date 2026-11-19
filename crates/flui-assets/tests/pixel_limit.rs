//! An image whose header declares more pixels than the decoder's cap is
//! refused by size before the decoder allocates them.

use flui_assets::{ImageCache, ImageLoadErrorKind, ImageSource, LoadServices};
use flui_painting::paint::ImageDecodeError;

use crate::lanes::block_on;

/// The committed fixture with its PNG header rewritten to declare
/// `width` x `height`, its checksum recomputed. The pixel data stays 4x2.
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

    let mut png = include_bytes!("fixtures/tiny.png").to_vec();
    // Signature (8), IHDR length (4), "IHDR" (4), then width and height.
    assert_eq!(
        &png[12..16],
        b"IHDR",
        "the fixture starts with its header chunk"
    );
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}

/// An image whose header declares more RGBA8 pixels than the decoded-image
/// cap is refused by size before the decoder allocates them.
#[test]
fn decoded_pixels_are_bounded_before_decoding() {
    let huge = png_declaring(9_000, 9_000);
    let cache = ImageCache::default();
    let directory = std::env::temp_dir().join(format!(
        "flui-assets-huge-pixels-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock is after the epoch")
            .as_nanos()
    ));
    std::fs::create_dir(&directory).expect("test directory is created");
    let path = directory.join("huge.png");
    std::fs::write(&path, &huge).expect("the oversized header is written");
    let decoded = block_on(cache.load(
        &ImageSource::asset(path.to_string_lossy().into_owned()),
        &LoadServices::new(),
    ));
    let _ = std::fs::remove_dir_all(&directory);
    assert!(
        matches!(&decoded, Err(error)
            if matches!(error.kind(), ImageLoadErrorKind::Decode(ImageDecodeError::TooLarge { .. }))),
        "the load is refused by size, got {decoded:?}"
    );
}

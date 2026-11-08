//! End-to-end image decoding through the application-owned registry: a real
//! committed PNG reaches its real dimensions, and malformed or oversized
//! sources fail without poisoning a later load.
#![cfg(feature = "images")]

use flui_assets::{AssetRegistryBuilder, LoadServices};

/// Absolute path to the committed 4x2 RGBA fixture PNG.
fn fixture_path() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tiny.png")
}

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

#[tokio::test]
async fn image_file_loads_a_committed_png_to_its_real_dimensions_and_recovers() {
    let registry = AssetRegistryBuilder::new().build();
    let image = registry
        .load_image(fixture_path(), LoadServices::new())
        .await
        .expect("a real, well-formed PNG fixture must decode successfully");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert_eq!(image.data().len(), 4 * 2 * 4);

    let missing = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/does-not-exist.png"
    );
    assert!(
        registry
            .load_image(missing, LoadServices::new())
            .await
            .is_err()
    );
    let recovered = registry
        .load_image(fixture_path(), LoadServices::new())
        .await
        .expect("a missing file does not prevent a later successful load");
    assert_eq!(recovered.data(), image.data());
}

/// An image whose header declares more RGBA8 pixels than the decoded-image cap
/// is refused by size before the decoder allocates them.
#[tokio::test]
async fn decoded_pixels_are_bounded_before_decoding() {
    let huge = png_declaring(9_000, 9_000);
    let registry = AssetRegistryBuilder::new().build();
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
    let decoded = registry
        .load_image(path.to_string_lossy().into_owned(), LoadServices::new())
        .await;
    let _ = std::fs::remove_dir_all(&directory);
    assert!(
        matches!(decoded, Err(flui_assets::AssetError::LoadFailed { ref reason, .. })
            if reason.contains("larger than")),
        "the decoded-image load is refused, got {decoded:?}"
    );
}

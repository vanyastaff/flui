//! End-to-end verification that `flui-assets`' own image pipeline works: a
//! real, committed PNG fixture flows through `ImageAsset::file` →
//! `AssetRegistry::load` → decode → cache, and the decoded dimensions are
//! real (not a placeholder).
//!
//! This is `flui-assets`' half of the Business.1 roadmap item ("confirm ...
//! asset image ... loading"). It does **not** prove anything about the
//! `Image` *widget*, whose asset and network providers have their own consumer tests.
#![cfg(feature = "images")]

use flui_assets::{AssetKey, AssetRegistryBuilder, ImageAsset};

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
/// cap is refused by size before the decoder allocates them, on both the
/// typed and the decoded-image path.
#[tokio::test]
async fn decoded_pixels_are_bounded_before_decoding() {
    let huge = png_declaring(9_000, 9_000);
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    fn refused_by_size<T>(result: &Result<T, flui_assets::AssetError>) -> bool {
        matches!(result, Err(flui_assets::AssetError::LoadFailed { reason, .. })
            if reason.contains("larger than"))
    }

    let typed = registry
        .load(ImageAsset::from_bytes("huge.png", huge.clone()))
        .await;
    assert!(
        refused_by_size(&typed),
        "the typed load is refused, got {typed:?}"
    );

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
        .load_image(path.to_string_lossy().into_owned(), None)
        .await;
    let _ = std::fs::remove_dir_all(&directory);
    assert!(
        refused_by_size(&decoded),
        "the decoded-image load is refused, got {decoded:?}"
    );
}

#[tokio::test]
async fn image_asset_file_loads_a_committed_png_fixture_to_its_real_dimensions() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(flui_assets::CacheCapacity::Entries(
            std::num::NonZeroU64::new(102).expect("nonzero test capacity"),
        ))
        .build();

    let handle = registry
        .load(ImageAsset::file(fixture_path()))
        .await
        .expect("a real, well-formed PNG fixture must decode successfully");

    assert_eq!(
        (handle.width(), handle.height()),
        (4, 2),
        "the decoded image must keep the fixture's true 4x2 dimensions, not a 0x0 placeholder",
    );
    assert_eq!(
        handle.data().len(),
        4 * 2 * 4,
        "decoded pixel buffer must be RGBA8 (4 bytes/pixel) at the fixture's dimensions",
    );

    let embedded_key = "embedded-fixture";
    assert!(
        registry
            .load(ImageAsset::from_bytes(
                embedded_key,
                b"invalid image".to_vec()
            ))
            .await
            .is_err()
    );
    assert!(
        registry
            .get::<ImageAsset>(&AssetKey::new(embedded_key))
            .await
            .is_none()
    );
    let embedded = registry
        .load(ImageAsset::from_bytes(
            embedded_key,
            include_bytes!("fixtures/tiny.png").to_vec(),
        ))
        .await
        .expect("valid embedded bytes recover after a failed decode");
    assert_eq!((embedded.width(), embedded.height()), (4, 2));
    assert_eq!(
        embedded.data(),
        handle.data(),
        "file and embedded sources decode identical pixels"
    );

    let missing = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/does-not-exist.png"
    );
    assert!(registry.load(ImageAsset::file(missing)).await.is_err());
    let recovered = registry
        .load(ImageAsset::file(fixture_path()))
        .await
        .expect("a missing file does not prevent a later successful load");
    assert_eq!(recovered.data(), handle.data());
}

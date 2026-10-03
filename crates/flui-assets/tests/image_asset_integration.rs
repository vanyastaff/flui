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

#[tokio::test]
async fn image_asset_file_loads_a_committed_png_fixture_to_its_real_dimensions() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(1024 * 1024)
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

//! End-to-end verification that `flui-assets`' own image pipeline works: a
//! real, committed PNG fixture flows through `ImageAsset::file` →
//! `AssetRegistry::load` → decode → cache, and the decoded dimensions are
//! real (not a placeholder).
//!
//! This is `flui-assets`' half of the Business.1 roadmap item ("confirm ...
//! asset image ... loading"). It does **not** prove anything about the
//! `Image` *widget* — `flui-widgets` has no dependency on `flui-assets` (see
//! `crates/flui-widgets/tests/image.rs`'s
//! `image_file_provider_decodes_a_committed_png_fixture_to_its_real_dimensions`
//! for the widget's own, independent decode path). `docs/ROADMAP.md`'s
//! Business.1 entry records that gap explicitly.
#![cfg(feature = "images")]

use flui_assets::{AssetRegistryBuilder, ImageAsset};

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
}

#[tokio::test]
async fn image_asset_file_is_present_in_cache_after_load() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(1024 * 1024)
        .build();

    let loaded = registry
        .load(ImageAsset::file(fixture_path()))
        .await
        .expect("load decodes the fixture from disk");

    let cached = registry
        .get::<ImageAsset>(loaded.key())
        .await
        .expect("the asset must be present in the cache under its own key after load");

    assert_eq!(
        (cached.width(), cached.height()),
        (loaded.width(), loaded.height()),
        "the cached handle must carry the same decoded dimensions as the loaded one",
    );
}

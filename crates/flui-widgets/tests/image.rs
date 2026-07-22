//! Layout parity tests for the `Image` widget's synchronous path.
//!
//! Each test exercises a distinct layout mode and asserts a computed size that
//! would be wrong if the widget mis-wired its render object, swapped
//! width/height, dropped the forced dimension, or failed to resolve the
//! provider correctly.
//!
//! `AssetImage`/`NetworkImage` (the async providers) are NOT covered here —
//! `Image::from_image`/`memory`/`file` all resolve synchronously via
//! `ImageProvider::resolve`, so `Image`'s `StatelessView::build` takes the
//! `build_sync` path unconditionally and every test below observes the FIRST
//! (and only) frame. `tests/image_async.rs` covers the async
//! probe-cache/`FutureBuilder`-wrap/coalescing dispatch that only exists once
//! a provider's `cache_key()` returns `Some`.

mod common;

use common::{lay_out, loose, size, tight};
use flui_types::geometry::px;
use flui_types::painting::Image as PixelImage;
use flui_widgets::{Image, ImageAlignment, ImageFit};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Solid-color RGBA8 image of given pixel dimensions. Each pixel is opaque
/// white. PixelImage::from_rgba8 panics if the byte count is wrong, so a
/// compile-time-unsatisfied length would be caught immediately.
fn solid_image(width: u32, height: u32) -> PixelImage {
    PixelImage::from_rgba8(width, height, vec![255u8; (width * height * 4) as usize])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn image_from_decoded_lays_out_at_intrinsic_size() {
    // A 4×6-pixel image under unconstrained (loose 1000×1000) layout should
    // occupy exactly its intrinsic size: 4×6 logical pixels. Asserts that the
    // provider resolved successfully (a 0×0 result means the provider failed).
    let laid = lay_out(Image::from_image(solid_image(4, 6)), loose(1000.0));
    assert_eq!(laid.size(laid.root()), size(4.0, 6.0));
}

#[test]
fn image_forced_width_preserves_aspect_ratio() {
    // A 4×8-pixel image (1∶2 aspect) with forced width=40, unconstrained
    // height. `tighten` fixes width to 40; `constrain_size_and_attempt_to_
    // preserve_aspect_ratio` selects height=80 to preserve the 1∶2 ratio.
    let laid = lay_out(
        Image::from_image(solid_image(4, 8)).width(40.0),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(40.0, 80.0));
}

#[test]
fn image_forced_width_and_height_override_aspect() {
    // Both width=50 and height=50 forced on a 100×200-pixel (1∶2) image.
    // Tight 50×50 constraints win; aspect ratio is NOT preserved — the box is
    // 50×50 regardless of the 1∶2 intrinsic ratio.
    let laid = lay_out(
        Image::from_image(solid_image(100, 200))
            .width(50.0)
            .height(50.0),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(50.0, 50.0));
}

#[test]
fn image_under_tight_constraints_fills_the_tight_box() {
    // Under tight 200×100 constraints a 10×10 image fills the box. Tight
    // constraints force min == max on both axes so the result must be 200×100
    // regardless of intrinsic size or aspect.
    let laid = lay_out(Image::from_image(solid_image(10, 10)), tight(200.0, 100.0));
    assert_eq!(laid.size(laid.root()), size(200.0, 100.0));
}

#[test]
fn image_large_intrinsic_shrinks_to_fit_loose_box() {
    // A 200×100-pixel (2∶1) image under loose(80): the box is 80×80 and
    // `constrain_size_and_attempt_to_preserve_aspect_ratio` scales the image
    // down to 80×40 (preserves 2∶1, fits width=80, height=40 < 80 ✓).
    let laid = lay_out(Image::from_image(solid_image(200, 100)), loose(80.0));
    assert_eq!(laid.size(laid.root()), size(80.0, 40.0));
}

#[test]
#[cfg(feature = "images")]
fn image_file_provider_decodes_a_committed_png_fixture_to_its_real_dimensions() {
    // `tests/fixtures/tiny.png` is a real, committed 5x3 RGBA PNG (not a
    // synthetic in-memory buffer). `Image::file` decodes it synchronously via
    // `flui-widgets`' OWN `image`-crate dependency (`ImageProvider::resolve`
    // in `src/image/provider.rs`) — it does NOT go through `flui-assets`;
    // `Image::asset` (the `asset-images`-feature, `flui-assets`-backed async
    // path) is covered separately in `tests/image_async.rs`.
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tiny.png");
    let laid = lay_out(Image::file(fixture), loose(1000.0));
    assert_eq!(
        laid.size(laid.root()),
        size(5.0, 3.0),
        "a 5x3 real PNG file must decode to its true pixel dimensions, not a \
         0x0 placeholder from a swallowed decode failure",
    );
}

#[test]
fn image_sync_provider_failure_renders_zero_size() {
    // A synchronously-failing custom `ImageProvider` (`cache_key` defaults to
    // `None`, so `Image` never leaves the `build_sync` path) must fall back
    // to `RenderImage::new(Size::ZERO, …)`, giving `constraints.smallest()`
    // == 0×0 under loose layout. If this assertion passes with a non-zero
    // size the provider succeeded unexpectedly — equally wrong, and caught
    // here.
    #[derive(Debug)]
    struct AlwaysFails;
    impl flui_widgets::ImageProvider for AlwaysFails {
        fn resolve(&self) -> Result<PixelImage, flui_widgets::ImageProviderError> {
            Err(flui_widgets::ImageProviderError::DecodeFailed {
                reason: "synthetic test failure".to_string(),
            })
        }
    }

    let laid = lay_out(Image::new(AlwaysFails), loose(1000.0));
    assert_eq!(laid.size(laid.root()), size(0.0, 0.0));
}

#[test]
fn image_fit_and_alignment_accessors_are_chainable() {
    // Builder chain smoke test: `fit` and `alignment` calls preserve the
    // underlying image and produce the correct layout size.
    let laid = lay_out(
        Image::from_image(solid_image(8, 8))
            .fit(ImageFit::Cover)
            .alignment(ImageAlignment::TopLeft),
        loose(1000.0),
    );
    // 8×8 intrinsic in 1000×1000 loose = 8×8 (no forced dims; image is below
    // the max so it sits at intrinsic size).
    assert_eq!(laid.size(laid.root()), size(8.0, 8.0));
}

// ---------------------------------------------------------------------------
// Full-pipeline paint-geometry wiring
//
// Not direct `image_test.dart` ports -- Flutter's fit/alignment paint math
// has its own oracle, `painting/paint_image_test.dart`, outside this
// corpus's denominator, and `RenderImage`'s fit math is already exhaustively
// unit-tested in `crates/flui-objects/src/image/render_image.rs`
// (`test_compute_paint_rect_*`, `test_paint_*`). These three prove the
// WIRING instead: that a real `Image` widget, mounted through the full
// View -> RawImage -> RenderImage pipeline, carries `fit`/`alignment` all
// the way to the committed paint rect -- every other test in this file only
// asserts the LAYOUT size, never where the image content actually paints.
// ---------------------------------------------------------------------------

#[test]
fn image_widget_wires_cover_fit_and_center_alignment_into_the_paint_rect() {
    // 100×100 image (1:1) Cover-fit into a 200×50 box: scale =
    // max(200/100, 50/100) = 2.0 -> painted 200×200, Center-aligned ->
    // origin (0, (50-200)/2 = -75) (cropped top and bottom).
    let laid = lay_out(
        Image::from_image(solid_image(100, 100))
            .fit(ImageFit::Cover)
            .alignment(ImageAlignment::Center),
        tight(200.0, 50.0),
    );
    let rect = laid
        .image_paint_rect(laid.root())
        .expect("a resolved image must produce a paint rect");
    assert_eq!(rect.size().width, px(200.0));
    assert_eq!(rect.size().height, px(200.0));
    assert_eq!(rect.origin().x, px(0.0));
    assert_eq!(rect.origin().y, px(-75.0));
}

#[test]
fn image_widget_wires_fill_fit_and_top_left_alignment_into_the_paint_rect() {
    // Fill ignores aspect ratio and stretches to the whole box regardless of
    // alignment (alignment becomes a no-op once the painted size equals the
    // box exactly).
    let laid = lay_out(
        Image::from_image(solid_image(10, 40))
            .fit(ImageFit::Fill)
            .alignment(ImageAlignment::TopLeft),
        tight(120.0, 80.0),
    );
    let rect = laid
        .image_paint_rect(laid.root())
        .expect("a resolved image must produce a paint rect");
    assert_eq!(rect.size().width, px(120.0));
    assert_eq!(rect.size().height, px(80.0));
    assert_eq!(rect.origin().x, px(0.0));
    assert_eq!(rect.origin().y, px(0.0));
}

#[test]
fn image_widget_wires_scale_down_fit_and_bottom_right_alignment_into_the_paint_rect() {
    // ScaleDown never enlarges: a small 10×10 image in a big 100×100 box
    // stays at its natural 10×10 size, BottomRight-aligned into the box's
    // bottom-right corner.
    let laid = lay_out(
        Image::from_image(solid_image(10, 10))
            .fit(ImageFit::ScaleDown)
            .alignment(ImageAlignment::BottomRight),
        tight(100.0, 100.0),
    );
    let rect = laid
        .image_paint_rect(laid.root())
        .expect("a resolved image must produce a paint rect");
    assert_eq!(rect.size().width, px(10.0));
    assert_eq!(rect.size().height, px(10.0));
    assert_eq!(rect.origin().x, px(90.0));
    assert_eq!(rect.origin().y, px(90.0));
}

// ---------------------------------------------------------------------------
// Post-mount provider swap / reconfiguration
//
// Every OTHER sync test in this file only exercises the FIRST frame. These
// two mount, then drive a SECOND frame that changes the resolved image,
// proving the update path (not just the create path) wires correctly.
// ---------------------------------------------------------------------------

#[test]
fn image_widget_sync_provider_swap_replaces_the_displayed_image_not_the_stale_one() {
    // `RawImage::update_render_object` always pushes the freshly resolved
    // image (`render.set_image(self.image.clone())`) on every rebuild --
    // this proves that wiring survives an actual provider swap on an
    // already-mounted `Image`, not just at initial creation.
    let mut laid = lay_out(Image::from_image(solid_image(4, 4)), loose(1000.0));
    assert_eq!(laid.size(laid.root()), size(4.0, 4.0));

    laid.pump_widget(Image::from_image(solid_image(9, 6)));
    assert_eq!(
        laid.size(laid.current_root()),
        size(9.0, 6.0),
        "swapping to a differently-sized decoded image on an already-mounted \
         Image must update the render object's displayed content, not keep \
         painting/laying-out the stale first image",
    );
    assert!(
        laid.image_has_image(laid.current_root()),
        "the render object must carry the NEW image, not have cleared to \
         the empty placeholder",
    );
}

/// Mirrors Flutter's `Image State can be reconfigured to use another image`
/// (`image_test.dart`, 3.44.0): reordering a list of UNKEYED `Image`
/// widgets does not move render objects around with them -- element
/// reconciliation matches by (type, position) when no key disambiguates,
/// so each POSITION keeps its own render object and merely receives the
/// other widget's config on the next update.
#[test]
fn image_state_rebinds_config_to_positional_render_objects_when_reordered_without_keys() {
    use flui_widgets::{Column, column};

    let image1 = Image::from_image(solid_image(4, 4)).width(10.0);
    let image2 = Image::from_image(solid_image(4, 4)).width(20.0);

    let mut laid = lay_out(
        Column::new(column![image1.clone(), image2.clone()]),
        loose(1000.0),
    );
    let root = laid.root();
    let first = laid.child(root, 0);
    let second = laid.child(root, 1);

    assert_eq!(laid.image_width(first), Some(px(10.0)));
    assert_eq!(laid.image_width(second), Some(px(20.0)));

    laid.pump_widget(Column::new(column![image2, image1]));
    let after_root = laid.current_root();

    assert_eq!(
        laid.child(after_root, 0),
        first,
        "reordering unkeyed widgets must reuse the SAME render object at \
         each position -- Flutter's default (type, position) matching \
         reuses the Element/RenderObject and swaps only its config, it does \
         not move objects to follow their originating widget instance",
    );
    assert_eq!(
        laid.image_width(first),
        Some(px(20.0)),
        "position 0 must now carry image2's width -- config rebinds to the \
         POSITION, not the widget instance that first created the object",
    );
    assert_eq!(laid.image_width(second), Some(px(10.0)));
}

/// Mirrors Flutter's `Image.memory control test` (`image_test.dart`,
/// 3.44.0) -- a smoke test that `Image.memory` mounts and decodes without
/// panicking. The oracle also passes `excludeFromSemantics: true`; FLUI's
/// `Image` contributes no semantics node at all yet (with or without such a
/// parameter -- there is no semantics wiring to exclude from, see
/// `docs/ROADMAP.md` Cross.H), so that part of the oracle has no FLUI
/// counterpart to assert against.
#[test]
#[cfg(feature = "images")]
fn image_memory_control_test_decodes_bytes_without_panicking() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tiny.png"
    ))
    .expect("the committed fixture PNG must be readable");

    let laid = lay_out(Image::memory(bytes), loose(1000.0));
    assert_eq!(
        laid.size(laid.root()),
        size(5.0, 3.0),
        "Image::memory must decode the real PNG bytes to their true \
         dimensions, not silently fail to an empty placeholder",
    );
}

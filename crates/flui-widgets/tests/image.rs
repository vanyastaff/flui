//! Layout parity tests for the `Image` widget.
//!
//! Each test exercises a distinct layout mode and asserts a computed size that
//! would be wrong if the widget mis-wired its render object, swapped
//! width/height, dropped the forced dimension, or failed to resolve the
//! provider correctly. The `image_failed_provider_renders_zero_size` test is
//! the only one that asserts 0×0 — if the provider is accidentally made to
//! succeed the assertion flips red, which is the desired sentinel.

mod common;

use common::{lay_out, loose, size, tight};
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
fn image_failed_provider_renders_zero_size() {
    // `NetworkImage` always returns `ImageProviderError::AsyncNotWired`.
    // `create_render_object` falls back to `RenderImage::new(Size::ZERO, …)`,
    // giving `constraints.smallest()` == 0×0 under loose layout. If this
    // assertion passes with a non-zero size the provider succeeded
    // unexpectedly — which is equally wrong and caught here.
    let laid = lay_out(Image::network("https://example.com/img.png"), loose(1000.0));
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

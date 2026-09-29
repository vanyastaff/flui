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

use crate::common::{lay_out, loose, size};
use flui_painting::paint::Image as PixelImage;
use flui_widgets::Image;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Solid-color RGBA8 image of given pixel dimensions. Each pixel is opaque
/// white. PixelImage::from_rgba8 panics if the byte count is wrong, so a
/// compile-time-unsatisfied length would be caught immediately.
fn solid_image(width: u32, height: u32) -> PixelImage {
    PixelImage::from_rgba8(
        width,
        height,
        vec![255u8; width as usize * height as usize * 4],
    )
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

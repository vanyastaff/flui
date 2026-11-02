//! Layout tests for the `Image` widget's synchronous path.
//!
//! Each test exercises a distinct layout mode and asserts a computed size that
//! would be wrong if the widget mis-wired its render object, swapped
//! width/height, dropped the forced dimension, or failed to resolve the
//! provider correctly.
//!
//! Every test below uses `Image::from_image`, whose provider has no cache key
//! and resolves inline in `build`, so it observes the first (and only) frame.
//! `tests/image_background.rs` covers `Image::memory`/`file`, which decode on
//! the background lanes, and `tests/image_async.rs` the `AssetImage` loads.

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

fn image_from_decoded_lays_out_at_intrinsic_size() {
    // A 4×6-pixel image under unconstrained (loose 1000×1000) layout should
    // occupy exactly its intrinsic size: 4×6 logical pixels. Asserts that the
    // provider resolved successfully (a 0×0 result means the provider failed).
    let laid = lay_out(Image::from_image(solid_image(4, 6)), loose(1000.0));
    assert_eq!(laid.size(laid.root()), size(4.0, 6.0));
}

/// The placement builders reach the painted scene through the mounted
/// `RenderImage`, on create and on update; the last of a slice and a fitted
/// builder wins.
fn image_placement_builders_reach_the_painted_scene() {
    use crate::common::tight;
    use flui_foundation::geometry::Rect;
    use flui_painting::DrawOp;
    use flui_painting::{Alignment, BoxFit};
    use flui_widgets::ImageRepeat;

    let regions = |laid: &crate::common::LaidOut| {
        laid.draw_ops()
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::ImageRegion { dst, repeat, .. } => Some((dst, repeat)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let center = Rect::from_ltrb(1.0, 1.0, 3.0, 3.0);
    let mut laid = lay_out(
        Image::from_image(solid_image(4, 4))
            .repeat(ImageRepeat::Repeat)
            .center_slice(center),
        tight(20.0, 10.0),
    );
    assert_eq!(regions(&laid).len(), 9, "a slice paints its nine regions");

    laid.pump_widget(
        Image::from_image(solid_image(4, 4))
            .center_slice(center)
            .fit(BoxFit::Contain)
            .alignment(Alignment::TOP_LEFT)
            .repeat(ImageRepeat::RepeatX),
    );
    assert_eq!(
        regions(&laid),
        [(Rect::from_ltrb(0.0, 0.0, 20.0, 10.0), ImageRepeat::RepeatX)],
        "a later fitted builder replaces the slice and repeats across"
    );
}

// ---------------------------------------------------------------------------
// Post-mount provider swap / reconfiguration
//
// Every OTHER sync test in this file only exercises the FIRST frame. These
// two mount, then drive a SECOND frame that changes the resolved image,
// proving the update path (not just the create path) wires correctly.
// ---------------------------------------------------------------------------

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

/// The `Image` widget: an eagerly decoded image lays out at its intrinsic size, and a
/// synchronous provider swap replaces the displayed image rather than keeping the stale one.
#[test]
fn image_widget_decoded_size_and_provider_swap() {
    image_from_decoded_lays_out_at_intrinsic_size();
    image_widget_sync_provider_swap_replaces_the_displayed_image_not_the_stale_one();
    image_placement_builders_reach_the_painted_scene();
}

//! Readback evidence that a clip layer clips, and that its ABSENCE does not.
//!
//! This is the engine half of the viewport's `clip_behavior` contract. The
//! other half lives at the widget layer
//! (`flui_widgets`'s `viewport_clip_behavior_controls_the_clip_layer`), which
//! pins that `Clip::None` pushes NO clip layer while an overflowing viewport
//! under any other behaviour pushes one. Neither half is evidence on its own:
//! the widget test reads layer kinds and never a pixel, and this one knows
//! nothing about viewports. Together they say what a user sees.
//!
//! **What this cannot show.** The three clipped modes — `HardEdge`,
//! `AntiAlias`, `AntiAliasWithSaveLayer` — are pixel-identical here, because
//! the wgpu backend takes a clip layer's `Clip` value and discards it
//! (`push_clip_rect`'s `_clip_behavior` in `backend.rs`). That gap is tracked
//! rather than papered over with a test that would pass either way.

use flui_layer::{LayerTree, SceneBuilder};
use flui_painting::{Canvas, Paint};
use flui_types::{Color, Rect, geometry::px, painting::Clip};

use super::headless::HeadlessRenderer;

const SIDE: u32 = 64;
/// The clip keeps the top half; content fills the whole surface.
const CLIP_BOTTOM: f32 = 32.0;
/// Sampled well inside the clipped-away half, and clear of its boundary so
/// no anti-aliased edge can decide the result either way.
const SAMPLE_Y: u32 = 48;
const SAMPLE_X: u32 = 32;

/// A blue rect covering the whole surface.
fn full_surface_content() -> flui_painting::DisplayList {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
        &Paint::fill(Color::rgb(0, 0, 255)),
    );
    canvas.finish()
}

fn sample(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * SIDE + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

/// Renders `tree` and returns the pixel at the sample point.
fn render_and_sample(renderer: &HeadlessRenderer, tree: &LayerTree) -> [u8; 4] {
    let pixels = renderer
        .render_layer_tree(tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize a two-layer tree");
    sample(&pixels, SAMPLE_X, SAMPLE_Y)
}

/// Content that overflows a clip layer is not painted past it; the same
/// content with no clip layer is. The surface is cleared to opaque white, so
/// "clipped away" reads as white and "painted" reads as blue.
#[test]
fn a_clip_rect_layer_clips_its_content_and_its_absence_does_not() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        // No adapter on this machine. CI's gpu-test job runs on WARP, where
        // this always resolves.
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let mut clipped_tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut clipped_tree);
        builder.push_clip_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(CLIP_BOTTOM)),
            Clip::HardEdge,
        );
        builder.add_picture(full_surface_content());
        builder.build();
    }

    let mut unclipped_tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut unclipped_tree);
        builder.add_picture(full_surface_content());
        builder.build();
    }

    let clipped = render_and_sample(&renderer, &clipped_tree);
    let unclipped = render_and_sample(&renderer, &unclipped_tree);

    // The RED channel is what discriminates here: the content is blue
    // (0, 0, 255) and the cleared surface is white (255, 255, 255), so they
    // agree on blue and disagree only on red. Asserting on blue would pass
    // against both and prove nothing.
    assert_eq!(
        unclipped[0], 0,
        "without a clip layer the blue content covers the sample point (got {unclipped:?})",
    );
    assert_eq!(
        clipped[0], 255,
        "the clip layer must keep the content off a sample point 16 px past \
         its edge, leaving the cleared white (got {clipped:?})",
    );
}

/// An aliased paint produces a hard edge on the GPU, and the default does not.
///
/// The knob is only worth having if the pixels change. `Paint::anti_alias`
/// existed as metadata for a long time before anything read it — the batcher
/// never looked, and the shader always applied `sdfToAlpha` — so a rect drawn
/// with `anti_alias: false` rasterized identically to one without. This is the
/// oracle that would have caught that.
///
/// The sample point is a ROTATED edge on purpose. An axis-aligned edge that
/// lands exactly on a pixel boundary has no partial coverage to smooth, so it
/// looks the same either way and the test would pass against both.
#[test]
fn an_aliased_paint_hardens_the_edge_the_default_smooths() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    // A square rotated 30 degrees about the surface centre: its edges cross
    // pixel centres at an angle, so an anti-aliased edge produces a band of
    // partial coverage that a hard one cannot.
    let content = |anti_alias: bool| {
        let mut canvas = Canvas::new();
        canvas.save();
        canvas.translate(SIDE as f32 / 2.0, SIDE as f32 / 2.0);
        canvas.rotate(std::f32::consts::FRAC_PI_6);
        canvas.draw_rect(
            Rect::from_xywh(px(-20.0), px(-20.0), px(40.0), px(40.0)),
            &Paint::fill(Color::rgb(0, 0, 255)).with_anti_alias(anti_alias),
        );
        canvas.restore();
        canvas.finish()
    };

    let render = |anti_alias: bool| {
        let mut tree = LayerTree::new();
        {
            let mut builder = SceneBuilder::new(&mut tree);
            builder.add_picture(content(anti_alias));
            builder.build();
        }
        renderer
            .render_layer_tree(&tree, (SIDE, SIDE))
            .expect("the headless capture path must rasterize a one-layer tree")
    };

    // Count pixels that are neither the white ground nor the solid blue fill.
    // Only a smoothed edge produces them.
    let partial = |pixels: &[u8]| {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| {
                let solid_blue = px[0] < 8 && px[1] < 8 && px[2] > 248;
                let ground = px[0] > 248 && px[1] > 248 && px[2] > 248;
                !solid_blue && !ground
            })
            .count()
    };

    let smoothed = partial(&render(true));
    let hard = partial(&render(false));

    assert!(
        smoothed > 0,
        "the default must smooth a rotated edge; found no partial-coverage pixels",
    );
    assert_eq!(
        hard, 0,
        "an aliased paint must leave every pixel fully in or fully out; \
         found {hard} partial-coverage pixels",
    );
}

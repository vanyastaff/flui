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
//! **The modes.** `HardEdge` and `AntiAlias` now render differently, which
//! `a_hard_clip_edge_and_a_smooth_one_differ` pins: the backend used to take a
//! clip layer's `Clip` and discard it, so all three clipped modes were one
//! picture. `AntiAliasWithSaveLayer` still renders as `AntiAlias` — its
//! offscreen composite is not implemented — and that is asserted deliberately
//! and narrowly, so the day it lands, the assertion fails in the right place
//! rather than the divergence going quiet.

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

// ---------------------------------------------------------------------------
// Per-mode edges (#848)
// ---------------------------------------------------------------------------

/// Counts pixels that are neither the white ground nor the solid blue fill.
///
/// Only a feathered boundary produces them. This is the same discriminator
/// `an_aliased_paint_hardens_the_edge_the_default_smooths` uses for a paint's
/// own edge, applied here to the *clip's* edge instead.
fn partial_coverage_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| {
            let white = p[0] > 250 && p[1] > 250 && p[2] > 250;
            let blue = p[0] < 5 && p[1] < 5 && p[2] > 250;
            !white && !blue
        })
        .count()
}

/// A clip whose boundary crosses pixel centres at an angle, so the two modes
/// cannot agree: `HardEdge` has to pick whole pixels, `AntiAlias` feathers.
///
/// The content is drawn WITHOUT anti-aliasing and strictly inside the surface,
/// so the only edge in the frame is the clip's own. Otherwise the paint's edge
/// would contribute partial pixels to both modes and swamp the difference —
/// one of the ways a clip oracle goes green against both the fixed and the
/// broken code.
fn rotated_clip_scene(behavior: Clip) -> LayerTree {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        // Rotate about the surface centre so the clip's edges cross pixel
        // centres at an angle — the only geometry where the two modes must
        // visibly disagree.
        let centre = flui_types::Matrix4::translation(SIDE as f32 / 2.0, SIDE as f32 / 2.0, 0.0);
        builder
            .push_transform(centre * flui_types::Matrix4::rotation_z(std::f32::consts::FRAC_PI_6));
        builder.push_clip_rect(
            Rect::from_xywh(px(-18.0), px(-18.0), px(36.0), px(36.0)),
            behavior,
        );
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(-40.0), px(-40.0), px(80.0), px(80.0)),
            &Paint::fill(Color::rgb(0, 0, 255)).with_anti_alias(false),
        );
        builder.add_picture(canvas.finish());
        builder.build();
    }
    tree
}

/// `Clip::HardEdge` and `Clip::AntiAlias` must not render the same picture.
///
/// This is the defect #848 named: the backend took the layer's `Clip` and
/// discarded it, so all three clipped modes were one picture. A test asserting
/// the modes AGREE would have pinned that defect as the contract, which is why
/// none existed before the modes were honoured.
#[test]
fn a_hard_clip_edge_and_a_smooth_one_differ() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let render = |behavior: Clip| {
        renderer
            .render_layer_tree(&rotated_clip_scene(behavior), (SIDE, SIDE))
            .expect("the headless capture path must rasterize a clipped tree")
    };

    let hard = partial_coverage_pixels(&render(Clip::HardEdge));
    let smooth = partial_coverage_pixels(&render(Clip::AntiAlias));

    assert!(
        smooth > hard,
        "an anti-aliased clip must feather its boundary: hard={hard} smooth={smooth}"
    );
    assert!(
        hard * 4 < smooth,
        "the difference must be a real band, not a few stray pixels: \
         hard={hard} smooth={smooth}"
    );
}

/// `AntiAliasWithSaveLayer` currently renders as `AntiAlias`, which is WRONG.
///
/// This asserts a known-wrong equality, which is normally how a defect gets
/// frozen as a contract. It is here deliberately and narrowly: the mode's
/// offscreen half is unimplemented, issue #848 stays open for it, and this is
/// what fails — loudly, in the right place — when it lands. Without the test
/// the divergence goes quiet; with it, the next person to implement the mode
/// is told exactly where to look.
///
/// What is actually wrong: Flutter composites the clipped subtree once, as a
/// group, against the clip edge. Applying coverage per draw makes the edge
/// darker or more opaque wherever the content overlaps itself or blends
/// non-trivially. A scene without such overlap is unaffected, which is why
/// this is a real defect and not a visible one in the common case.
#[test]
fn anti_alias_with_save_layer_currently_matches_plain_anti_alias() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let render = |behavior: Clip| {
        renderer
            .render_layer_tree(&rotated_clip_scene(behavior), (SIDE, SIDE))
            .expect("the headless capture path must rasterize a clipped tree")
    };

    assert_eq!(
        render(Clip::AntiAliasWithSaveLayer),
        render(Clip::AntiAlias),
        "the save-layer mode is approximated by plain anti-alias until its \
         offscreen composite lands (#848); when that changes, this test is the \
         one to update"
    );
}

/// A ROUNDED clip: hard thresholds its corners, smooth feathers them.
///
/// This is the case the rect test above cannot reach. A hard *rect* clip is
/// the hardware scissor, so it exercises no shader code at all; only a rounded
/// clip goes through `clipAlpha`'s hard branch and the `clip_kind.z` lane that
/// carries the mode there. The plumbing for that lane was in fact broken when
/// the rect test was the only oracle — `RectInstance::with_clip` rebuilt the
/// attribute as `[kind, aliased, 0, 0]` and dropped it — so a rounded
/// `HardEdge` clip still feathered while every test passed.
///
/// The clip is axis-aligned on purpose: its straight edges land on pixel
/// boundaries and contribute nothing either way, so the corners are the only
/// place the two modes can differ, and the count is about them.
fn rounded_clip_scene(behavior: Clip) -> LayerTree {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        builder.push_clip_rrect(
            flui_types::geometry::RRect::from_rect_circular(
                Rect::from_xywh(px(8.0), px(8.0), px(48.0), px(48.0)),
                px(16.0),
            ),
            behavior,
        );
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
            &Paint::fill(Color::rgb(0, 0, 255)).with_anti_alias(false),
        );
        builder.add_picture(canvas.finish());
        builder.build();
    }
    tree
}

#[test]
fn a_rounded_clip_honours_hard_edge_and_anti_alias_differently() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let render = |behavior: Clip| {
        renderer
            .render_layer_tree(&rounded_clip_scene(behavior), (SIDE, SIDE))
            .expect("the headless capture path must rasterize a rounded-clipped tree")
    };

    let hard = partial_coverage_pixels(&render(Clip::HardEdge));
    let smooth = partial_coverage_pixels(&render(Clip::AntiAlias));

    assert!(
        smooth > 0,
        "an anti-aliased rounded clip must feather its corners, got {smooth} partial pixels"
    );
    assert!(
        hard * 4 < smooth,
        "a hard rounded clip must threshold its corners rather than feather them: \
         hard={hard} smooth={smooth}"
    );
}

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
//! **The modes.** The backend used to take a clip layer's `Clip` and discard
//! it, so all three clipped modes were one picture. Now:
//!
//! - a **rounded** clip honours `HardEdge` vs `AntiAlias`
//!   (`a_rounded_clip_honours_hard_edge_and_anti_alias_differently`);
//! - a **rect** clip does not — it is the hardware scissor under both modes,
//!   because routing it to the SDF costs text clipping, nested intersection
//!   and exactness (`Painter::clip_rect` has the full reasoning);
//! - `AntiAliasWithSaveLayer` renders as `AntiAlias`; its offscreen group
//!   composite is unimplemented.
//!
//! The last two are pinned by tests asserting the known-wrong equalities —
//! deliberately, so each fails in the right place when it is fixed rather than
//! the divergence going quiet. Both are tracked on #848, which stays open.

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

/// A RECTANGULAR clip renders the same under both modes, deliberately.
///
/// `Clip::AntiAlias` on a rect is not honoured: a rect clip is the hardware
/// scissor, and routing it to the SDF to get a feathered edge was tried and
/// reverted. The SDF is a per-instance uniform and the scissor is not, so the
/// swap gave up three things — it stopped reaching text (handed to glyphon
/// with the scissor alone), stopped intersecting under nesting (one SDF slot,
/// inner overwrites outer), and stopped being exact (its coarse scissor is
/// padded, so pixels leak up to a pixel out). See `Painter::clip_rect`.
///
/// Asserting a known-wrong equality is normally how a defect gets frozen as a
/// contract, so this is deliberate and narrow, and paired with
/// `a_rounded_clip_honours_hard_edge_and_anti_alias_differently`, which proves
/// the mode IS honoured where it can be. When the shader grows a clip stack
/// and text routes through the same mask, this is the test that fails.
#[test]
fn a_rect_clip_renders_the_same_under_both_modes_for_now() {
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
        render(Clip::HardEdge),
        render(Clip::AntiAlias),
        "a rect clip is the scissor under both modes until the SDF can carry \
         one without losing text, nesting and exactness (#848)"
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

/// `Clip::None` through the CANVAS api applies no clip at all.
///
/// Reachable, despite the layer path never emitting one: `Canvas::clip_rect_ext`
/// and its siblings push their `DrawCommand` whatever mode they are given. The
/// backend used to map `None` alongside `HardEdge` — "the cheapest path" — which
/// clipped content the caller had explicitly asked to leave alone.
#[test]
fn a_canvas_clip_with_mode_none_does_not_clip() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let scene = |behavior: Clip| {
        let mut tree = LayerTree::new();
        {
            let mut builder = SceneBuilder::new(&mut tree);
            let mut canvas = Canvas::new();
            canvas.clip_rect_ext(
                Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(CLIP_BOTTOM)),
                flui_types::painting::ClipOp::Intersect,
                behavior,
            );
            canvas.draw_rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
                &Paint::fill(Color::rgb(0, 0, 255)),
            );
            builder.add_picture(canvas.finish());
            builder.build();
        }
        renderer
            .render_layer_tree(&tree, (SIDE, SIDE))
            .expect("the headless capture path must rasterize a canvas-clipped tree")
    };

    // Sampled below the clip's bottom edge: `HardEdge` clips it away, leaving
    // the white ground; `None` must leave it painted blue.
    //
    // The RED channel is the oracle, not blue: the content is blue and the
    // cleared surface is white, so the two AGREE on blue and an assertion
    // there passes either way. ADR-0054 records this about the sibling test;
    // I wrote blue first and it passed against both cases.
    let hard = sample(&scene(Clip::HardEdge), SAMPLE_X, SAMPLE_Y);
    let none = sample(&scene(Clip::None), SAMPLE_X, SAMPLE_Y);

    assert!(
        hard[0] > 200,
        "the control must actually clip — expected the white ground, got {hard:?}, \
         so the sample point proves nothing"
    );
    assert!(
        none[0] < 64,
        "Clip::None must not clip: content below the rect should still be painted \
         blue, got {none:?}"
    );
}

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
//!   and exactness (`Painter::clip_rect` has the full reasoning). That one is
//!   still pinned by a test asserting the known-wrong equality, deliberately,
//!   so it fails in the right place when the shader grows a clip stack;
//! - `AntiAliasWithSaveLayer` renders the clipped subtree into an offscreen and
//!   applies the clip's coverage ONCE, to the finished group
//!   (`the_save_layer_mode_composites_the_clipped_group_once`). The offscreen
//!   is declined in two cases, neither keyed on the clip's shape: where no clip
//!   was installed at all, and inside a bounds-growing image-filter layer,
//!   which would discard it along with its siblings. Their tests are
//!   `a_path_clip_opens_no_offscreen_because_it_installs_no_clip` and
//!   `a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings`;
//!   both are reasoned in the crate's `ARCHITECTURE.md`.

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

// ---------------------------------------------------------------------------
// `AntiAliasWithSaveLayer`: the clip applies once, to the group
// ---------------------------------------------------------------------------

/// The rounded clip every scene in this section is drawn through.
///
/// Axis-aligned and on integer bounds so the straight edges land on pixel
/// boundaries and only the four corners carry fractional coverage — the only
/// place the two modes can differ at all.
fn corner_clip() -> flui_types::geometry::RRect {
    flui_types::geometry::RRect::from_rect_circular(
        Rect::from_xywh(px(8.0), px(8.0), px(48.0), px(48.0)),
        px(16.0),
    )
}

/// One picture inside [`corner_clip`] under `behavior`.
fn clipped_tree(behavior: Clip, paint_content: impl FnOnce(&mut Canvas)) -> LayerTree {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        builder.push_clip_rrect(corner_clip(), behavior);
        let mut canvas = Canvas::new();
        paint_content(&mut canvas);
        builder.add_picture(canvas.finish());
        builder.build();
    }
    tree
}

/// A hard-edged rect covering the whole surface.
///
/// Hard-edged and surface-sized so the paint contributes no partial coverage of
/// its own: the clip's boundary is then the only fractional edge in the frame,
/// and the difference these tests measure cannot come from anywhere else.
fn full_surface(canvas: &mut Canvas, color: Color) {
    canvas.draw_rect(
        Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
        &Paint::fill(color).with_anti_alias(false),
    );
}

/// The alpha both translucent draws carry.
const DRAW_ALPHA: u8 = 200;

/// TWO COINCIDENT TRANSLUCENT DRAWS inside the rounded clip.
///
/// The content shape is what makes this scene able to see the defect at all,
/// and two conditions are easy to miss.
///
/// **One draw is never enough.** A single rect gives identical pixels whether
/// it is clipped on its own or composited once as a group, so a scene built on
/// one stays green either way.
///
/// **Overlapping draws are not enough either.** The two modes agree wherever
/// the clip's coverage is 1 or 0, so the overlap has to sit ON the clip's
/// fractional boundary. Two rects overlapping deep inside the clip differ by a
/// level of quantization and nothing else.
///
/// Coincident full-surface draws satisfy both: the overlap is everywhere, the
/// clip's whole feather included. Where coverage is `c` and both draws carry
/// alpha `a`, per-draw clipping reaches `1 - (1 - a·c)²` while the group
/// composite reaches `c · (2a - a²)`; the first is larger for every
/// `0 < c < 1`, which is the "darker or more opaque edge" this mode exists to
/// remove.
fn two_overlapping_translucent_draws(behavior: Clip) -> LayerTree {
    clipped_tree(behavior, |canvas| {
        full_surface(canvas, Color::rgba(0, 0, 255, DRAW_ALPHA));
        full_surface(canvas, Color::rgba(255, 0, 0, DRAW_ALPHA));
    })
}

/// The value [`two_overlapping_translucent_draws`] must produce under
/// `AntiAliasWithSaveLayer`: the two draws COMPOSED, then clipped once.
///
/// This is the expectation, not "some other number". A group composite applies
/// the clip to the finished group, so the result along the feather is exactly a
/// single draw of the composed colour at the same coverage — and a single draw
/// is where the two modes agree (per-draw and group coverage differ only where
/// draws overlap), which makes this reference a stable oracle produced by a
/// code path other than the one under test.
///
/// Red over blue, both at `DRAW_ALPHA` (`a = 200/255 = 0.784314`):
///
/// ```text
///   premul.r = 1 · a           = 0.784314
///   premul.b = 1 · a · (1 - a) = 0.169166
///   alpha    = a + a · (1 - a) = 0.953479
///   straight = premul / alpha  = (0.822567, 0, 0.177418)
///   as bytes                   = (210, 0, 45) at alpha 243
/// ```
fn the_two_draws_composed_once() -> LayerTree {
    clipped_tree(Clip::AntiAlias, |canvas| {
        full_surface(canvas, Color::rgba(210, 0, 45, 243));
    })
}

/// The same two translucent draws, moved apart so they do NOT overlap.
///
/// The control for [`two_overlapping_translucent_draws`]: same colours, same
/// alphas, same clip, same feather — only the overlap is gone. Each draw's own
/// edges are hard and sit in the gap between them, so the clip's boundary is
/// still the only fractional edge.
fn two_disjoint_translucent_draws(behavior: Clip) -> LayerTree {
    clipped_tree(behavior, |canvas| {
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(30.0), px(SIDE as f32)),
            &Paint::fill(Color::rgba(0, 0, 255, DRAW_ALPHA)).with_anti_alias(false),
        );
        canvas.draw_rect(
            Rect::from_xywh(px(34.0), px(0.0), px(30.0), px(SIDE as f32)),
            &Paint::fill(Color::rgba(255, 0, 0, DRAW_ALPHA)).with_anti_alias(false),
        );
    })
}

/// How far two renders of the same group may drift and still be the same
/// picture.
///
/// Not slop for a fuzzy comparison: two named, bounded sources of error, both
/// confined to the extreme fringe, and the bound is their sum rather than a
/// number picked to make a run pass.
///
/// 1. The group's colour makes one round trip through an 8-bit offscreen, and
///    the composed value is not exactly representable there (its premultiplied
///    blue is 43.14/255) — under one level.
/// 2. The composite is a textured quad, and that shader discards below 1/100
///    coverage where a rect draw has no alpha test at all. A fringe pixel the
///    rect path renders at up to `0.01 · 255` and the composite drops entirely
///    is worth up to 2.55 levels.
///
/// Neither term depends on the rasterizer: any rasterizer quantizes to the same
/// 8 bits and runs the same alpha test. The MEASUREMENT does — 2 on the
/// overlapping scene and 1 on the disjoint one, both on this machine's Vulkan
/// adapter. The merge-blocking `gpu-test` job runs on WARP, a different
/// rasterizer whose feather may place fringe pixels differently, and this bound
/// has not been checked there. It is stated as a derivation rather than fitted
/// to the measurement for that reason; the difference these tests are looking
/// for is 39, so what headroom the derivation leaves costs the oracle nothing.
const COMPOSITE_TOLERANCE: i32 = 4;

/// The floor the two modes must differ by for a scene to be able to tell them
/// apart at all.
///
/// Two thirds of the 39 levels the coincident-draws scene actually produces at
/// half coverage — enough margin for a rasterizer whose feather is narrower or
/// lands on different pixel centres, and still an order of magnitude above
/// [`COMPOSITE_TOLERANCE`], so a scene that only just clears it would still be
/// saying something.
const DISCRIMINATING_GAP: i32 = 25;

/// The largest per-channel difference between two renders, and the pixel it is
/// at.
fn largest_channel_difference(left: &[u8], right: &[u8]) -> (i32, usize) {
    (0..left.len())
        .step_by(4)
        .map(|byte| {
            let worst = (0..4)
                .map(|channel| {
                    (i32::from(left[byte + channel]) - i32::from(right[byte + channel])).abs()
                })
                .max()
                .unwrap_or(0);
            (worst, byte / 4)
        })
        .max_by_key(|&(worst, _)| worst)
        .unwrap_or((0, 0))
}

/// `AntiAliasWithSaveLayer` composites the clipped subtree ONCE, as a group.
///
/// Flutter renders the clipped subtree into an offscreen so the group
/// composites against the clip edge once. Applying coverage per draw instead
/// attenuates each draw by the clip and then blends it over an
/// already-attenuated one, which makes the edge darker or more opaque wherever
/// the content overlaps itself. A scene without such overlap is unaffected —
/// see the control below — which is what makes this a real defect and not a
/// visible one in the common case.
///
/// The expectation is a value, not an inequality: the group composite must
/// equal a single draw of the two colours composed
/// ([`the_two_draws_composed_once`]), everywhere, because that is what
/// compositing once means.
#[test]
fn the_save_layer_mode_composites_the_clipped_group_once() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let render = |tree: &LayerTree| {
        renderer
            .render_layer_tree(tree, (SIDE, SIDE))
            .expect("the headless capture path must rasterize a clipped tree")
    };

    let composited_once = render(&the_two_draws_composed_once());
    let save_layer = render(&two_overlapping_translucent_draws(
        Clip::AntiAliasWithSaveLayer,
    ));
    let per_draw = render(&two_overlapping_translucent_draws(Clip::AntiAlias));

    // The premise, first: the two modes must be able to disagree on this scene
    // at all. Without this the assertion below would pass against a renderer
    // that had never heard of the mode — which is exactly how the tripwire this
    // replaces came to fire on quantization instead of on the defect.
    let (per_draw_gap, per_draw_pixel) = largest_channel_difference(&per_draw, &composited_once);
    assert!(
        per_draw_gap > DISCRIMINATING_GAP,
        "premise: clipping each draw separately must visibly differ from \
         compositing the group once — the clip's feather is where it shows. \
         Largest difference was only {per_draw_gap} at pixel {per_draw_pixel}, \
         so this scene cannot tell the two apart and proves nothing"
    );

    let (gap, pixel) = largest_channel_difference(&save_layer, &composited_once);
    assert!(
        gap <= COMPOSITE_TOLERANCE,
        "`AntiAliasWithSaveLayer` must composite the clipped group once: every \
         pixel has to match a single draw of the two colours composed, within \
         {COMPOSITE_TOLERANCE}. Largest difference was {gap} at pixel {pixel}. \
         A difference near {per_draw_gap} means the clip is being applied per \
         draw again: the mode approximated by plain anti-alias, which is what \
         the two modes producing IDENTICAL pixels used to mean"
    );
}

/// The mode changes nothing where the content does not overlap itself.
///
/// The control, and it matters as much as the oracle above: per-draw coverage
/// and a group composite agree wherever at most one draw covers a pixel, so a
/// change that shifted anything else would be doing more than this mode asks
/// for. Applying the clip twice — the obvious wrong way to wire the offscreen —
/// fails here as well as above, because a double multiply moves a single draw
/// too.
#[test]
fn the_save_layer_mode_leaves_non_overlapping_content_alone() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let render = |behavior: Clip| {
        renderer
            .render_layer_tree(&two_disjoint_translucent_draws(behavior), (SIDE, SIDE))
            .expect("the headless capture path must rasterize a clipped tree")
    };

    // The premise: this scene's content reaches the clip's fractional boundary,
    // where the modes could differ if the change were not confined. Content
    // that never touched the feather would make the assertion below vacuous.
    // The hard-vs-smooth axis is independent of the mode under test.
    let (feather, _) =
        largest_channel_difference(&render(Clip::HardEdge), &render(Clip::AntiAlias));
    assert!(
        feather > DISCRIMINATING_GAP,
        "premise: the disjoint draws must reach the clip's feather — thresholding \
         it should visibly change them. Largest difference was only {feather}, so \
         this scene has no fractional coverage to be confined to"
    );

    let (gap, pixel) = largest_channel_difference(
        &render(Clip::AntiAliasWithSaveLayer),
        &render(Clip::AntiAlias),
    );
    assert!(
        gap <= COMPOSITE_TOLERANCE,
        "the same two draws, moved apart so they do not overlap, must render \
         the same under both modes — the offscreen exists to fix overlap and \
         nothing else. Largest difference was {gap} at pixel {pixel}, against a \
         tolerance of {COMPOSITE_TOLERANCE} (see `COMPOSITE_TOLERANCE`)"
    );
}

/// A red backdrop, then `paint_inside` within a clip in `behavior`.
///
/// The backdrop is a picture of its own, OUTSIDE the clip layer, which is what
/// makes the offscreen observable: the layer composites back with `SrcOver`, so
/// a destructive blend inside it cannot reach a ground painted outside it,
/// while without the layer the two share a pass and it can.
fn inside_a_clip(
    behavior: Clip,
    push_clip: impl FnOnce(&mut SceneBuilder<'_>, Clip),
    paint_inside: impl FnOnce(&mut Canvas),
) -> LayerTree {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        builder.push_offset(flui_types::Offset::ZERO);

        let mut canvas = Canvas::new();
        full_surface(&mut canvas, Color::rgb(255, 0, 0));
        builder.add_picture(canvas.finish());

        push_clip(&mut builder, behavior);
        let mut canvas = Canvas::new();
        paint_inside(&mut canvas);
        builder.add_picture(canvas.finish());
        builder.pop().expect("the clip is open");
        builder.build();
    }
    tree
}

/// A full-surface eraser.
fn erase_everything(canvas: &mut Canvas) {
    canvas.draw_rect(
        Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
        &Paint::fill(Color::rgb(0, 0, 0))
            .with_anti_alias(false)
            .with_blend_mode(flui_types::painting::BlendMode::Clear),
    );
}

/// An unbounded green fill — the shape `RenderPhysicalModel` emits.
///
/// That render object is the mode's only production consumer, and its fill goes
/// through `Canvas::draw_paint`, which has no geometry of its own:
/// `Backend::render_paint` expands it to the whole viewport, so its extent is
/// decided entirely by the clip.
fn fill_everything(canvas: &mut Canvas) {
    canvas.draw_paint(&Paint::fill(Color::rgb(0, 160, 0)).with_anti_alias(false));
}

/// A RECT clip in `AntiAliasWithSaveLayer` opens the offscreen, and the
/// offscreen isolates — while the clip goes on clipping.
///
/// The rect clip's own pixels cannot show the mode: the clip is the hardware
/// scissor, which is binary, so applying it once to the group and once per draw
/// give the same answer for every `SrcOver` scene. Isolation is the half that
/// remains observable, and it is the half `Clip`'s own contract calls a
/// semantic change — so it is what this pins. Without it the mode would be
/// silently unhonoured for rect clips, which is the defect #848 opened on.
///
/// TWO scenes, because one cannot carry both oracles: a fill drawn after the
/// eraser hides exactly the pixels the isolation oracle reads, and an eraser
/// alone leaves nothing whose confinement could be measured.
#[test]
fn a_rect_clip_in_the_save_layer_mode_isolates_a_destructive_blend() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let clip_rect = |builder: &mut SceneBuilder<'_>, behavior: Clip| {
        builder.push_clip_rect(
            Rect::from_xywh(px(16.0), px(16.0), px(32.0), px(32.0)),
            behavior,
        );
    };
    let render = |behavior: Clip, paint_inside: fn(&mut Canvas)| {
        renderer
            .render_layer_tree(
                &inside_a_clip(behavior, clip_rect, paint_inside),
                (SIDE, SIDE),
            )
            .expect("the headless capture path must rasterize the scene")
    };

    // Scene 1, isolation. The premise first: without the layer the eraser
    // reaches the backdrop, so the assertion after it is not vacuous.
    let per_draw = sample(&render(Clip::AntiAlias, erase_everything), 32, 32);
    assert!(
        per_draw[0] < 8,
        "premise: with no offscreen the eraser must reach the backdrop through \
         the clip, got {per_draw:?}"
    );
    let isolated = sample(
        &render(Clip::AntiAliasWithSaveLayer, erase_everything),
        32,
        32,
    );
    assert!(
        isolated[0] > 248 && isolated[1] < 8,
        "`AntiAliasWithSaveLayer` must render the clipped subtree into an \
         offscreen, so an eraser inside the clip cannot reach a backdrop \
         painted outside it — the layer composites back with `SrcOver`. Got \
         {isolated:?}, which is what a clip that never opened the layer leaves"
    );

    // Scene 2, the clip still clips. Nothing above would notice a rect clip
    // whose scissor stopped reaching the offscreen: an eraser is invisible
    // outside the clip either way. An unbounded fill is not.
    let filled = render(Clip::AntiAliasWithSaveLayer, fill_everything);
    let inside = sample(&filled, 32, 32);
    assert!(
        inside[1] > 128 && inside[0] < 64,
        "premise: the unbounded fill must land inside the clip, got {inside:?}"
    );
    let outside = sample(&filled, 4, 4);
    assert!(
        outside[0] > 248 && outside[1] < 64,
        "the backdrop must survive OUTSIDE the clip: the fill has no geometry \
         of its own, so only the clip keeps it off (4, 4), got {outside:?}"
    );
}

/// A PATH clip opens no offscreen BECAUSE it installs no clip.
///
/// The offscreen is granted on `ClipOutcome`, not on which `push_clip_*` was
/// entered, so this is a consequence rather than an exemption:
/// `WgpuPainter::clip_path` warns and returns without touching any state, and a
/// group composite has no edge to composite against.
///
/// Both halves are asserted, precondition first. A test that pinned only the
/// consequence would keep passing after its premise was repaired — and that
/// repair is imminent: issue #921 records that `ClipSuperellipseLayer` routes
/// its squircle through this same call and so does not clip at all, even though
/// `GpuStateStack::clip_rsuperellipse` implements one. When `clip_path` starts
/// installing, the precondition assertion fails first and says exactly what
/// changed; the consequence follows on its own, with no code to update.
#[test]
fn a_path_clip_opens_no_offscreen_because_it_installs_no_clip() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let render = |behavior: Clip| {
        let tree = inside_a_clip(
            behavior,
            |builder, behavior| {
                let mut path = flui_types::painting::Path::new();
                path.add_rect(Rect::from_xywh(px(16.0), px(16.0), px(32.0), px(32.0)));
                builder.push_clip_path(path, behavior);
            },
            erase_everything,
        );
        renderer
            .render_layer_tree(&tree, (SIDE, SIDE))
            .expect("the headless capture path must rasterize the scene")
    };

    // The PRECONDITION the rest of this test rests on, asserted rather than
    // assumed: `clip_path` installs nothing, so its content is unclipped.
    // (32, 32) is inside the path; (2, 2) is far outside it and must be painted
    // all the same. Read on the BLUE channel — the ground is white, so the red
    // backdrop is the one that discriminates.
    let outside_the_path = sample(&render(Clip::AntiAliasWithSaveLayer), 2, 2);
    assert!(
        outside_the_path[2] < 8,
        "precondition: `Painter::clip_path` installs no clip, so the backdrop \
         is painted outside the path too, got {outside_the_path:?}. If this \
         fails, path clipping now works (#921) — and the offscreen below \
         follows from `ClipOutcome` with no code change, so update this test, \
         not the backend"
    );

    let escaped = sample(&render(Clip::AntiAliasWithSaveLayer), 32, 32);
    assert!(
        escaped[0] < 8,
        "with no clip installed there is no edge for a group composite, so no \
         offscreen is opened and an eraser inside still reaches the backdrop, \
         got {escaped:?}"
    );
    assert_eq!(
        render(Clip::AntiAliasWithSaveLayer),
        render(Clip::AntiAlias),
        "a path clip installs no clip at all, so its modes cannot differ"
    );
}

/// The clip's subtree and a sibling drawn before it, inside a blur layer.
///
/// `with_filter` decides whether the pair is wrapped; everything else is
/// identical, so the wrapped and unwrapped renders differ only by the filter
/// layer.
fn a_clip_beside_a_sibling(with_filter: bool) -> LayerTree {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        builder.push_offset(flui_types::Offset::ZERO);
        if with_filter {
            builder.push_blur(1.0);
        }

        // A sibling FLUSHED BEFORE the clip. Opening an offscreen finalises the
        // enclosing layer's pending segment into its draw order, so this is
        // discarded alongside the clip's own subtree, not just beside it.
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(24.0), px(SIDE as f32)),
            &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
        );
        builder.add_picture(canvas.finish());

        builder.push_clip_rect(
            Rect::from_xywh(px(32.0), px(0.0), px(32.0), px(SIDE as f32)),
            Clip::AntiAliasWithSaveLayer,
        );
        let mut canvas = Canvas::new();
        full_surface(&mut canvas, Color::rgb(0, 0, 255));
        builder.add_picture(canvas.finish());
        builder.pop().expect("the clip is open");

        if with_filter {
            builder.pop().expect("the blur is open");
        }
        builder.build();
    }
    tree
}

/// Inside a bounds-growing image-filter layer the mode DEGRADES; it does not
/// delete the content.
///
/// Those layers carry only their final `DrawSegment` into `FilterOp::input` and
/// discard `offscreen_items`. A `DrawItem::OpacityLayer` opened inside one is
/// therefore thrown away — and so is every sibling already flushed into the
/// enclosing layer's draw order, because opening the layer finalises the pending
/// segment first. `Backend::opens_offscreen` declines the offscreen there and
/// falls back to per-draw coverage: losing an edge beats losing the picture.
///
/// Both samples matter. The blue is the clip's own subtree; the red is the
/// sibling drawn BEFORE it, which is the half that makes this a data-loss bug
/// rather than a clipping one. The red sample doubles as the pin that the
/// degraded path still CLIPS: blue is `(0, 0, 255)`, so a leak past the clip
/// rect would take the red channel down with it.
///
/// The other direction — that the refusal is narrow, and an offscreen is still
/// opened everywhere else — is
/// `a_rect_clip_in_the_save_layer_mode_isolates_a_destructive_blend`, which
/// fails the moment `opens_offscreen` declines unconditionally.
///
/// What is NOT pinned, deliberately: that the degraded content inside a filter
/// layer takes per-draw coverage rather than group coverage. The two differ
/// only along a clip's fractional edge under overlapping translucency, and a
/// blur pass smears exactly that edge — there is no sample point here that
/// could tell them apart, so no assertion pretends to.
#[test]
fn a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let render = |with_filter: bool| {
        renderer
            .render_layer_tree(&a_clip_beside_a_sibling(with_filter), (SIDE, SIDE))
            .expect("the headless capture path must rasterize the scene")
    };

    // Each colour is read on the channel the WHITE ground does not share with
    // it: the ground is `(255, 255, 255)`, so "blue is present" asserted on the
    // blue channel passes against a blank frame and proves nothing. Red content
    // is read on BLUE, blue content on RED.
    let red_is_present = |pixel: [u8; 4]| pixel[2] < 64;
    let blue_is_present = |pixel: [u8; 4]| pixel[0] < 64;

    // The premise: unwrapped, the scene paints both. Without this the wrapped
    // assertions would pass against a scene that never painted anything.
    let unwrapped = render(false);
    let (sibling_x, clipped_x) = (8, 48);
    let unwrapped_sibling = sample(&unwrapped, sibling_x, 32);
    let unwrapped_clipped = sample(&unwrapped, clipped_x, 32);
    assert!(
        red_is_present(unwrapped_sibling) && blue_is_present(unwrapped_clipped),
        "premise: outside a filter layer the scene paints a red sibling and a \
         blue clipped subtree, got {unwrapped_sibling:?} and {unwrapped_clipped:?}"
    );

    let wrapped = render(true);
    let sibling = sample(&wrapped, sibling_x, 32);
    let clipped = sample(&wrapped, clipped_x, 32);
    assert!(
        blue_is_present(clipped),
        "the clip's own subtree must survive inside a filter layer, got \
         {clipped:?} — the white ground. Gone means an offscreen was opened \
         where the enclosing layer discards nested draw items"
    );
    assert!(
        red_is_present(sibling),
        "the SIBLING drawn before the clip must survive too, got {sibling:?}. \
         Opening an offscreen finalises the enclosing layer's pending segment \
         into its draw order first, so it is discarded with the clip"
    );
}

/// An ancestor's per-draw clip still clips the draws INSIDE a save-layer
/// offscreen.
///
/// `clip_rrect_at_composite` installs only the bounding scissor and leaves the
/// per-draw SDF slot alone. That is what lets the group composite apply its own
/// coverage once — and it is also the whole reason nesting works: an enclosing
/// `Clip::AntiAlias` is still in the slot, and every draw that goes into the
/// offscreen is still subject to it.
///
/// The nesting test cannot see this, because there the inner clip sits strictly
/// inside the outer and no content ever reaches the outer's round. Here the
/// inner save-layer clip is LARGER than the outer, with square corners, so its
/// own composite excludes nothing and the outer's rounded corner is the only
/// boundary in the frame. If the ancestor's SDF stopped applying inside the
/// offscreen, the outer's bounding-box scissor would be all that remained and
/// the corner would fill in.
#[test]
fn an_ancestor_clip_still_clips_the_draws_inside_a_save_layer_offscreen() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        builder.push_clip_rrect(corner_clip(), Clip::AntiAlias);
        // Square corners and larger than the outer clip: this one's composite
        // must not be what keeps the content off the sample point.
        builder.push_clip_rrect(
            flui_types::geometry::RRect::from_rect_circular(
                Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
                px(0.0),
            ),
            Clip::AntiAliasWithSaveLayer,
        );
        let mut canvas = Canvas::new();
        full_surface(&mut canvas, Color::rgb(0, 0, 255));
        builder.add_picture(canvas.finish());
        builder.build();
    }

    let pixels = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize a nested clipped tree");

    // The premise: content inside the outer round is painted, so the corner
    // assertion is about the clip and not about an empty frame.
    let centre = sample(&pixels, 32, 32);
    assert!(
        centre[0] < 8 && centre[2] > 248,
        "premise: content inside the outer clip must be painted blue, got {centre:?}"
    );

    // (9, 9) is inside the outer clip's bounding box — so its scissor admits
    // it — and 20.5 px from the corner arc's centre (24, 24), radius 16, so
    // only the ancestor's SDF can keep it clear. Read on RED: the content is
    // blue and the ground is white, and they agree on blue.
    let past_the_ancestor_round = sample(&pixels, 9, 9);
    assert!(
        past_the_ancestor_round[0] > 248,
        "an enclosing `Clip::AntiAlias` must still clip every draw inside the \
         offscreen — `clip_rrect_at_composite` leaves its SDF slot in place for \
         exactly this. Got {past_the_ancestor_round:?} at (9, 9), which is the \
         outer clip's bounding box with its rounded corner ignored"
    );
}

/// The outer clip of the nesting scene. Its rounded corner is what
/// `NESTED_OUTSIDE_OUTER_ROUND` samples.
fn nesting_outer_clip() -> flui_types::geometry::RRect {
    flui_types::geometry::RRect::from_rect_circular(
        Rect::from_xywh(px(4.0), px(4.0), px(56.0), px(56.0)),
        px(8.0),
    )
}

/// The inner clip of the nesting scene, wholly inside [`nesting_outer_clip`].
fn nesting_inner_clip() -> flui_types::geometry::RRect {
    flui_types::geometry::RRect::from_rect_circular(
        Rect::from_xywh(px(16.0), px(16.0), px(32.0), px(32.0)),
        px(8.0),
    )
}

/// Inside the INNER clip's bounding box, outside its rounded corner.
///
/// The corner arc is centred at `(24, 24)` with radius 8, and this pixel's
/// centre `(17.5, 17.5)` is 9.19 away — outside. Only the inner clip's own
/// coverage can keep the blue off it.
const NESTED_OUTSIDE_INNER_ROUND: (u32, u32) = (17, 17);

/// Inside the OUTER clip's bounding box, outside its rounded corner.
///
/// Arc centred at `(12, 12)` radius 8; this pixel's centre `(5.5, 5.5)` is 9.19
/// away. Only the outer clip's own coverage can keep the red band off it.
const NESTED_OUTSIDE_OUTER_ROUND: (u32, u32) = (5, 5);

/// Outside BOTH clips, and outside the marker-free zone the two above sample.
const NESTED_AFTER_BOTH_POPS: (u32, u32) = (1, 1);

/// Two nested clips of mixed modes, each with its own content, and a marker
/// drawn after both pops.
///
/// Three draws, one per thing that can go wrong:
///
/// - **blue**, inside both clips — the inner clip's coverage is the only thing
///   that keeps it off `NESTED_OUTSIDE_INNER_ROUND`;
/// - **red**, a band between the two clips, drawn after the inner pop — the
///   outer clip's coverage is the only thing that keeps it off
///   `NESTED_OUTSIDE_OUTER_ROUND`;
/// - **green**, a marker in the surface corner drawn after BOTH pops — only a
///   fully released stack leaves it unclipped.
///
/// A save-layer clip applies its coverage at the composite, so a `pop_clip`
/// that fails to close the layer never applies it at all and the corresponding
/// draw leaks past its round. A `pop_clip` that closes a layer that was never
/// opened underflows the compositor and composites the OTHER clip's layer early
/// — before its own content is drawn — which leaks the same way.
fn nested_mixed_clip_tree(outer: Clip, inner: Clip) -> LayerTree {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        // An inert container, so the marker below is a SIBLING of the clips and
        // not an orphan: the first layer pushed becomes the root, and a leaf
        // added on an empty stack is never attached to the tree at all.
        builder.push_offset(flui_types::Offset::ZERO);

        builder.push_clip_rrect(nesting_outer_clip(), outer);
        builder.push_clip_rrect(nesting_inner_clip(), inner);
        let mut canvas = Canvas::new();
        full_surface(&mut canvas, Color::rgb(0, 0, 255));
        builder.add_picture(canvas.finish());
        builder.pop().expect("the inner clip is open");

        // A band between the two clips: below the outer's top edge, above the
        // inner's. Disjoint from the blue, so neither can hide the other.
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(14.0)),
            &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
        );
        builder.add_picture(canvas.finish());
        builder.pop().expect("the outer clip is open");

        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(4.0), px(4.0)),
            &Paint::fill(Color::rgb(0, 160, 0)).with_anti_alias(false),
        );
        builder.add_picture(canvas.finish());
        builder.build();
    }
    tree
}

/// Nested clips of MIXED modes each apply their own coverage, and both release.
///
/// `pop_clip` serves all three `push_clip_*` variants and takes no argument, so
/// nothing in the call can say whether the push it balances opened an offscreen
/// — only `Backend`'s frame stack can. Nesting the two modes is what makes a
/// desynchronised stack observable: each pop would close the other's frame.
///
/// Both orders run, because only one of the two pops is the save-layer one in
/// each, and a stack that is wrong in one direction can be right in the other.
#[test]
fn nested_clips_of_mixed_modes_close_in_the_order_they_opened() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    for (outer, inner) in [
        (Clip::AntiAlias, Clip::AntiAliasWithSaveLayer),
        (Clip::AntiAliasWithSaveLayer, Clip::AntiAlias),
    ] {
        let pixels = renderer
            .render_layer_tree(&nested_mixed_clip_tree(outer, inner), (SIDE, SIDE))
            .expect("the headless capture path must rasterize a nested clipped tree");
        let at = |(x, y): (u32, u32)| sample(&pixels, x, y);
        let modes = format!("outer={outer:?} inner={inner:?}");

        // The premise: both draws land somewhere. Without it every assertion
        // below would pass against a renderer that painted nothing at all.
        let inside_both = at((32, 32));
        assert!(
            inside_both[0] < 8 && inside_both[2] > 248,
            "premise: content inside both clips must be painted blue ({modes}), \
             got {inside_both:?}"
        );
        let inside_outer_only = at((32, 8));
        assert!(
            inside_outer_only[0] > 248 && inside_outer_only[2] < 8,
            "premise: the band between the two clips must be painted red \
             ({modes}), got {inside_outer_only:?}"
        );

        // The inner clip's own coverage.
        let past_inner_round = at(NESTED_OUTSIDE_INNER_ROUND);
        assert!(
            past_inner_round[2] > 248 && past_inner_round[0] > 248,
            "the blue must not reach past the INNER clip's rounded corner \
             ({modes}), got {past_inner_round:?} at \
             {NESTED_OUTSIDE_INNER_ROUND:?}. A save-layer clip applies its \
             coverage at the composite, so blue here means the layer its push \
             opened was never closed"
        );

        // The outer clip's own coverage, on content drawn after the inner pop.
        let past_outer_round = at(NESTED_OUTSIDE_OUTER_ROUND);
        assert!(
            past_outer_round[0] > 248 && past_outer_round[2] > 248,
            "the red band must not reach past the OUTER clip's rounded corner \
             ({modes}), got {past_outer_round:?} at \
             {NESTED_OUTSIDE_OUTER_ROUND:?}. Red here means the inner pop \
             closed the outer clip's frame"
        );

        // Both pops released.
        let after_both_pops = at(NESTED_AFTER_BOTH_POPS);
        assert!(
            after_both_pops[1] > 128 && after_both_pops[0] < 64,
            "the marker drawn AFTER both pops must reach the corner unclipped \
             ({modes}), got {after_both_pops:?} at {NESTED_AFTER_BOTH_POPS:?}"
        );
    }
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

    // The feather must reach LOW coverage, not merely exist.
    //
    // The clip shader discards fully clipped-out fragments, and the threshold
    // for that has to be exactly zero. A "small" threshold — discarding
    // anything under, say, 0.5 coverage — still leaves a feather, just a
    // truncated one, so the comparison above stays true and says nothing.
    // Blue over white at coverage c reads `(255(1-c), 255(1-c), 255)`, so a
    // faint fringe pixel is one whose red channel is still high.
    let faint = render(Clip::AntiAlias)
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| (200..=250).contains(&p[0]) && p[2] > 250)
        .count();
    assert!(
        faint > 0,
        "the anti-aliased fringe must include faint, low-coverage pixels; \
         none means the clip is discarding fragments it should still blend"
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

/// A destructive blend mode must not escape a rounded clip's corners.
///
/// The SDF clip modulates alpha rather than discarding: the shader ends in
/// `color.a * alpha * clip_alpha`, and `clip_alpha` is `0.0` outside the clip.
/// For `SrcOver` that is indistinguishable from being clipped — zero alpha
/// contributes nothing. For a destination-destructive mode it is not, because
/// the blend factors clear or replace the destination regardless of source
/// alpha.
///
/// So a full-surface `Clear` through a rounded clip wipes the clip's whole
/// BOUNDING BOX, corners included, instead of only the rounded region.
///
/// Both draws share ONE canvas on purpose. A `push_clip_rrect` LAYER isolates
/// its content, so an eraser inside it cannot reach a ground painted outside
/// it — the layer composites back with `SrcOver` and the ground survives for a
/// reason that has nothing to do with the clip. The canvas-level clip keeps
/// both in the same pass, which is where the escape is observable.
///
/// The oracle samples the CORNER: inside the rounded region both the correct
/// and the broken renderer clear, and outside the bounding box neither does.
/// Only the corner — inside the box, outside the round — tells them apart,
/// which is why every existing clip oracle missed this. They all fill with
/// `SrcOver`.
#[test]
fn a_destructive_blend_does_not_escape_a_rounded_clip() {
    use flui_types::painting::{BlendMode, ClipOp};

    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        let mut canvas = Canvas::new();

        // Opaque red ground, unclipped.
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
            &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
        );

        // Then a full-surface CLEAR through a rounded clip, same pass.
        canvas.save();
        canvas.clip_rrect_ext(
            flui_types::geometry::RRect::from_rect_circular(
                Rect::from_xywh(px(8.0), px(8.0), px(48.0), px(48.0)),
                px(16.0),
            ),
            ClipOp::Intersect,
            Clip::HardEdge,
        );
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
            &Paint::fill(Color::rgb(0, 0, 0))
                .with_anti_alias(false)
                .with_blend_mode(BlendMode::Clear),
        );
        canvas.restore();

        builder.add_picture(canvas.finish());
        builder.build();
    }

    let pixels = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene");

    let at = |x: usize, y: usize| {
        let i = (y * SIDE as usize + x) * 4;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    };

    // (32, 32) is the centre — well inside the round, so the clear must land.
    let centre = at(32, 32);
    // (9, 9) is inside the clip's bounding box but outside its rounded corner.
    // The corner arc is centred at (origin + radius) = (24, 24) with radius 16,
    // and (9, 9) is 21.2 away — outside the arc, so the ground must survive.
    let corner = at(9, 9);

    assert!(
        centre.0 < 250,
        "premise: the clear must take effect inside the rounded region, \
         got {centre:?} at the centre — without that this proves nothing"
    );
    assert!(
        corner.0 > 250 && corner.1 < 5 && corner.2 < 5,
        "the ground must survive OUTSIDE the rounded corner: a destructive \
         blend that only zeroes alpha still clears the whole bounding box, \
         got {corner:?} at (9, 9)"
    );
}

/// An ANTI-ALIASED clip FEATHERS a destructive blend's fringe — and a device
/// without a second blend source still cannot.
///
/// This replaces a tripwire that asserted the opposite. That test pinned the
/// state where `sdfToAlpha` produced fractional coverage along the rounded
/// edge, the fragment reached the blender, and `Clear`'s `(Zero, Zero)` factors
/// wiped the destination regardless of the alpha emitted — a hard-edged eraser.
/// It was written to fail once coverage stopped riding in the source alpha, and
/// it fired as designed — reporting 68 pixels along this exact edge that it
/// required to be zero. The count is a record of that firing, not a number
/// this test asserts; the exact per-mode arithmetic lives in
/// `coverage_blend_readback_tests`.
///
/// The old claim is not discarded, it is DEMOTED to the fallback: a device
/// without `wgpu::Features::DUAL_SOURCE_BLENDING` has nowhere to put coverage
/// but the alpha channel `Clear` ignores, so the second half of this test
/// asserts the hard fringe still, on a device built with the feature withheld.
/// Rendering both ways in one test is what makes each half evidence — a single
/// render could not tell "the fix works" from "this scene never had a fringe".
///
/// The sibling tests miss this by construction: the destructive one uses
/// `HardEdge`, which has no fringe, and the fringe one uses `SrcOver`, whose
/// destination factor absorbs partial coverage already.
#[test]
fn an_anti_aliased_destructive_blend_feathers_its_fringe() {
    use flui_types::painting::{BlendMode, ClipOp};

    /// Pixels along the edge that are neither untouched ground nor fully
    /// erased. The ground is opaque red, so the RED channel discriminates:
    /// 255 = untouched, 0 = erased, anything between = partially erased.
    fn partially_erased(pixels: &[u8]) -> usize {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| (5..250).contains(&pixel[0]))
            .count()
    }

    fn erase_through_an_anti_aliased_clip(renderer: &HeadlessRenderer) -> Vec<u8> {
        let mut tree = LayerTree::new();
        {
            let mut builder = SceneBuilder::new(&mut tree);
            let mut canvas = Canvas::new();
            canvas.draw_rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
                &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
            );
            canvas.save();
            canvas.clip_rrect_ext(
                flui_types::geometry::RRect::from_rect_circular(
                    Rect::from_xywh(px(8.0), px(8.0), px(48.0), px(48.0)),
                    px(16.0),
                ),
                ClipOp::Intersect,
                Clip::AntiAlias,
            );
            canvas.draw_rect(
                Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32)),
                &Paint::fill(Color::rgb(0, 0, 0))
                    .with_anti_alias(false)
                    .with_blend_mode(BlendMode::Clear),
            );
            canvas.restore();
            builder.add_picture(canvas.finish());
            builder.build();
        }

        renderer
            .render_layer_tree(&tree, (SIDE, SIDE))
            .expect("the headless capture path must rasterize the scene")
    }

    let Ok(feathering) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let folded = HeadlessRenderer::without_dual_source_blending()
        .expect("an adapter that answered once must answer again with fewer features");

    // The fallback half runs on every device, including one whose adapter
    // simply lacks the feature.
    assert_eq!(
        partially_erased(&erase_through_an_anti_aliased_clip(&folded)),
        0,
        "without a second blend source, coverage has nowhere to ride but the \
         alpha channel `Clear`'s (Zero, Zero) factors ignore, so every pixel \
         along the edge is either untouched or fully erased"
    );

    if !feathering.supports_dual_source_blending() {
        eprintln!(
            "skipping the feathered half: this adapter does not expose \
             DUAL_SOURCE_BLENDING, so both renderers take the folded path"
        );
        return;
    }

    let feathered = partially_erased(&erase_through_an_anti_aliased_clip(&feathering));
    assert!(
        feathered > 0,
        "an anti-aliased eraser must feather its edge: with coverage on its \
         own blend channel, `Clear` erases a fringe pixel in proportion to how \
         much of it the clip admits, leaving a partial value. Found none"
    );
}

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
//! - a **rounded** clip honours `HardEdge` vs `AntiAlias`;
//! - a **rect** clip does not — it is the hardware scissor under both modes,
//!   because routing it to the SDF costs text clipping, nested intersection
//!   and exactness (`Painter::clip_rect` has the full reasoning). That one is
//!   still pinned by a test asserting the known-wrong equality, deliberately,
//!   so it fails in the right place when the shader grows a clip stack;
//! - `AntiAliasWithSaveLayer` renders the clipped subtree into an offscreen and
//!   applies the clip's coverage ONCE, to the finished group. The offscreen
//!   is declined in one case, and it is not keyed on the clip's shape: inside a
//!   bounds-growing image-filter layer, which would discard the offscreen along
//!   with its siblings
//!   (`a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings`,
//!   reasoned in the crate's `ARCHITECTURE.md`).

use flui_foundation::geometry::Rect;
use flui_layer::{LayerTree, SceneBuilder};
use flui_painting::{Canvas, Paint};
use flui_painting::{paint::Clip, styling::Color};

use crate::headless::HeadlessRenderer;

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
        Rect::from_xywh(0.0, 0.0, f64::from(SIDE as f32), f64::from(SIDE as f32)),
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
fn a_clip_rect_layer_clips_its_content_and_its_absence_does_not() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let clipped_tree = {
        let mut builder = SceneBuilder::new();
        builder.push_clip_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIDE as f32), f64::from(CLIP_BOTTOM)),
            Clip::HardEdge,
        );
        builder.add_picture(full_surface_content());
        builder.build()
    };

    let unclipped_tree = {
        let mut builder = SceneBuilder::new();
        builder.add_picture(full_surface_content());
        builder.build()
    };

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

// ---------------------------------------------------------------------------
// Per-mode edges (#848)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// `AntiAliasWithSaveLayer`: the clip applies once, to the group
// ---------------------------------------------------------------------------

/// A hard-edged rect covering the whole surface.
///
/// Hard-edged and surface-sized so the paint contributes no partial coverage of
/// its own: the clip's boundary is then the only fractional edge in the frame,
/// and the difference these tests measure cannot come from anywhere else.
fn full_surface(canvas: &mut Canvas, color: Color) {
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, f64::from(SIDE as f32), f64::from(SIDE as f32)),
        &Paint::fill(color).with_anti_alias(false),
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
    push_clip: impl FnOnce(&mut SceneBuilder, Clip),
    paint_inside: impl FnOnce(&mut Canvas),
) -> LayerTree {
    let mut builder = SceneBuilder::new();
    builder.push_offset(flui_foundation::geometry::Offset::ZERO);

    let mut canvas = Canvas::new();
    full_surface(&mut canvas, Color::rgb(255, 0, 0));
    builder.add_picture(canvas.finish());

    push_clip(&mut builder, behavior);
    let mut canvas = Canvas::new();
    paint_inside(&mut canvas);
    builder.add_picture(canvas.finish());
    builder.pop().expect("the clip is open");
    builder.build()
}

/// The squircle used by the `ClipSuperellipseLayer` tests: the whole surface,
/// corner radius equal to the half-extent — an iOS app-icon shape.
///
/// The sample points below are derived from the SDF this shape is evaluated
/// with, `sdRoundedSuperellipse` in `shaders/common/clip.wgsl`. With
/// `b = (32, 32)` and `r3 = 32`, `q = |p|` for every pixel, so the corner
/// branch is always the active one and the distance is
/// `(⁴√(ax⁴ + ay⁴) − 1) · 32` where `ax = ay = |p| / 32`. The rounded-box SDF
/// the *approximating* rrect would use replaces that fourth-power norm with the
/// Euclidean one, which is what makes point B below discriminate.
///
/// | point | pixel centre | \|p\| | squircle | circle of the same radius |
/// |---|---|---|---|---|
/// | A `(2, 2)` | `(2.5, 2.5)` | 29.5 | **+3.08 outside** | +9.72 outside |
/// | B `(7, 7)` | `(7.5, 7.5)` | 24.5 | **−2.87 inside** | **+2.65 outside** |
/// | C `(32, 32)` | `(32.5, 32.5)` | 0.5 | −31.4 inside | −31.29 inside |
///
/// Every margin clears the roughly one-pixel anti-aliasing band, so no
/// assertion below depends on a coverage threshold.
fn icon_squircle() -> flui_foundation::geometry::RSuperellipse {
    flui_foundation::geometry::RSuperellipse::from_rect_circular(
        Rect::from_xywh(0.0, 0.0, f64::from(SIDE as f32), f64::from(SIDE as f32)),
        f64::from(SIDE as f32 / 2.0),
    )
}

/// The squircle the GPU clips to is the squircle the CPU generator describes.
///
/// The two are independent expressions of the same shape:
/// `sdRoundedSuperellipse` in `shaders/common/clip.wgsl` evaluates a signed
/// distance in the fragment shader, and `superellipse::generate_superellipse_path`
/// walks the same parametric form on the CPU into a `Path`. `clip.wgsl` is the
/// SHIPPED evaluator — every clip-evaluating shader is prepended with it — and
/// until this test nothing held the two statements of the shape to each other.
/// (`common/sdf.wgsl` carries a reference copy that reaches no GPU; it is not
/// what this measures, and its doc now says so.)
///
/// The neighbouring tests pin three hand-computed sample points, which proves
/// the SDF is not the approximating rounded rectangle but says nothing about
/// the rest of the boundary. This walks a whole grid and asks the CPU path
/// whether each pixel is in or out, so a divergence anywhere on the curve
/// surfaces as a coordinate rather than as a demo that looks slightly wrong.
///
/// **Points near the boundary are skipped, and skipped by construction rather
/// than by a tolerance.** A pixel whose eight neighbours do not all agree with
/// it sits within a pixel of the edge, where the SDF is deliberately feathering
/// and the CPU predicate is a hard in/out — so the two must disagree there and
/// an assertion would be measuring anti-aliasing, not geometry. The count of
/// surviving points is asserted too: a filter that excluded everything would
/// otherwise leave this test green and empty.
fn the_squircle_sdf_agrees_with_the_cpu_path_across_the_whole_boundary() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };

    let squircle = icon_squircle();
    let tree = inside_a_clip(
        Clip::AntiAlias,
        |builder, behavior| {
            builder.push_clip_superellipse(squircle, behavior);
        },
        fill_everything,
    );
    let frame = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene");

    let path = crate::superellipse::generate_superellipse_path(&squircle);
    let inside_cpu = |x: u32, y: u32| {
        path.contains(flui_foundation::geometry::Point::new(
            f64::from(x as f32 + 0.5),
            f64::from(y as f32 + 0.5),
        ))
    };

    let mut compared = 0_usize;
    for y in 1..SIDE - 1 {
        for x in 1..SIDE - 1 {
            let here = inside_cpu(x, y);
            let unanimous = (-1i64..=1)
                .flat_map(|dy| (-1i64..=1).map(move |dx| (dx, dy)))
                .all(|(dx, dy)| {
                    inside_cpu(
                        u32::try_from(i64::from(x) + dx).expect("x stays in 0..SIDE"),
                        u32::try_from(i64::from(y) + dy).expect("y stays in 0..SIDE"),
                    ) == here
                });
            if !unanimous {
                continue;
            }
            compared += 1;
            let pixel = sample(&frame, x, y);
            if here {
                assert!(
                    pixel[1] > 128 && pixel[0] < 64,
                    "({x}, {y}) is inside the CPU squircle, so the clip must \
                     keep the fill there, got {pixel:?}"
                );
            } else {
                assert!(
                    pixel[0] > 200 && pixel[1] < 64,
                    "({x}, {y}) is outside the CPU squircle, so the backdrop \
                     must survive there, got {pixel:?}"
                );
            }
        }
    }

    assert!(
        compared > 2_000,
        "the boundary filter must leave the interior and the far exterior to \
         compare; only {compared} of {} points survived, which means it \
         excluded almost everything and the loop above asserted nothing",
        (SIDE - 2) * (SIDE - 2)
    );
}

/// An unbounded green fill — the shape `RenderPhysicalModel` emits.
///
/// That render object is the mode's only production consumer, and its fill goes
/// through `Canvas::draw_paint`, which has no geometry of its own:
/// `LayerDispatcher::render_paint` expands it to the whole viewport, so its extent is
/// decided entirely by the clip.
fn fill_everything(canvas: &mut Canvas) {
    canvas.draw_paint(&Paint::fill(Color::rgb(0, 160, 0)).with_anti_alias(false));
}

/// The bounding box is an APPROXIMATION, and content it lets through is the
/// documented remainder — not an accident to be quietly narrowed later.
///
/// The clip is a triangle. `(40, 20)` lies inside the triangle's bounding box
/// but outside the triangle itself, and the fill reaches it. An exact path clip
/// would not. Pinning it here means the day a stencil pass lands, this test
/// fails and says which promise changed, rather than the gap being closed
/// silently or — worse — the approximation being mistaken for exactness by a
/// reader of the tests.
fn a_path_clip_lets_through_what_lies_inside_the_box_but_outside_the_shape() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };

    let tree = inside_a_clip(
        Clip::AntiAlias,
        |builder, behavior| {
            // A right triangle filling (8,8)-(48,48): the diagonal runs from
            // the bottom-left corner to the top-right, so the region above it
            // — where (40, 20) sits — is box-but-not-shape.
            let mut path = flui_painting::paint::Path::new();
            path.move_to(flui_foundation::geometry::Point::new(8.0, 48.0));
            path.line_to(flui_foundation::geometry::Point::new(48.0, 48.0));
            path.line_to(flui_foundation::geometry::Point::new(8.0, 8.0));
            path.close();
            builder.push_clip_path(path, behavior);
        },
        fill_everything,
    );
    let frame = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene");

    let in_box_outside_shape = sample(&frame, 40, 20);
    assert!(
        in_box_outside_shape[1] > 128,
        "the bounding-box approximation still paints inside the box but \
         outside the triangle at (40, 20), got {in_box_outside_shape:?}. If \
         this fails, path clipping became exact — update this test and \
         `WgpuPainter::clip_path`'s doc, which promises the gap"
    );
    let outside_the_box = sample(&frame, 2, 2);
    assert!(
        outside_the_box[0] > 200,
        "premise: outside the BOX the backdrop still survives, got \
         {outside_the_box:?}"
    );
}

/// An EMPTY clip path clips everything away.
///
/// `Path::compute_bounds` answers `Rect::ZERO` for a path with no commands, so
/// the scissor is zero-area and the draw is dropped. That is the same answer
/// a clip with an empty path gives — nothing visible — and it is the
/// one case where the bounding-box approximation is EXACT, since the box and
/// the shape are both empty. Asserted rather than assumed: the previous
/// behaviour was that an empty clip path clipped nothing at all, and the chain
/// that turns a zero-area scissor into a dropped draw runs through three files.
fn an_empty_clip_path_clips_everything() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };

    // Both modes, because `AntiAliasWithSaveLayer` now opens an offscreen for a
    // path clip where it never did before — so an empty path hands
    // `save_layer_clipped` a zero-area `clip_bounds()`, a state this change
    // created. Asserting only the layerless mode would leave it unexercised.
    for behavior in [Clip::AntiAlias, Clip::AntiAliasWithSaveLayer] {
        let tree = inside_a_clip(
            behavior,
            |builder, behavior| {
                builder.push_clip_path(flui_painting::paint::Path::new(), behavior);
            },
            fill_everything,
        );
        let frame = renderer
            .render_layer_tree(&tree, (SIDE, SIDE))
            .expect("the headless capture path must rasterize the scene");

        for (x, y) in [(2, 2), (32, 32), (61, 61)] {
            let pixel = sample(&frame, x, y);
            assert!(
                pixel[0] > 200 && pixel[1] < 64,
                "an empty clip path keeps nothing, so the red backdrop must \
                 survive everywhere under {behavior:?} — ({x}, {y}) got {pixel:?}"
            );
        }
    }
}

/// The clip's subtree and a sibling drawn before it, inside a blur layer.
///
/// `with_filter` decides whether the pair is wrapped; everything else is
/// identical, so the wrapped and unwrapped renders differ only by the filter
/// layer.
fn a_clip_beside_a_sibling(with_filter: bool) -> LayerTree {
    let mut builder = SceneBuilder::new();
    builder.push_offset(flui_foundation::geometry::Offset::ZERO);
    if with_filter {
        builder.push_image_filter(flui_painting::paint::ImageFilter::blur(1.0));
    }

    // A sibling FLUSHED BEFORE the clip. Opening an offscreen finalises the
    // enclosing layer's pending segment into its draw order, so this is
    // discarded alongside the clip's own subtree, not just beside it.
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 24.0, f64::from(SIDE as f32)),
        &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
    );
    builder.add_picture(canvas.finish());

    builder.push_clip_rect(
        Rect::from_xywh(32.0, 0.0, 32.0, f64::from(SIDE as f32)),
        Clip::AntiAliasWithSaveLayer,
    );
    let mut canvas = Canvas::new();
    full_surface(&mut canvas, Color::rgb(0, 0, 255));
    builder.add_picture(canvas.finish());
    builder.pop().expect("the clip is open");

    if with_filter {
        builder.pop().expect("the blur is open");
    }
    builder.build()
}

/// Inside a bounds-growing image-filter layer the mode DEGRADES; it does not
/// delete the content.
///
/// Those layers carry only their final `DrawSegment` into `FilterOp::input` and
/// discard `offscreen_items`. A `DrawItem::OpacityLayer` opened inside one is
/// therefore thrown away — and so is every sibling already flushed into the
/// enclosing layer's draw order, because opening the layer finalises the pending
/// segment first. `LayerDispatcher::opens_offscreen` declines the offscreen there and
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
fn a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
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

/// Pushes one clip shape through the canvas `_ext` API under a given `ClipOp`.
///
/// A function pointer rather than a closure so the four shapes can sit in one
/// array and the loop below reads against a single geometry.
type PushClip = fn(&mut Canvas, flui_painting::paint::ClipOp, Rect<f64>);

/// EVERY canvas clip shape refuses `ClipOp::Difference` rather than inverting.
///
/// A difference clip asks to remove the pixels INSIDE the shape — the region
/// kept is its complement. A scissor cannot express a complement, and the SDF
/// slot evaluates the shape rather than its inverse, so no clip primitive here
/// can honour the request. What the three shapes besides `path` used to do was
/// worse than refusing: they bound `_clip_op` and installed the shape as an
/// INTERSECT, so a caller asking to punch a hole got everything outside the
/// hole erased instead — the exact inverse of the request, and destructive
/// where refusing is merely permissive (issue #941).
///
/// `Canvas::clip_path_ext(&path, ClipOp::Difference, ..)` is public and
/// documented in `flui-painting`'s README as the way to punch a hole, so these
/// are reachable from outside the workspace, not just in principle.
///
/// Both arms are asserted for every shape. The `Intersect` control is what
/// makes this more than "nothing happened": it proves the same shape through
/// the same call DOES clip, so the difference arm is being refused rather than
/// silently mis-plumbed.
///
/// Read on RED: the content is blue and the cleared ground is white, so the two
/// agree on blue and an assertion there would pass either way.
fn every_canvas_clip_shape_refuses_difference_rather_than_inverting() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };

    // The same box for every shape, so the sample points read against one
    // geometry: (2, 2) is far outside it under all four.
    let box_rect = Rect::from_xywh(16.0, 16.0, 32.0, 32.0);

    let shapes: [(&str, PushClip); 4] = [
        ("rect", |canvas, op, r| {
            canvas.clip_rect_ext(r, op, Clip::AntiAlias);
        }),
        ("rrect", |canvas, op, r| {
            canvas.clip_rrect_ext(
                flui_foundation::geometry::RRect::from_rect_and_radius(
                    r,
                    flui_foundation::geometry::Radius::circular(4.0),
                ),
                op,
                Clip::AntiAlias,
            );
        }),
        ("superellipse", |canvas, op, r| {
            canvas.clip_rsuperellipse_ext(
                flui_foundation::geometry::RSuperellipse::from_rect_and_radius(
                    r,
                    flui_foundation::geometry::Radius::circular(4.0),
                ),
                op,
                Clip::AntiAlias,
            );
        }),
        ("path", |canvas, op, r| {
            let mut path = flui_painting::paint::Path::new();
            path.add_rect(r);
            canvas.clip_path_ext(&path, op, Clip::AntiAlias);
        }),
    ];

    for (name, push) in shapes {
        let scene = |op: flui_painting::paint::ClipOp| {
            let tree = {
                let mut builder = SceneBuilder::new();
                let mut canvas = Canvas::new();
                push(&mut canvas, op, box_rect);
                canvas.draw_rect(
                    Rect::from_xywh(0.0, 0.0, f64::from(SIDE as f32), f64::from(SIDE as f32)),
                    &Paint::fill(Color::rgb(0, 0, 255)),
                );
                builder.add_picture(canvas.finish());
                builder.build()
            };
            renderer
                .render_layer_tree(&tree, (SIDE, SIDE))
                .expect("the headless capture path must rasterize a canvas-clipped tree")
        };

        let intersect = sample(&scene(flui_painting::paint::ClipOp::Intersect), 2, 2);
        assert!(
            intersect[0] > 200,
            "control for {name}: an INTERSECT clip must actually clip, so (2, 2) \
             expects the white ground, got {intersect:?} — without this the \
             difference assertion below proves nothing"
        );

        let difference = sample(&scene(flui_painting::paint::ClipOp::Difference), 2, 2);
        assert!(
            difference[0] < 64,
            "a DIFFERENCE {name} clip must install nothing, so content outside \
             the shape is still painted, got {difference:?}. White here means \
             the shape was installed as an intersect and the clip inverted \
             (issue #941)"
        );
    }
}

/// Clip contract, read back from the GPU: one row per clip feature, each keeping
/// its own sample points (rect, squircle SDF, path, empty path, clip inside an
/// image-filter layer, difference refusal).
#[test]
fn clip_layers_read_back_as_the_clip_contract_specifies() {
    a_clip_rect_layer_clips_its_content_and_its_absence_does_not();
    the_squircle_sdf_agrees_with_the_cpu_path_across_the_whole_boundary();
    a_path_clip_lets_through_what_lies_inside_the_box_but_outside_the_shape();
    an_empty_clip_path_clips_everything();
    a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings();
    every_canvas_clip_shape_refuses_difference_rather_than_inverting();
}

//! Readback evidence that a clip layer clips, and that its ABSENCE does not.
//!
//! This is the engine half of the viewport's `clip_behavior` contract. The
//! other half — that `Clip::None` pushes NO clip layer while an overflowing
//! viewport under any other behaviour pushes one — belongs to the widget
//! layer, and no test there pins it; this suite knows nothing about
//! viewports.
//!
//! **The modes.** The backend used to take a clip layer's `Clip` and discard
//! it, so all three clipped modes were one picture. Now:
//!
//! - a **rounded** clip honours `HardEdge` vs `AntiAlias`;
//! - rect and curved clip chains resolve exact membership on a common sample grid;
//! - `AntiAliasWithSaveLayer` renders the clipped subtree into an offscreen and
//!   applies the clip's coverage ONCE, to the finished group. The offscreen
//!   remains active inside image filters; ordered filter input preserves the
//!   offscreen and its siblings
//!   (`a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings`).

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
/// Independent fourth-power membership gives the witness points: (2,2) is
/// outside, (7,7) inside the squircle but outside an equal-radius circle, and
/// (32,32) deep inside. All points are clear of the common 8x8 resolve fringe.
fn icon_squircle() -> flui_foundation::geometry::RSuperellipse {
    flui_foundation::geometry::RSuperellipse::from_rect_circular(
        Rect::from_xywh(0.0, 0.0, f64::from(SIDE as f32), f64::from(SIDE as f32)),
        f64::from(SIDE as f32 / 2.0),
    )
}

/// The GPU analytic membership in `shaders/clip_mask.wgsl` and the independent
/// CPU parametric path agree away from the boundary. Neighbour-disagreement
/// points are excluded because finite sample coverage is fractional there.
/// The historical function name is preserved; the production evaluator is
/// Boolean membership followed by one common-grid resolve, not the old SDF.
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

/// Exact triangle membership rejects a point inside its bounding box.
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
    assert_eq!(
        in_box_outside_shape,
        [255, 0, 0, 255],
        "exact triangle rejects bounding-box-only interior"
    );
    assert_eq!(sample(&frame, 20, 40), [0, 160, 0, 255]);
    let outside_the_box = sample(&frame, 2, 2);
    assert!(
        outside_the_box[0] > 200,
        "premise: outside the BOX the backdrop still survives, got \
         {outside_the_box:?}"
    );
}

/// Empty path membership is empty under Intersect, including grouped clipping.
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

    // The sibling is flushed into ordered filter input before the clip group.
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 24.0, f64::from(SIDE as f32)),
        &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
    );
    builder.add_picture(canvas.finish());

    builder.push_clip_rrect(
        flui_foundation::geometry::RRect::from_rect_circular(
            Rect::from_xywh(32.0, 0.0, 32.0, f64::from(SIDE as f32)),
            8.0,
        ),
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

/// A filter preserves the rounded clip group and the sibling flushed before it.
/// Interior samples pin content preservation; the red sample also catches blue
/// leaking past the clip. These samples do not pin fractional edge coverage.
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

/// Difference retains the exterior and removes the interior for every shape.
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

        let difference_frame = scene(flui_painting::paint::ClipOp::Difference);
        assert_eq!(
            sample(&difference_frame, 32, 32),
            [255, 255, 255, 255],
            "difference removes interior for {name}"
        );
        let difference = sample(&difference_frame, 2, 2);
        assert!(
            difference[0] < 64,
            "difference retains the exterior for {name}: {difference:?}"
        );
    }
}

/// Clip contract, read back from the GPU: one row per clip feature, each keeping
/// its own sample points (rect, squircle membership, path, empty path, clip inside an
/// image-filter layer, difference complement).
#[test]
fn clip_layers_read_back_as_the_clip_contract_specifies() {
    #[cfg(feature = "testing")]
    lazy_pipeline_admission_recovers();
    clip_failures_and_singular_membership_recover();
    grouped_clip_prefix_and_destructive_coverage();
    path_clip_fill_rules_and_implicit_close();
    transformed_nested_clip_on_antialiased_path();
    nested_exact_clip_geometry_and_coverage();
    hard_clip_membership_scales_to_a_full_hd_frame();
    command_transform_changes_preserve_captured_clips();
    display_list_and_save_layer_scopes_own_their_clips();
    a_clip_rect_layer_clips_its_content_and_its_absence_does_not();
    the_squircle_sdf_agrees_with_the_cpu_path_across_the_whole_boundary();
    a_path_clip_lets_through_what_lies_inside_the_box_but_outside_the_shape();
    an_empty_clip_path_clips_everything();
    a_clip_inside_an_image_filter_layer_keeps_its_content_and_its_siblings();
    every_canvas_clip_shape_refuses_difference_rather_than_inverting();
}

fn display_list_and_save_layer_scopes_own_their_clips() {
    use flui_foundation::geometry::Offset;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    for layer in [false, true] {
        let mut canvas = Canvas::new();
        if layer {
            canvas.save_layer(None, &Paint::fill(Color::rgba(255, 255, 255, 128)));
        }
        canvas.translate(10.0, 10.0);
        canvas.clip_rect(Rect::from_xywh(0.0, 0.0, 20.0, 20.0));
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 20.0, 20.0),
            &Paint::fill(Color::RED),
        );
        if layer {
            canvas.restore();
            canvas.draw_rect(
                Rect::from_xywh(0.0, 40.0, 8.0, 8.0),
                &Paint::fill(Color::BLUE),
            );
        }
        let mut builder = SceneBuilder::new();
        builder.push_offset(Offset::new(2.0, 3.0));
        builder.add_picture(canvas.finish());
        let mut sibling = Canvas::new();
        sibling.draw_rect(
            Rect::from_xywh(40.0, 40.0, 8.0, 8.0),
            &Paint::fill(Color::GREEN),
        );
        builder.add_picture(sibling.finish());
        let pixels = renderer
            .render_layer_tree(&builder.build(), (SIDE, SIDE))
            .expect("isolated display lists");
        assert_eq!(
            sample(&pixels, 46, 47),
            [0, 255, 0, 255],
            "sibling picture does not inherit clip or command CTM (layer={layer})"
        );
        if layer {
            assert_eq!(
                sample(&pixels, 6, 47),
                [0, 0, 255, 255],
                "SaveLayer restore removes child clip"
            );
            let actual = sample(&pixels, 18, 19);
            for (&actual, expected) in actual.iter().zip([255_u8, 127, 127, 255]) {
                assert!(
                    actual.abs_diff(expected) <= 2,
                    "group opacity remains applied once"
                );
            }
        } else {
            assert_eq!(sample(&pixels, 18, 19), [255, 0, 0, 255]);
        }
    }
}

fn command_transform_changes_preserve_captured_clips() {
    use flui_foundation::geometry::{Matrix4, Offset, RRect, RSuperellipse, Radius};
    use flui_painting::paint::Path;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    type PushClip = fn(&mut Canvas, Rect<f64>);
    let shapes: [(&str, PushClip); 4] = [
        ("rect", |canvas, rect| {
            canvas.clip_rect_ext(
                rect,
                flui_painting::paint::ClipOp::Intersect,
                Clip::HardEdge,
            );
        }),
        ("rrect", |canvas, rect| {
            canvas.clip_rrect(RRect::from_rect_and_radius(rect, Radius::circular(4.0)));
        }),
        ("superellipse", |canvas, rect| {
            canvas.clip_rsuperellipse(RSuperellipse::from_rect_and_radius(
                rect,
                Radius::circular(4.0),
            ));
        }),
        ("rectangular path", |canvas, rect| {
            let mut path = Path::new();
            path.add_rect(rect);
            canvas.clip_path(&path);
        }),
    ];
    for (name, push_clip) in shapes {
        for identity in [false, true] {
            let mut canvas = Canvas::new();
            canvas.save();
            canvas.translate(10.0, 10.0);
            push_clip(&mut canvas, Rect::from_xywh(0.0, 0.0, 20.0, 20.0));
            // Same-transform recording must keep the original clip as well.
            canvas.draw_rect(
                Rect::from_xywh(6.0, 6.0, 4.0, 4.0),
                &Paint::fill(Color::RED),
            );
            if identity {
                canvas.set_transform(Matrix4::IDENTITY);
                canvas.draw_rect(
                    Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                    &Paint::fill(Color::RED),
                );
            } else {
                canvas.translate(40.0, 0.0);
                canvas.draw_rect(
                    Rect::from_xywh(0.0, 0.0, 12.0, 12.0),
                    &Paint::fill(Color::RED),
                );
            }
            canvas.restore();
            canvas.draw_rect(
                Rect::from_xywh(0.0, 40.0, 8.0, 8.0),
                &Paint::fill(Color::BLUE),
            );
            let mut builder = SceneBuilder::new();
            builder.push_offset(Offset::new(2.0, 3.0));
            builder.add_picture(canvas.finish());
            let pixels = renderer
                .render_layer_tree(&builder.build(), (SIDE, SIDE))
                .expect("transformed clip capture");
            assert_eq!(
                sample(&pixels, 18, 19),
                [255, 0, 0, 255],
                "{name}: same-transform content"
            );
            let (x, y) = if identity { (6, 7) } else { (56, 17) };
            assert_eq!(
                sample(&pixels, x, y),
                [255, 255, 255, 255],
                "{name}: clip must survive command transform change (identity={identity})"
            );
            assert_eq!(
                sample(&pixels, 6, 47),
                [0, 0, 255, 255],
                "{name}: explicit restore removes the clip and preserves the parent offset"
            );
        }
    }
}

/// Pins immutable nested membership and one AA resolve through consumer recording.
fn nested_exact_clip_geometry_and_coverage() {
    use flui_foundation::geometry::RRect;
    use flui_painting::paint::ClipOp;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let capture = |draw: &dyn Fn(&mut Canvas)| {
        let mut canvas = Canvas::new();
        draw(&mut canvas);
        full_surface(&mut canvas, Color::BLUE);
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        renderer
            .render_layer_tree(&builder.build(), (SIDE, SIDE))
            .expect("exact canvas clip capture")
    };
    let nested = capture(&|canvas| {
        for radius in [16.0, 2.0] {
            canvas.clip_rrect_ext(
                RRect::from_rect_circular(Rect::from_xywh(8.0, 8.0, 48.0, 48.0), radius),
                ClipOp::Intersect,
                Clip::HardEdge,
            );
        }
    });
    assert_eq!(
        sample(&nested, 10, 10),
        [255, 255, 255, 255],
        "outer rounded corner survives inner clip"
    );
    assert_eq!(sample(&nested, 32, 32), [0, 0, 255, 255]);
    for repetitions in [1, 2] {
        let frame = capture(&|canvas| {
            for _ in 0..repetitions {
                canvas.clip_rrect_ext(
                    RRect::from_rect_circular(Rect::from_xywh(8.25, 8.0, 48.0, 48.0), 8.0),
                    ClipOp::Intersect,
                    Clip::AntiAlias,
                );
            }
        });
        for (actual, expected) in sample(&frame, 8, 32).into_iter().zip([64_u8, 64, 255, 255]) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "AA repetition {repetitions} preserves 48/64 coverage"
            );
        }
    }
    let excluded = capture(&|canvas| {
        for operation in [ClipOp::Intersect, ClipOp::Difference] {
            canvas.clip_rrect_ext(
                RRect::from_rect_circular(Rect::from_xywh(8.25, 8.0, 48.0, 48.0), 8.0),
                operation,
                Clip::AntiAlias,
            );
        }
    });
    for point in [(8, 32), (32, 32)] {
        assert_eq!(
            sample(&excluded, point.0, point.1),
            [255, 255, 255, 255],
            "C minus C is empty even at a partially covered edge"
        );
    }
    let elliptical = capture(&|canvas| {
        canvas.clip_rrect_ext(
            RRect::from_rect_elliptical(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), 32.0, 8.0),
            ClipOp::Intersect,
            Clip::HardEdge,
        );
    });
    assert_eq!(
        sample(&elliptical, 5, 5),
        [0, 0, 255, 255],
        "elliptical rx32 ry8 differs from radius32"
    );
    assert_eq!(sample(&elliptical, 1, 1), [255, 255, 255, 255]);
    let subpixel = capture(&|canvas| {
        canvas.clip_rect_ext(
            Rect::from_xywh(8.25, 0.0, 48.0, 64.0),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
    });
    for (actual, expected) in sample(&subpixel, 8, 32)
        .into_iter()
        .zip([64_u8, 64, 255, 255])
    {
        assert!(
            actual.abs_diff(expected) <= 1,
            "AA rect resolves 48 of64 membership samples"
        );
    }
}

fn canvas_clip_tree(record: impl FnOnce(&mut Canvas)) -> LayerTree {
    let mut canvas = Canvas::new();
    record(&mut canvas);
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    builder.build()
}

fn clip_failures_and_singular_membership_recover() {
    use flui_foundation::geometry::{Matrix4, RRect};
    use flui_painting::paint::ClipOp;
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    type InvalidClip = fn(&mut Canvas);
    let invalid: [InvalidClip; 5] = [
        |c| c.clip_rect(Rect::from_xywh(f64::NAN, 0.0, 32.0, 32.0)),
        |c| c.clip_rect(Rect::from_xywh(0.0, f64::INFINITY, 32.0, 32.0)),
        |c| c.clip_rect(Rect::from_xywh(8.0, 8.0, -4.0, 32.0)),
        |c| {
            c.clip_rrect(RRect::from_rect_elliptical(
                Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                -2.0,
                4.0,
            ));
        },
        |c| {
            c.clip_rrect(RRect::from_rect_elliptical(
                Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                f64::NAN,
                4.0,
            ));
        },
    ];
    for clip in invalid {
        let tree = canvas_clip_tree(|c| {
            clip(c);
            full_surface(c, Color::BLUE);
        });
        assert!(
            matches!(
                renderer.render_layer_tree(&tree, (SIDE, SIDE)),
                Err(crate::EngineError::InvalidGeometry(_))
            ),
            "invalid clip returns typed geometry refusal"
        );
        let frame = renderer
            .render_layer_tree(
                &canvas_clip_tree(|c| full_surface(c, Color::GREEN)),
                (SIDE, SIDE),
            )
            .expect("valid frame after geometry refusal");
        assert_eq!(sample(&frame, 32, 32), [0, 255, 0, 255]);
    }
    let depth = canvas_clip_tree(|c| {
        for _ in 0..65 {
            c.clip_rrect(RRect::from_rect_circular(
                Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
                8.0,
            ));
        }
        full_surface(c, Color::BLUE);
    });
    assert!(
        matches!(
            renderer.render_layer_tree(&depth, (SIDE, SIDE)),
            Err(crate::EngineError::PreparedResourceLimit { .. })
        ),
        "depth admission is a typed refusal"
    );
    let next = renderer
        .render_layer_tree(
            &canvas_clip_tree(|c| full_surface(c, Color::GREEN)),
            (SIDE, SIDE),
        )
        .expect("valid frame after depth refusal");
    assert_eq!(sample(&next, 32, 32), [0, 255, 0, 255]);
    for (op, expected) in [
        (ClipOp::Intersect, [255, 255, 255, 255]),
        (ClipOp::Difference, [0, 0, 255, 255]),
    ] {
        let tree = canvas_clip_tree(|c| {
            c.scale(0.0, 1.0);
            c.clip_rect_ext(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), op, Clip::AntiAlias);
            c.set_transform(Matrix4::IDENTITY);
            full_surface(c, Color::BLUE);
        });
        let frame = renderer
            .render_layer_tree(&tree, (SIDE, SIDE))
            .expect("singular clip has defined membership");
        assert_eq!(sample(&frame, 32, 32), expected);
    }
}

fn grouped_clip_prefix_and_destructive_coverage() {
    use flui_painting::{BlendMode, paint::ClipOp};
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    for grouped in [false, true] {
        for (mode, source, expected) in [
            (BlendMode::Clear, Color::BLUE, [64_u8, 0, 0, 64]),
            (BlendMode::Src, Color::BLUE, [64, 0, 191, 255]),
            (
                BlendMode::DstIn,
                Color::rgba(0, 0, 255, 128),
                [160, 0, 0, 160],
            ),
        ] {
            let tree = canvas_clip_tree(|c| {
                full_surface(c, Color::RED);
                c.clip_rect_ext(
                    Rect::from_xywh(8.25, 0.0, 48.0, 64.0),
                    ClipOp::Intersect,
                    Clip::AntiAlias,
                );
                if grouped {
                    c.save_layer(None, &Paint::fill(Color::WHITE).with_blend_mode(mode));
                    full_surface(c, source);
                    c.restore();
                } else {
                    c.draw_rect(
                        Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
                        &Paint::fill(source)
                            .with_anti_alias(false)
                            .with_blend_mode(mode),
                    );
                }
            });
            let frame = renderer
                .render_layer_tree(&tree, (SIDE, SIDE))
                .expect("destructive clip coverage");
            let pixel = sample(&frame, 8, 32);
            for (actual, expected) in pixel.into_iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "operator {mode:?}, grouped={grouped}, coverage mixes with destination: pixel={pixel:?}, expected channel={expected}"
                );
            }
            assert_eq!(
                sample(&frame, 7, 32),
                [255, 0, 0, 255],
                "outside clip remains red"
            );
        }
    }
    let tree = canvas_clip_tree(|c| {
        c.clip_rect_ext(
            Rect::from_xywh(8.25, 0.0, 48.0, 64.0),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        c.save_layer(None, &Paint::fill(Color::rgba(255, 255, 255, 128)));
        c.save_layer(None, &Paint::fill(Color::WHITE));
        full_surface(c, Color::BLUE);
        c.restore();
        c.restore();
    });
    let frame = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("nested group prefix coverage");
    for (actual, expected) in sample(&frame, 8, 32)
        .into_iter()
        .zip([159_u8, 159, 255, 255])
    {
        assert!(
            actual.abs_diff(expected) <= 1,
            "inherited AA prefix and group opacity each apply once"
        );
    }
}

/// Exercises the antialiased path route's cropped/scaled attachment with a
/// captured rotated clip and a later root-coordinate nested clip.
fn transformed_nested_clip_on_antialiased_path() {
    use flui_foundation::geometry::{Matrix4, RRect};
    use flui_painting::paint::{ClipOp, Path};
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let tree = canvas_clip_tree(|c| {
        c.translate(32.0, 32.0);
        c.rotate(std::f64::consts::FRAC_PI_4);
        c.clip_rrect_ext(
            RRect::from_rect_circular(Rect::from_xywh(-16.0, -16.0, 32.0, 32.0), 4.0),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        c.set_transform(Matrix4::IDENTITY);
        c.clip_rrect_ext(
            RRect::from_rect_circular(Rect::from_xywh(10.0, 10.0, 44.0, 44.0), 2.0),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        let mut path = Path::new();
        path.add_rect(Rect::from_xywh(4.0, 4.0, 56.0, 56.0));
        c.draw_path(&path, &Paint::fill(Color::BLUE).with_anti_alias(true));
    });
    let frame = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("transformed path clip capture");
    assert_eq!(
        sample(&frame, 12, 12),
        [255, 255, 255, 255],
        "rotated ancestor rejects nested bounding-box corner"
    );
    assert_eq!(
        sample(&frame, 32, 32),
        [0, 0, 255, 255],
        "cropped path attachment maps interior to root"
    );
    assert_eq!(
        sample(&frame, 32, 18),
        [0, 0, 255, 255],
        "noncentral interior survives attachment rebase"
    );
    assert_eq!(sample(&frame, 5, 32), [255, 255, 255, 255]);
}

/// Same-direction contours distinguish winding from parity; fill membership
/// also closes an otherwise open contour without requiring a close command.
fn path_clip_fill_rules_and_implicit_close() {
    use flui_foundation::geometry::Point;
    use flui_painting::paint::{ClipOp, Path, PathFillType};
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    for fill_type in [PathFillType::NonZero, PathFillType::EvenOdd] {
        let tree = canvas_clip_tree(|c| {
            let mut path = Path::with_fill_type(fill_type);
            path.add_rect(Rect::from_xywh(8.0, 8.0, 48.0, 48.0));
            path.add_rect(Rect::from_xywh(24.0, 24.0, 16.0, 16.0));
            c.clip_path_ext(&path, ClipOp::Intersect, Clip::HardEdge);
            full_surface(c, Color::BLUE);
        });
        let frame = renderer
            .render_layer_tree(&tree, (SIDE, SIDE))
            .expect("path clip fill rule capture");
        assert_eq!(
            sample(&frame, 16, 32),
            [0, 0, 255, 255],
            "ring is filled under both rules"
        );
        let expected = match fill_type {
            PathFillType::NonZero => [0, 0, 255, 255],
            PathFillType::EvenOdd => [255, 255, 255, 255],
        };
        assert_eq!(
            sample(&frame, 32, 32),
            expected,
            "nested same-direction contours use {fill_type:?}"
        );
        assert_eq!(sample(&frame, 2, 32), [255, 255, 255, 255]);
    }
    let tree = canvas_clip_tree(|c| {
        let mut path = Path::new();
        path.move_to(Point::new(8.0, 8.0));
        path.line_to(Point::new(56.0, 8.0));
        path.line_to(Point::new(8.0, 56.0));
        c.clip_path_ext(&path, ClipOp::Intersect, Clip::HardEdge);
        full_surface(c, Color::BLUE);
    });
    let frame = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("open contour fills with implicit close");
    assert_eq!(sample(&frame, 16, 16), [0, 0, 255, 255]);
    assert_eq!(
        sample(&frame, 48, 48),
        [255, 255, 255, 255],
        "outside triangle but inside bounding box stays unpainted"
    );
}

/// A routine full-HD hard-clipped frame fits the bounded work allowance.
/// Evaluating the same pixel-centre membership 64 times needlessly refuses it.
fn hard_clip_membership_scales_to_a_full_hd_frame() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let size = (1920, 1080);
    let full = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
    let tree = canvas_clip_tree(|canvas| {
        for _ in 0..8 {
            canvas.clip_rect_ext(
                full,
                flui_painting::paint::ClipOp::Intersect,
                Clip::HardEdge,
            );
        }
        canvas.draw_rect(full, &Paint::fill(Color::BLUE).with_anti_alias(false));
    });
    let frame = renderer
        .render_layer_tree(&tree, size)
        .expect("full-HD hard membership is admitted without supersample work");
    for (x, y) in [(0, 0), (960, 540), (1919, 1079)] {
        let offset = ((y * size.0 + x) * 4) as usize;
        assert_eq!(&frame[offset..offset + 4], &[0, 0, 255, 255]);
    }
}

// A private quota seam exercises failure before driver object creation; the
// public painter intentionally does not expose device-domain limits.
#[cfg(feature = "testing")]
fn lazy_pipeline_admission_recovers() {
    use crate::clip_mask::{ClipMaskPipeline, MaskMapping, MaskNode};
    use crate::device_domain::{DeviceDomain, PreparedCost, PreparedIrLimits};
    use crate::{EngineError, resources::GpuResources};
    use std::sync::Arc;

    let Some((device, queue)) = crate::test_support::try_test_device_and_queue("clip admission")
    else {
        return;
    };
    for shape in [0, 1, 3] {
        let domain = DeviceDomain::with_limits(
            Arc::clone(&device),
            Arc::clone(&queue),
            PreparedIrLimits {
                cost: PreparedCost {
                    objects: 7,
                    ..PreparedIrLimits::default().cost
                },
                ..Default::default()
            },
        );
        let mut pipeline = ClipMaskPipeline::new(&device);
        let nodes = [MaskNode {
            bounds: [0.0, 0.0, 4.0, 4.0],
            radii_x: [1.0; 4],
            radii_y: [1.0; 4],
            inverse: [1.0, 0.0, 0.0, 1.0],
            translation: [0.0; 4],
            meta: [shape, 0, 1, 0],
            path_range: [0; 4],
        }];
        let mapping = MaskMapping {
            attachment_to_root: [1.0, 0.0, 0.0, 1.0],
            translation: [0.0; 4],
            extent_counts: [4, 4, 0, 0],
        };
        let prepare = |pipeline: &mut ClipMaskPipeline,
                       resources: &mut GpuResources,
                       encoder: &mut wgpu::CommandEncoder| {
            pipeline.prepare(
                &device,
                resources,
                encoder,
                &nodes,
                &[],
                mapping,
                usize::MAX,
            )
        };
        // Leave room for the mask but not its first membership pipeline.
        // Also exercise refusal of the mask allocation itself on the same owner.
        for occupied in [1, 2] {
            let blocker = domain
                .reserve(PreparedCost {
                    objects: occupied,
                    ..Default::default()
                })
                .expect("competing allocation fits");
            let mut resources = GpuResources::new(Arc::clone(&domain));
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            assert!(
                matches!(
                    prepare(&mut pipeline, &mut resources, &mut encoder),
                    Err(EngineError::PreparedResourceLimit { .. })
                ),
                "shape {shape} must reject preparation under competing occupancy {occupied}"
            );
            drop(encoder);
            drop(resources);
            drop(blocker);
        }
        // Recovery on the very same pipeline/domain must release failed work
        // and leave a refused specialization available for first use.
        let mut resources = GpuResources::new(Arc::clone(&domain));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mask = prepare(&mut pipeline, &mut resources, &mut encoder)
            .expect("valid clip prepares after failed admission");
        let submission = queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .expect("recovered clip completes");
        drop(mask);
        drop(resources);
        // A materialized pipeline requires no second creation charge.
        let blocker = domain
            .reserve(PreparedCost {
                objects: 1,
                ..Default::default()
            })
            .expect("competing allocation fits after completion");
        let mut resources = GpuResources::new(Arc::clone(&domain));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        prepare(&mut pipeline, &mut resources, &mut encoder)
            .expect("repeat use fits without another pipeline allocation");
        drop(encoder);
        drop(resources);
        drop(blocker);
    }
}

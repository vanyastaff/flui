//! Partial frames on the GPU: the damage a `LayerDiffer` produces, rendered
//! through the retained target, read back.
//!
//! Every test drives [`RetainedCapture`](crate::headless::RetainedCapture),
//! which runs each frame through the windowed renderer's own
//! `FrameProtocol` (plan, target selection, clear, commit, the unmanaged
//! frame's bookkeeping) and `record_frame_content`. The sample points are chosen so
//! a broken implementation fails: a pixel outside the damage that a full
//! repaint would overwrite, a pixel the old position left behind, a pixel
//! whose colour depends on whether the partial clear ran first.

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_layer::{
    BackdropFilterLayer, ContentToken, DamageRegion, Layer, LayerDiffer, LayerNode, LayerTree,
    OffsetLayer, PictureLayer, Scene, TransformLayer,
};
use flui_painting::paint::{BlendMode, ImageFilter, Path};
use flui_painting::styling::Color;
use flui_painting::typography::{TextDirection, TextSpan, TextStyle};
use flui_painting::{Canvas, Paint, TextPainter};

use crate::damage::FramePlan;
use crate::headless::RetainedCapture;
use crate::raster::RasterBackend;

const SIDE: u32 = 128;

fn id(raw: usize) -> RenderId {
    RenderId::new(raw)
}

/// One boundary of a test scene: a stamped `OffsetLayer` at `at` holding the
/// picture `paint` records at its origin.
struct Boundary<'a> {
    id: usize,
    token: &'a ContentToken,
    at: Offset<f64>,
    paint: &'a dyn Fn(&mut Canvas),
}

/// A root stamped as the paint pass stamps a `RenderView`, over the given
/// boundaries and, when set, a backdrop filter drawn after them.
fn scene(root: &ContentToken, boundaries: &[Boundary<'_>], backdrop: Option<Rect<f64>>) -> Scene {
    let mut tree = LayerTree::new(
        LayerNode::new(Layer::from(TransformLayer::new(Matrix4::IDENTITY)))
            .with_boundary(id(1), root.clone()),
    );
    let root_id = tree.root();
    for boundary in boundaries {
        let layer = tree.push_child(
            root_id,
            LayerNode::new(Layer::from(OffsetLayer::new(boundary.at)))
                .with_boundary(id(boundary.id), boundary.token.clone()),
        );
        let mut canvas = Canvas::new();
        (boundary.paint)(&mut canvas);
        tree.push_child(layer, Layer::from(PictureLayer::new(canvas.finish())));
    }
    if let Some(bounds) = backdrop {
        tree.push_child(
            root_id,
            Layer::from(BackdropFilterLayer::new(
                ImageFilter::blur(3.0),
                BlendMode::SrcOver,
                bounds,
            )),
        );
    }
    Scene::new(tree)
}

/// Hands `region` to the capture the way `RasterOwner::pump` hands it to a
/// backend.
fn apply(capture: &mut RetainedCapture, region: DamageRegion) {
    match region {
        DamageRegion::Partial(rect) => capture.mark_dirty(rect.to_rect()),
        DamageRegion::Unchanged => {}
        _ => capture.mark_full_repaint(),
    }
}

/// Renders `scene` after applying `region`, asserting the plan it ran.
fn frame(
    capture: &mut RetainedCapture,
    scene: &Scene,
    region: DamageRegion,
    plan: fn(FramePlan) -> bool,
) {
    apply(capture, region);
    capture.render_scene(scene).expect("the frame renders");
    let ran = capture.last_plan();
    assert!(
        ran.is_some_and(plan),
        "unexpected plan {ran:?} for {region:?}"
    );
}

/// Re-renders `scene` with a one-pixel damage, which a capture whose target
/// is not valid renders in full into the target: the warm-up every partial
/// frame after a direct one pays.
fn warm(capture: &mut RetainedCapture, scene: &Scene) {
    capture.mark_dirty(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0));
    capture.render_scene(scene).expect("the warm-up renders");
    assert_eq!(capture.last_plan(), Some(FramePlan::RetainedFull));
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIDE + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn near(actual: [u8; 4], expected: [u8; 4], tolerance: u8) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(&a, e)| a.abs_diff(e) <= tolerance)
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

fn square(color: Color) -> impl Fn(&mut Canvas) {
    move |canvas: &mut Canvas| {
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 16.0, 16.0), &Paint::fill(color));
    }
}

/// A box moves from A to A′ with another box B standing still, and a
/// sentinel painted into the retained target at C, outside the damage,
/// stands for the previous frame's pixels there.
///
/// - Old A reads white: damage computed from the new bounds only leaves the
///   box's last frame there.
/// - A′ reads red.
/// - C still reads the sentinel: a full repaint, or a scissor that is
///   ignored, overwrites it.
/// - B still reads blue.
#[test]
fn a_moved_box_repaints_its_old_and_new_positions_only() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    let (root, moving, still) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let red = square(Color::RED);
    let blue = square(Color::BLUE);
    let build = |at: Offset<f64>| {
        scene(
            &root,
            &[
                Boundary {
                    id: 2,
                    token: &moving,
                    at,
                    paint: &red,
                },
                Boundary {
                    id: 3,
                    token: &still,
                    at: Offset::new(80.0, 80.0),
                    paint: &blue,
                },
            ],
            None,
        )
    };
    let (a, a_moved) = (Offset::new(8.0, 8.0), Offset::new(8.0, 48.0));
    let mut differ = LayerDiffer::default();

    let first = build(a);
    let region = differ.diff(&first, (SIDE, SIDE));
    frame(&mut capture, &first, region, |plan| {
        plan == FramePlan::Direct
    });
    warm(&mut capture, &first);

    let sentinel = (80, 8, 8, 8);
    capture.paint_retained(sentinel, GREEN);

    let second = build(a_moved);
    let region = differ.diff(&second, (SIDE, SIDE));
    assert!(matches!(region, DamageRegion::Partial(_)), "{region:?}");
    frame(&mut capture, &second, region, |plan| {
        matches!(plan, FramePlan::RetainedPartial(_))
    });

    let pixels = capture.read_rgba().expect("readback");
    assert_eq!(
        px(&pixels, 16, 16),
        WHITE,
        "the box's old position is cleared"
    );
    assert_eq!(
        px(&pixels, 16, 56),
        RED,
        "the box's new position is painted"
    );
    assert_eq!(
        px(&pixels, 84, 12),
        GREEN,
        "pixels outside the damage are the previous frame's, untouched"
    );
    assert_eq!(px(&pixels, 88, 88), BLUE, "the still box is untouched");
}

/// Content inside the damage blends over the background, not over what the
/// previous frame left there: 50% blue over the partial clear reads light
/// blue, where 50% blue over the old opaque red would read purple.
#[test]
fn the_partial_clear_runs_before_content() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    let root = ContentToken::mint();
    let red = square(Color::RED);
    let translucent_blue = square(Color::rgba(0, 0, 255, 128));
    let build = |token: &ContentToken, paint: &dyn Fn(&mut Canvas)| {
        scene(
            &root,
            &[Boundary {
                id: 2,
                token,
                at: Offset::new(40.0, 40.0),
                paint,
            }],
            None,
        )
    };
    let mut differ = LayerDiffer::default();
    let first = build(&ContentToken::mint(), &red);
    let region = differ.diff(&first, (SIDE, SIDE));
    frame(&mut capture, &first, region, |plan| {
        plan == FramePlan::Direct
    });
    warm(&mut capture, &first);

    let second = build(&ContentToken::mint(), &translucent_blue);
    let region = differ.diff(&second, (SIDE, SIDE));
    frame(&mut capture, &second, region, |plan| {
        matches!(plan, FramePlan::RetainedPartial(_))
    });

    let pixels = capture.read_rgba().expect("readback");
    let center = px(&pixels, 48, 48);
    assert!(
        near(center, [127, 127, 255, 255], 3),
        "50% blue over the cleared background, got {center:?} (purple means the \
         partial frame blended over the previous frame's red)"
    );
}

/// A retained target that is not known to hold the last frame is never
/// trusted: a partial frame after a resize, or after a frame that failed
/// between beginning the target and submitting, renders in full, so the
/// sentinel standing for stale pixels is overwritten.
#[test]
fn an_invalid_target_promotes_to_full() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let root = ContentToken::mint();
    let token = ContentToken::mint();
    let red = square(Color::RED);
    let still = scene(
        &root,
        &[Boundary {
            id: 2,
            token: &token,
            at: Offset::new(8.0, 8.0),
            paint: &red,
        }],
        None,
    );
    let sentinel = (80, 80, 8, 8);
    let small = Rect::from_ltrb(0.0, 0.0, 2.0, 2.0);

    // A frame that fails after beginning the target.
    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    capture.mark_full_repaint();
    capture.render_scene(&still).expect("first frame");
    warm(&mut capture, &still);
    capture.paint_retained(sentinel, GREEN);
    capture.fail_next_frame_after_begin();
    capture.mark_dirty(small);
    assert!(
        capture.render_scene(&still).is_err(),
        "the injected failure"
    );
    capture.mark_dirty(small);
    capture.render_scene(&still).expect("the retry renders");
    assert_eq!(
        capture.last_plan(),
        Some(FramePlan::RetainedFull),
        "a target begun by a failed frame is not valid"
    );
    let pixels = capture.read_rgba().expect("readback");
    assert_eq!(
        px(&pixels, 84, 84),
        WHITE,
        "the stale sentinel is repainted"
    );

    // A resize.
    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    capture.mark_full_repaint();
    capture.render_scene(&still).expect("first frame");
    warm(&mut capture, &still);
    capture.paint_retained(sentinel, GREEN);
    capture.resize(SIDE, SIDE);
    capture
        .render_scene(&still)
        .expect("the frame after the resize");
    assert_eq!(capture.last_plan(), Some(FramePlan::Direct));
    capture.mark_dirty(small);
    capture
        .render_scene(&still)
        .expect("the next partial frame");
    assert_eq!(
        capture.last_plan(),
        Some(FramePlan::RetainedFull),
        "the target missed the direct frame, so the partial one renders in full"
    );
    let pixels = capture.read_rgba().expect("readback");
    assert_eq!(
        px(&pixels, 84, 84),
        WHITE,
        "the stale sentinel is repainted"
    );

    // A full frame rendered straight into the surface, with no resize: the
    // target did not see it, so it no longer holds the last frame.
    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    capture.mark_full_repaint();
    capture.render_scene(&still).expect("first frame");
    warm(&mut capture, &still);
    capture.paint_retained(sentinel, GREEN);
    capture.mark_full_repaint();
    capture.render_scene(&still).expect("the direct frame");
    assert_eq!(capture.last_plan(), Some(FramePlan::Direct));
    capture.mark_dirty(small);
    capture
        .render_scene(&still)
        .expect("the next partial frame");
    assert_eq!(
        capture.last_plan(),
        Some(FramePlan::RetainedFull),
        "a direct frame invalidates the target"
    );
    let pixels = capture.read_rgba().expect("readback");
    assert_eq!(
        px(&pixels, 84, 84),
        WHITE,
        "the stale sentinel is repainted"
    );
}

/// A frame rendered outside the damage protocol (a hot-reload plugin's
/// scene through `Renderer::render_scene`) is followed by a full frame, on
/// a surface that renders every frame through the retained target: the
/// producer diffs against the last scene IT submitted, which is no longer
/// what the target and the screen hold.
///
/// Both a partial region and an unchanged one must repaint everything; a
/// pixel far from the damage reads the app's background, not the plugin's
/// green.
#[test]
fn a_frame_outside_the_protocol_is_followed_by_a_full_one() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, card, plugin_root, plugin) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let red = square(Color::RED);
    let flood = |canvas: &mut Canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 128.0, 128.0),
            &Paint::fill(Color::rgba(0, 255, 0, 255)),
        );
    };
    let app = |at: Offset<f64>| {
        scene(
            &root,
            &[Boundary {
                id: 2,
                token: &card,
                at,
                paint: &red,
            }],
            None,
        )
    };
    let plugin_scene = scene(
        &plugin_root,
        &[Boundary {
            id: 2,
            token: &plugin,
            at: Offset::ZERO,
            paint: &flood,
        }],
        None,
    );

    for resume_with_change in [true, false] {
        let mut capture = renderer
            .retained_capture((SIDE, SIDE))
            .expect("capture target");
        capture.require_intermediate();
        let mut differ = LayerDiffer::default();
        let first = app(Offset::new(8.0, 8.0));
        let region = differ.diff(&first, (SIDE, SIDE));
        frame(&mut capture, &first, region, |plan| {
            plan == FramePlan::RetainedFull
        });

        capture
            .render_unmanaged(&plugin_scene)
            .expect("the plugin frame");
        assert_eq!(capture.last_plan(), Some(FramePlan::RetainedFull));
        assert_eq!(
            px(&capture.read_rgba().expect("readback"), 100, 100),
            GREEN,
            "precondition: the plugin frame is on screen"
        );

        let resumed = if resume_with_change {
            app(Offset::new(8.0, 12.0))
        } else {
            app(Offset::new(8.0, 8.0))
        };
        let region = differ.diff(&resumed, (SIDE, SIDE));
        assert_eq!(
            matches!(region, DamageRegion::Partial(_)),
            resume_with_change,
            "precondition: the producer sees only its own scenes: {region:?}"
        );
        frame(&mut capture, &resumed, region, |plan| {
            plan == FramePlan::RetainedFull
        });
        let pixels = capture.read_rgba().expect("readback");
        assert_eq!(
            px(&pixels, 100, 100),
            WHITE,
            "the plugin's pixels are gone (resumed with a change: {resume_with_change})"
        );
        assert_eq!(px(&pixels, 14, 18), RED, "the app's box is painted");
    }
}

/// Renders `after` in full on a fresh capture and returns its pixels.
fn full_frame_pixels(renderer: &crate::headless::HeadlessRenderer, after: &Scene) -> Vec<u8> {
    let mut full = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    full.mark_full_repaint();
    full.render_scene(after).expect("full frame");
    full.read_rgba().expect("readback")
}

/// The first few pixels where `partial` and `full` differ by more than
/// `tolerance` in any channel, as `(x, y, partial, full)`.
fn mismatches(partial: &[u8], full: &[u8], tolerance: u8) -> Vec<(u32, u32, [u8; 4], [u8; 4])> {
    (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .filter_map(|(x, y)| {
            let (p, f) = (px(partial, x, y), px(full, x, y));
            (!near(p, f, tolerance)).then_some((x, y, p, f))
        })
        .take(8)
        .collect()
}

/// A removed boundary's shadow leaves no penumbra behind: the damage its
/// display list reports reaches as far as the GPU shadow's ink, which the
/// analytic rounded-rect shadow spreads three sigma past a rect offset half
/// a sigma down (sigma = elevation), so 3.5 x elevation below the shape.
///
/// The removed boundary is the only change, so the damage is its old
/// extent alone and any ink outside it survives the partial frame. The
/// comparison is exact: both frames paint only background there.
#[test]
fn a_removed_shadow_leaves_no_penumbra() {
    use flui_foundation::geometry::RRect;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, card) = (ContentToken::mint(), ContentToken::mint());
    let shadowed = |canvas: &mut Canvas| {
        let shape = Path::from_rrect(RRect::from_rect_circular(
            Rect::from_xywh(0.0, 0.0, 32.0, 16.0),
            4.0,
        ));
        assert!(shape.rrect_hint().is_some(), "precondition: analytic path");
        canvas.draw_shadow(&shape, Color::BLACK, 8.0);
    };
    let before = scene(
        &root,
        &[Boundary {
            id: 2,
            token: &card,
            at: Offset::new(40.0, 40.0),
            paint: &shadowed,
        }],
        None,
    );
    let after = scene(&root, &[], None);

    let mut partial = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    let mut differ = LayerDiffer::default();
    let region = differ.diff(&before, (SIDE, SIDE));
    frame(&mut partial, &before, region, |plan| {
        plan == FramePlan::Direct
    });
    warm(&mut partial, &before);
    let region = differ.diff(&after, (SIDE, SIDE));
    frame(&mut partial, &after, region, |plan| {
        matches!(plan, FramePlan::RetainedPartial(_))
    });
    let partial_pixels = partial.read_rgba().expect("readback");
    let full_pixels = full_frame_pixels(&renderer, &after);

    let stale = mismatches(&partial_pixels, &full_pixels, 0);
    assert!(
        stale.is_empty(),
        "the old shadow's penumbra survived outside the damage at \
         (x, y, partial, full): {stale:?}"
    );
}

/// A frame rendered partially over its predecessor equals the same frame
/// rendered in full, everywhere: inside the damage (text with its glyph
/// overflow, a shadow, a dst-reading blend), where a backdrop filter reads
/// across the damage edge, and outside it.
#[test]
fn partial_equals_full_inside_damage() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let text = |word: &'static str| {
        move |canvas: &mut Canvas| {
            let mut painter = TextPainter::new()
                .with_text(
                    TextSpan::new(word).with_style(
                        TextStyle::new()
                            .with_font_size(14.0)
                            .with_color(Color::BLACK),
                    ),
                )
                .with_text_direction(TextDirection::Ltr);
            painter.layout(0.0, 100.0);
            let shadow = Path::rectangle(Rect::from_xywh(0.0, 20.0, 30.0, 8.0));
            canvas.draw_shadow(&shadow, Color::BLACK, 3.0);
            painter.paint(canvas, Offset::ZERO);
            canvas.draw_rect(
                Rect::from_xywh(4.0, 4.0, 12.0, 12.0),
                &Paint::fill(Color::rgba(0, 128, 255, 255)).with_blend_mode(BlendMode::Multiply),
            );
        }
    };
    let static_art = |canvas: &mut Canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 30.0, 30.0),
            &Paint::fill(Color::rgba(255, 160, 0, 255)),
        );
        canvas.draw_rect(
            Rect::from_xywh(8.0, 8.0, 14.0, 14.0),
            &Paint::fill(Color::rgba(0, 0, 255, 255)).with_blend_mode(BlendMode::Multiply),
        );
    };
    let (root, fixed) = (ContentToken::mint(), ContentToken::mint());
    let hello = text("Hello");
    let world = text("World");
    let build = |token: &ContentToken, paint: &dyn Fn(&mut Canvas)| {
        scene(
            &root,
            &[
                Boundary {
                    id: 2,
                    token,
                    at: Offset::new(10.0, 10.0),
                    paint,
                },
                Boundary {
                    id: 3,
                    token: &fixed,
                    at: Offset::new(90.0, 90.0),
                    paint: &static_art,
                },
            ],
            Some(Rect::from_xywh(30.0, 30.0, 40.0, 30.0)),
        )
    };
    let before = build(&ContentToken::mint(), &hello);
    let after = build(&ContentToken::mint(), &world);

    // Partial: the previous frame, then the new one over it.
    let mut partial = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    let mut differ = LayerDiffer::default();
    let region = differ.diff(&before, (SIDE, SIDE));
    frame(&mut partial, &before, region, |plan| {
        plan == FramePlan::Direct
    });
    warm(&mut partial, &before);
    let region = differ.diff(&after, (SIDE, SIDE));
    let DamageRegion::Partial(damage) = region else {
        panic!("the text change is a partial region, got {region:?}");
    };
    assert!(
        damage.right() >= 70 && damage.bottom() >= 60,
        "the damage reaches the backdrop and joins it: {damage:?}"
    );
    frame(&mut partial, &after, region, |plan| {
        matches!(plan, FramePlan::RetainedPartial(_))
    });
    let partial_pixels = partial.read_rgba().expect("readback");

    // Full: the new frame alone.
    let mut full = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    full.mark_full_repaint();
    full.render_scene(&after).expect("full frame");
    let full_pixels = full.read_rgba().expect("readback");

    let mismatches: Vec<(u32, u32, [u8; 4], [u8; 4])> = (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .filter_map(|(x, y)| {
            let (p, f) = (px(&partial_pixels, x, y), px(&full_pixels, x, y));
            (!near(p, f, 2)).then_some((x, y, p, f))
        })
        .take(8)
        .collect();
    assert!(
        mismatches.is_empty(),
        "partial and full frames differ at (x, y, partial, full): {mismatches:?}"
    );
    assert_ne!(
        px(&full_pixels, 100, 100),
        WHITE,
        "precondition: the static art painted"
    );
}

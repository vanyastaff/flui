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
#[derive(Clone, Copy)]
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

// Private quota seam is necessary: public capture uses the default profile.
// Assertions observe rendered pixels and admission, never slot/texture identity.
#[cfg(feature = "testing")]
fn bounded_reused_targets_preserve_pixels() {
    use crate::device_domain::{DeviceDomain, PreparedCost, PreparedIrLimits};
    use crate::retained_target::RetainedTarget;
    use std::sync::Arc;
    let (device, queue) = crate::test_support::test_device_and_queue("Bounded retained reuse");
    let domain = DeviceDomain::with_limits(
        Arc::clone(&device),
        queue,
        PreparedIrLimits {
            cost: PreparedCost {
                gpu_bytes: 8 * 8 * 4 * 2,
                cpu_bytes: 0,
                objects: 4,
            },
            submissions: 64,
        },
    );
    missing_partial_source_preserves_both_targets(&device, domain.queue());
    let mut target = RetainedTarget::default();
    for (size, fail, red) in [
        ((8, 8), false, false),
        ((8, 8), false, true),
        ((8, 8), false, false),
        ((8, 8), true, true),
        ((8, 8), false, true),
        ((4, 4), false, false),
    ] {
        let candidate = target
            .begin(&domain, size, wgpu::TextureFormat::Rgba8Unorm, false)
            .expect("two-slot quota admits reused candidate");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: candidate.view(),
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if red {
                            wgpu::Color::RED
                        } else {
                            wgpu::Color::GREEN
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        domain
            .submit(
                domain
                    .prepare(vec![encoder.finish()], vec![])
                    .expect("prepare clear"),
            )
            .expect("candidate clear submits");
        if fail {
            drop(candidate);
        } else {
            target.commit(candidate);
        }
        let expected = if red && !fail {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        };
        let pixels = crate::test_support::readback_bytes(
            &device,
            domain.queue(),
            target.texture().expect("committed image"),
            size.0,
            size.1,
        );
        assert_eq!(
            &pixels[0..4],
            &expected,
            "failed clear must not replace committed pixels"
        );
    }
}

#[cfg(feature = "testing")]
fn missing_partial_source_preserves_both_targets(
    device: &std::sync::Arc<wgpu::Device>,
    queue: &std::sync::Arc<wgpu::Queue>,
) {
    use crate::device_domain::{DeviceDomain, DomainError, PreparedCost, PreparedIrLimits};
    use crate::error::{EngineError, Recoverability};
    use crate::retained_target::RetainedTarget;
    use std::sync::Arc;
    let limits = PreparedIrLimits {
        cost: PreparedCost {
            gpu_bytes: 8 * 8 * 4 * 2,
            cpu_bytes: 0,
            objects: 4,
        },
        submissions: 64,
    };
    let domain = DeviceDomain::with_limits(Arc::clone(device), Arc::clone(queue), limits);
    let foreign = DeviceDomain::new(Arc::clone(device), Arc::clone(queue));
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut target = RetainedTarget::default();
    let Err(missing) = target.begin(&domain, (8, 8), format, true) else {
        panic!("partial preparation requires a committed source");
    };
    assert!(matches!(missing, EngineError::MissingRetainedSource));
    assert_eq!(missing.recoverability(), Recoverability::Recoverable);
    for color in [wgpu::Color::GREEN, wgpu::Color::RED] {
        let candidate = target
            .begin(&domain, (8, 8), format, false)
            .expect("bounded candidate");
        crate::test_support::clear_target(device, queue, candidate.view(), color);
        target.commit(candidate);
    }
    for (owner, size, expected_missing) in [(&foreign, (8, 8), false), (&domain, (4, 4), true)] {
        let Err(error) = target.begin(owner, size, format, true) else {
            panic!("incompatible partial source must fail");
        };
        if expected_missing {
            assert!(matches!(error, EngineError::MissingRetainedSource));
            assert_eq!(error.recoverability(), Recoverability::Recoverable);
        } else {
            assert!(matches!(error, EngineError::DeviceDomainMismatch));
            assert_eq!(error.recoverability(), Recoverability::Unrecoverable);
        }
    }
    // Acquire/reconfigure can invalidate the source after planning a partial.
    target.invalidate();
    assert!(matches!(
        target.begin(&domain, (8, 8), format, true),
        Err(EngineError::MissingRetainedSource)
    ));
    let pixels = crate::test_support::readback_bytes(
        device,
        queue,
        target.texture().expect("previous committed image"),
        8,
        8,
    );
    assert_eq!(&pixels[0..4], &[255, 0, 0, 255]);
    domain
        .poll()
        .expect("retirement progress after rejected preparation");
    // Both allocations must remain owned: polling must not free a spare that
    // failed source validation never had authority to consume.
    assert!(matches!(
        domain.reserve(PreparedCost {
            gpu_bytes: 1,
            cpu_bytes: 0,
            objects: 0
        }),
        Err(DomainError::Budget { .. })
    ));
    let candidate = target
        .begin(&domain, (8, 8), format, false)
        .expect("full retry reuses spare within two-target quota");
    crate::test_support::clear_target(device, queue, candidate.view(), wgpu::Color::GREEN);
    target.commit(candidate);
    let pixels = crate::test_support::readback_bytes(
        device,
        queue,
        target.texture().expect("retried committed image"),
        8,
        8,
    );
    assert_eq!(&pixels[0..4], &[0, 255, 0, 255]);
}

/// A retained target that is not known to hold the last frame is never
/// trusted after resize. A failed candidate preserves the committed image
/// and leaves a full retry owed; the retry then overwrites stale pixels.
#[test]
fn an_invalid_target_promotes_to_full() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    #[cfg(feature = "testing")]
    bounded_reused_targets_preserve_pixels();
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
    // Alternate candidates repeatedly: reuse must seed from the latest committed
    // image rather than exposing stale contents from the spare slot.
    for _ in 0..4 {
        capture.mark_dirty(small);
        capture
            .render_scene(&still)
            .expect("reused partial candidate");
        let pixels = capture.read_rgba().expect("reused candidate readback");
        assert_eq!(
            px(&pixels, 84, 84),
            GREEN,
            "copy preserves undamaged pixels across slot swaps"
        );
        assert_eq!(px(&pixels, 16, 16), [255, 0, 0, 255]);
    }
    capture.fail_next_frame_after_begin();
    capture.mark_dirty(small);
    assert!(
        capture.render_scene(&still).is_err(),
        "the injected failure"
    );
    let committed = capture
        .read_retained_rgba()
        .expect("committed readback after failure");
    assert_eq!(
        px(&committed, 84, 84),
        GREEN,
        "failed candidate must preserve the previous committed pixels"
    );
    capture.mark_dirty(small);
    capture.render_scene(&still).expect("the retry renders");
    assert_eq!(
        capture.last_plan(),
        Some(FramePlan::Direct),
        "full retry uses the direct surface when no intermediate is required"
    );
    let pixels = capture.read_rgba().expect("readback");
    assert_eq!(
        px(&pixels, 84, 84),
        WHITE,
        "the stale sentinel is repainted"
    );
    capture.mark_dirty(small);
    capture
        .render_scene(&still)
        .expect("retained target is rewarmed after direct retry");
    assert_eq!(capture.last_plan(), Some(FramePlan::RetainedFull));
    let committed = capture
        .read_retained_rgba()
        .expect("committed retry readback");
    assert_eq!(px(&committed, 84, 84), WHITE);
    capture.mark_dirty(small);
    capture
        .render_scene(&still)
        .expect("next partial frame progresses");
    assert!(matches!(
        capture.last_plan(),
        Some(FramePlan::RetainedPartial(_))
    ));

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
            painter.layout(
                &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
                0.0,
                100.0,
            );
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

/// A picture that draws an external texture keeps its boundary's token when
/// the texture's producer hands over a new frame behind the same id, as a
/// video decoder or a camera does: the next frame repaints the texture's
/// rect and shows the new content, rather than finding the scene unchanged
/// and presenting the old frame.
#[test]
fn an_updated_texture_repaints_under_an_unchanged_picture() {
    use flui_painting::paint::{FilterQuality, TextureId};

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    let texture = TextureId::new(41);
    capture.set_solid_texture(texture, RED);
    let (root, card) = (ContentToken::mint(), ContentToken::mint());
    let video = move |canvas: &mut Canvas| {
        canvas.draw_texture(
            texture,
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            None,
            FilterQuality::None,
            1.0,
        );
    };
    let build = || {
        scene(
            &root,
            &[Boundary {
                id: 2,
                token: &card,
                at: Offset::new(40.0, 40.0),
                paint: &video,
            }],
            None,
        )
    };
    let mut differ = LayerDiffer::default();
    let first = build();
    let region = differ.diff(&first, (SIDE, SIDE));
    frame(&mut capture, &first, region, |plan| {
        plan == FramePlan::Direct
    });
    warm(&mut capture, &first);
    assert!(
        near(px(&capture.read_rgba().expect("readback"), 56, 56), RED, 2),
        "precondition: the texture's first frame is on screen"
    );

    capture.set_solid_texture(texture, BLUE);
    let second = build();
    let region = differ.diff(&second, (SIDE, SIDE));
    apply(&mut capture, region);
    capture.render_scene(&second).expect("the frame renders");
    let shown = px(&capture.read_rgba().expect("readback"), 56, 56);
    assert!(
        near(shown, BLUE, 2),
        "the texture's new content is shown, got {shown:?} (red is the frozen \
         previous frame; region {region:?}, plan {:?})",
        capture.last_plan()
    );
}

/// A partial frame whose damage cuts through destination-reading blends (a
/// Multiply rect, a ColorBurn circle, a Screen save-layer) presents the same
/// pixels as the frame rendered in full, everywhere. Such a shape composites
/// over its whole device bounds with no scissor, but its foreground was
/// recorded under the damage scissor: outside the damage it is transparent,
/// and a blend of a transparent source leaves the retained pixels (which
/// already hold the shape's result) as they were.
#[test]
fn a_damage_edge_through_an_advanced_blend_matches_a_full_frame() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let art = |canvas: &mut Canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 48.0, 48.0),
            &Paint::fill(Color::rgba(255, 160, 0, 255)),
        );
        canvas.draw_rect(
            Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
            &Paint::fill(Color::rgba(128, 128, 128, 255)).with_blend_mode(BlendMode::Multiply),
        );
        canvas.draw_circle(
            flui_foundation::geometry::Point::new(24.0, 24.0),
            14.0,
            &Paint::fill(Color::rgba(40, 90, 200, 200)).with_blend_mode(BlendMode::ColorBurn),
        );
        canvas.save_layer_blend(
            Some(Rect::from_xywh(4.0, 14.0, 10.0, 30.0)),
            BlendMode::Screen,
        );
        canvas.draw_rect(
            Rect::from_xywh(4.0, 14.0, 10.0, 30.0),
            &Paint::fill(Color::rgba(0, 120, 255, 160)),
        );
        canvas.restore();
    };
    let marker = square(Color::rgba(0, 160, 0, 255));
    let (root, fixed, moving) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let build = |at: Offset<f64>| {
        scene(
            &root,
            &[
                Boundary {
                    id: 3,
                    token: &fixed,
                    at: Offset::new(40.0, 40.0),
                    paint: &art,
                },
                Boundary {
                    id: 2,
                    token: &moving,
                    at,
                    paint: &marker,
                },
            ],
            None,
        )
    };
    let before = build(Offset::new(36.0, 36.0));
    let after = build(Offset::new(36.0, 40.0));

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
        panic!("the moved marker is a partial region, got {region:?}");
    };
    let multiply = Rect::from_xywh(48.0, 48.0, 32.0, 32.0);
    assert!(
        damage.to_rect().intersects(&multiply) && !damage.to_rect().contains_rect(&multiply),
        "precondition: the damage {damage:?} cuts through the multiply {multiply:?}"
    );
    frame(&mut partial, &after, region, |plan| {
        matches!(plan, FramePlan::RetainedPartial(_))
    });
    let partial_pixels = partial.read_rgba().expect("readback");
    let full_pixels = full_frame_pixels(&renderer, &after);

    let wrong = mismatches(&partial_pixels, &full_pixels, 2);
    assert!(
        wrong.is_empty(),
        "the presented frame differs from the full frame at (x, y, partial, full): {wrong:?}"
    );
    assert!(
        near(px(&full_pixels, 79, 79), [128, 80, 0, 255], 3),
        "precondition: the multiply painted, got {:?}",
        px(&full_pixels, 79, 79)
    );
}

/// A removed shadow under `scale(4, 0.25)` leaves no penumbra on the axis
/// the scale compresses: the renderer blurs with one sigma from the largest
/// scale (4 x elevation) on both axes, so the damage reaches that far
/// vertically too, not a quarter of it.
#[test]
fn a_removed_shadow_under_a_non_uniform_scale_leaves_no_penumbra() {
    use flui_foundation::geometry::RRect;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, card) = (ContentToken::mint(), ContentToken::mint());
    let shadowed = |canvas: &mut Canvas| {
        let shape = Path::from_rrect(RRect::from_rect_circular(
            Rect::from_xywh(0.0, 0.0, 8.0, 64.0),
            1.0,
        ));
        canvas.scale(4.0, 0.25);
        canvas.draw_shadow(&shape, Color::BLACK, 2.0);
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
    assert_ne!(
        px(&partial.read_rgba().expect("readback"), 56, 62),
        WHITE,
        "precondition: the penumbra reaches below the shape on the compressed axis"
    );
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

/// Removed text leaves no ink behind when its glyphs stand far outside its
/// line box: under a line height a third of the font size, ascenders and
/// descenders reach well past half a line.
#[test]
fn removed_text_under_a_tight_line_height_leaves_no_ink() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, card) = (ContentToken::mint(), ContentToken::mint());
    let text = |canvas: &mut Canvas| {
        let mut painter = TextPainter::new()
            .with_text(
                TextSpan::new("Hgjy").with_style(
                    TextStyle::new()
                        .with_font_size(24.0)
                        .with_height(0.3)
                        .with_color(Color::BLACK),
                ),
            )
            .with_text_direction(TextDirection::Ltr);
        painter.layout(
            &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
            0.0,
            100.0,
        );
        painter.paint(canvas, Offset::ZERO);
    };
    let before = scene(
        &root,
        &[Boundary {
            id: 2,
            token: &card,
            at: Offset::new(30.0, 50.0),
            paint: &text,
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
        "the old glyphs' ink survived outside the damage at \
         (x, y, partial, full): {stale:?}"
    );
}

/// Renders `before` in full, then `after` as the partial frame its diff
/// produces, and returns the partial frame's pixels beside `after` rendered
/// in full on a fresh capture.
fn partial_and_full(
    renderer: &crate::headless::HeadlessRenderer,
    before: &Scene,
    after: &Scene,
) -> (Vec<u8>, Vec<u8>) {
    damaged_and_full(renderer, before, after, |plan| {
        matches!(plan, FramePlan::RetainedPartial(_))
    })
}

/// [`partial_and_full`] for a second frame whose plan `plan` accepts.
fn damaged_and_full(
    renderer: &crate::headless::HeadlessRenderer,
    before: &Scene,
    after: &Scene,
    plan: fn(FramePlan) -> bool,
) -> (Vec<u8>, Vec<u8>) {
    let mut partial = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    let mut differ = LayerDiffer::default();
    let region = differ.diff(before, (SIDE, SIDE));
    frame(&mut partial, before, region, |plan| {
        plan == FramePlan::Direct
    });
    warm(&mut partial, before);
    let region = differ.diff(after, (SIDE, SIDE));
    frame(&mut partial, after, region, plan);
    (
        partial.read_rgba().expect("readback"),
        full_frame_pixels(renderer, after),
    )
}

/// Two overlapping boundaries that swap paint order, keeping their tokens and
/// positions: where they overlap, the one now on top shows.
#[test]
fn overlapping_boundaries_that_swap_order_repaint_the_overlap() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, red_token, blue_token) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let red = square(Color::RED);
    let blue = square(Color::BLUE);
    let red_box = Boundary {
        id: 2,
        token: &red_token,
        at: Offset::new(40.0, 40.0),
        paint: &red,
    };
    let blue_box = Boundary {
        id: 3,
        token: &blue_token,
        at: Offset::new(48.0, 48.0),
        paint: &blue,
    };
    let red_on_top = scene(&root, &[blue_box, red_box], None);
    let blue_on_top = scene(&root, &[red_box, blue_box], None);

    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("capture target");
    let mut differ = LayerDiffer::default();
    let region = differ.diff(&red_on_top, (SIDE, SIDE));
    frame(&mut capture, &red_on_top, region, |plan| {
        plan == FramePlan::Direct
    });
    warm(&mut capture, &red_on_top);
    assert_eq!(
        px(&capture.read_rgba().expect("readback"), 52, 52),
        RED,
        "precondition: red is on top"
    );
    let region = differ.diff(&blue_on_top, (SIDE, SIDE));
    apply(&mut capture, region);
    capture
        .render_scene(&blue_on_top)
        .expect("the frame renders");
    let partial = capture.read_rgba().expect("readback");
    let full = full_frame_pixels(&renderer, &blue_on_top);
    assert_eq!(px(&full, 52, 52), BLUE, "precondition: blue is on top");
    assert_eq!(
        px(&partial, 52, 52),
        BLUE,
        "the overlap shows the boundary now on top (region {region:?})"
    );
    let stale = mismatches(&partial, &full, 0);
    assert!(stale.is_empty(), "stale pixels at {stale:?}");
}

/// A removed atlas sprite leaves nothing where the renderer placed it: the
/// sprite's size at its transform's translation, far from the source rect's
/// position in the image.
#[test]
fn a_removed_atlas_sprite_leaves_nothing_at_its_destination() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, card) = (ContentToken::mint(), ContentToken::mint());
    let sprites = |canvas: &mut Canvas| {
        canvas.draw_atlas(
            flui_painting::paint::Image::solid_color(64, 64, Color::RED),
            vec![Rect::from_ltrb(40.0, 40.0, 56.0, 56.0)],
            vec![Matrix4::translation(8.0, 8.0, 0.0)],
            None,
            BlendMode::SrcOver,
            None,
        );
    };
    let before = scene(
        &root,
        &[Boundary {
            id: 2,
            token: &card,
            at: Offset::ZERO,
            paint: &sprites,
        }],
        None,
    );
    let after = scene(&root, &[], None);
    let (partial, full) = partial_and_full(&renderer, &before, &after);
    assert_eq!(px(&full, 16, 16), WHITE);
    assert_eq!(px(&partial, 16, 16), WHITE, "the sprite is gone");
    let stale = mismatches(&partial, &full, 0);
    assert!(stale.is_empty(), "stale pixels at {stale:?}");
}

/// A removed fill-style line and point leave nothing behind: the renderer
/// strokes both at the paint's raw `stroke_width` whatever its style.
#[test]
fn removed_fill_style_lines_and_points_leave_nothing() {
    use flui_foundation::geometry::Point;
    use flui_painting::paint::PointMode;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, card) = (ContentToken::mint(), ContentToken::mint());
    let strokes = |canvas: &mut Canvas| {
        let mut paint = Paint::fill(Color::BLACK);
        paint.stroke_width = 6.0;
        canvas.draw_line(Point::new(10.0, 20.0), Point::new(90.0, 20.0), &paint);
        canvas.draw_points_with_mode(PointMode::Points, vec![Point::new(60.0, 60.0)], &paint);
    };
    let before = scene(
        &root,
        &[Boundary {
            id: 2,
            token: &card,
            at: Offset::ZERO,
            paint: &strokes,
        }],
        None,
    );
    let drawn = full_frame_pixels(&renderer, &before);
    assert_ne!(px(&drawn, 50, 22), WHITE, "precondition: the line is drawn");
    assert_ne!(
        px(&drawn, 60, 62),
        WHITE,
        "precondition: the point is drawn"
    );

    let after = scene(&root, &[], None);
    let (partial, full) = partial_and_full(&renderer, &before, &after);
    let stale = mismatches(&partial, &full, 0);
    assert!(stale.is_empty(), "stale pixels at {stale:?}");
}

/// A full-surface green picture, standing for whatever a frame paints under
/// the content a test adds and removes.
fn green_background(canvas: &mut Canvas) {
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, f64::from(SIDE), f64::from(SIDE)),
        &Paint::fill(GREEN_COLOR),
    );
}

const GREEN_COLOR: Color = Color::rgba(0, 255, 0, 255);

/// `translate(4, 0)`, a `Src` layer over local `(0, 0, 32, 32)`, red ink
/// over its first 16×16. At the boundary's `(40, 40)` the layer covers
/// device `(44, 40)-(76, 72)` and the ink `(44, 40)-(60, 56)`.
fn translated_src_layer(canvas: &mut Canvas) {
    canvas.translate(4.0, 0.0);
    canvas.save_layer(
        Some(Rect::from_xywh(0.0, 0.0, 32.0, 32.0)),
        &Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Src),
    );
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 16.0, 16.0),
        &Paint::fill(Color::RED),
    );
    canvas.restore();
}

/// The same device rectangles as [`translated_src_layer`], reached through
/// `translate(4, 0) scale(2)` over half-size local bounds and ink.
fn translated_scaled_src_layer(canvas: &mut Canvas) {
    canvas.translate(4.0, 0.0);
    canvas.scale(2.0, 2.0);
    canvas.save_layer(
        Some(Rect::from_xywh(0.0, 0.0, 16.0, 16.0)),
        &Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Src),
    );
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
        &Paint::fill(Color::RED),
    );
    canvas.restore();
}

/// An empty `Clear` layer over the same device rectangle as
/// [`translated_src_layer`].
fn translated_empty_clear_layer(canvas: &mut Canvas) {
    canvas.translate(4.0, 0.0);
    canvas.save_layer(
        Some(Rect::from_xywh(0.0, 0.0, 32.0, 32.0)),
        &Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Clear),
    );
    canvas.restore();
}

/// An opaque `SrcOver` layer over local `(-12, -12, 24, 24)` rotated 45°
/// about the boundary's `(20, 20)`, device `(60, 60)`: a diamond whose
/// bounding box reaches `(43, 43)`. Its red ink, local `(-30, -30, 60, 60)`,
/// reaches far past the diamond, through the bounding box's corners.
fn rotated_opaque_layer_with_oversized_ink(canvas: &mut Canvas) {
    canvas.translate(20.0, 20.0);
    canvas.rotate(std::f64::consts::FRAC_PI_4);
    canvas.save_layer(
        Some(Rect::from_xywh(-12.0, -12.0, 24.0, 24.0)),
        &Paint::fill(Color::WHITE),
    );
    canvas.draw_rect(
        Rect::from_xywh(-30.0, -30.0, 60.0, 60.0),
        &Paint::fill(Color::RED),
    );
    canvas.restore();
}

/// The samples of the translated rows: the layer covers device
/// `(44, 40)-(76, 72)`, its ink `(44, 40)-(60, 56)`.
const TRANSLATED_LAYER_SAMPLES: &[((u32, u32), [u8; 4])] = &[
    ((70, 66), [0, 0, 0, 0]),
    ((42, 50), GREEN),
    ((10, 10), GREEN),
    ((78, 74), GREEN),
    ((100, 100), GREEN),
    ((50, 46), RED),
];

/// A save layer recorded under a transform leaves nothing behind when
/// removed. While present it composites its whole bounds, mapped through the
/// transform, the pixels its content left transparent included, and nothing
/// outside them, the corners of a rotated layer's bounding box included; the
/// damage, which covers the mapped bounds, covers every pixel it changed.
#[test]
fn a_removed_translated_src_save_layer_leaves_nothing_behind() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background, card) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    // Name, the boundary's picture, what the full frame must read.
    type Row = (
        &'static str,
        fn(&mut Canvas),
        &'static [((u32, u32), [u8; 4])],
    );
    let rows: [Row; 4] = [
        (
            "translated Src",
            translated_src_layer,
            TRANSLATED_LAYER_SAMPLES,
        ),
        (
            "translated and scaled Src",
            translated_scaled_src_layer,
            TRANSLATED_LAYER_SAMPLES,
        ),
        (
            "empty Clear",
            translated_empty_clear_layer,
            &[
                ((70, 66), [0, 0, 0, 0]),
                ((42, 50), GREEN),
                ((10, 10), GREEN),
                ((78, 74), GREEN),
                ((100, 100), GREEN),
                ((50, 46), [0, 0, 0, 0]),
            ],
        ),
        (
            "rotated opaque SrcOver",
            rotated_opaque_layer_with_oversized_ink,
            // The centre, then a bounding-box corner the ink covers but the
            // diamond does not, then a point past the bounding box.
            &[((60, 60), RED), ((46, 46), GREEN), ((40, 60), GREEN)],
        ),
    ];
    let mut failed = Vec::new();
    for (name, layered, expected) in rows {
        let backdrop = Boundary {
            id: 3,
            token: &background,
            at: Offset::ZERO,
            paint: &green_background,
        };
        let before = scene(
            &root,
            &[
                backdrop,
                Boundary {
                    id: 2,
                    token: &card,
                    at: Offset::new(40.0, 40.0),
                    paint: &layered,
                },
            ],
            None,
        );
        let after = scene(&root, &[backdrop], None);

        let drawn = full_frame_pixels(&renderer, &before);
        for &((x, y), want) in expected {
            let got = px(&drawn, x, y);
            if got != want {
                failed.push(format!(
                    "{name}: ({x}, {y}) drawn {got:?}, expected {want:?}"
                ));
            }
        }

        let (partial, full) = partial_and_full(&renderer, &before, &after);
        let stale = mismatches(&partial, &full, 0);
        if !stale.is_empty() {
            failed.push(format!("{name}: stale pixels at {stale:?}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A removed shader mask leaves nothing behind: the boundary's damage covers
/// the mask's whole bounds, which contain every pixel its composite writes.
/// The mask records `Clear`, which combines its shader with its child and is
/// not applied at the composite (ADR-0099 §4): the child shows and the
/// backdrop around it stays.
#[test]
fn a_removed_shader_mask_leaves_nothing_behind() {
    use flui_layer::ShaderMaskLayer;
    use flui_painting::paint::Shader;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background, card) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let build = |masked: bool| {
        let mut tree = LayerTree::new(
            LayerNode::new(Layer::from(TransformLayer::new(Matrix4::IDENTITY)))
                .with_boundary(id(1), root.clone()),
        );
        let root_id = tree.root();
        let backdrop = tree.push_child(
            root_id,
            LayerNode::new(Layer::from(OffsetLayer::new(Offset::ZERO)))
                .with_boundary(id(3), background.clone()),
        );
        let mut canvas = Canvas::new();
        green_background(&mut canvas);
        tree.push_child(backdrop, Layer::from(PictureLayer::new(canvas.finish())));
        if masked {
            let boundary = tree.push_child(
                root_id,
                LayerNode::new(Layer::from(OffsetLayer::new(Offset::new(20.0, 20.0))))
                    .with_boundary(id(2), card.clone()),
            );
            let mask = tree.push_child(
                boundary,
                Layer::from(ShaderMaskLayer::new(
                    Shader::solid(Color::WHITE),
                    BlendMode::Clear,
                    Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
                )),
            );
            let mut canvas = Canvas::new();
            canvas.draw_rect(
                Rect::from_xywh(0.0, 0.0, 16.0, 16.0),
                &Paint::fill(Color::RED),
            );
            tree.push_child(mask, Layer::from(PictureLayer::new(canvas.finish())));
        }
        Scene::new(tree)
    };
    let before = build(true);
    let after = build(false);

    let drawn = full_frame_pixels(&renderer, &before);
    assert_eq!(
        (px(&drawn, 24, 24), px(&drawn, 45, 45)),
        (RED, GREEN),
        "precondition: the mask composited its child over the backdrop"
    );

    let (partial, full) = partial_and_full(&renderer, &before, &after);
    let stale = mismatches(&partial, &full, 0);
    assert!(stale.is_empty(), "stale pixels at {stale:?}");
}

/// A frame's tree: a stamped root over a stamped full-surface backdrop
/// (boundary 3), then whatever `extra` adds under the root.
fn backdrop_scene(
    root: &ContentToken,
    backdrop: &ContentToken,
    paint_backdrop: &dyn Fn(&mut Canvas),
    extra: &dyn Fn(&mut LayerTree, flui_layer::LayerId),
) -> Scene {
    let mut tree = LayerTree::new(
        LayerNode::new(Layer::from(TransformLayer::new(Matrix4::IDENTITY)))
            .with_boundary(id(1), root.clone()),
    );
    let root_id = tree.root();
    let layer = tree.push_child(
        root_id,
        LayerNode::new(Layer::from(OffsetLayer::new(Offset::ZERO)))
            .with_boundary(id(3), backdrop.clone()),
    );
    let mut canvas = Canvas::new();
    paint_backdrop(&mut canvas);
    tree.push_child(layer, Layer::from(PictureLayer::new(canvas.finish())));
    extra(&mut tree, root_id);
    Scene::new(tree)
}

/// A picture of one `color` rect.
fn rect_picture(rect: Rect<f64>, color: Color) -> Layer {
    let mut canvas = Canvas::new();
    canvas.draw_rect(rect, &Paint::fill(color));
    Layer::from(PictureLayer::new(canvas.finish()))
}

/// Vertical stripes, so a blur of the backdrop changes its pixels.
fn striped_backdrop(canvas: &mut Canvas) {
    green_background(canvas);
    for x in (0..SIDE).step_by(8) {
        canvas.draw_rect(
            Rect::from_xywh(f64::from(x), 0.0, 4.0, f64::from(SIDE)),
            &Paint::fill(Color::BLUE),
        );
    }
}

/// Removing an effect that draws through an offscreen (a shader mask, a
/// backdrop filter) under an ancestor clip smaller than its bounds leaves
/// nothing behind, wherever the offscreen composite lands.
#[test]
fn a_removed_offscreen_effect_under_a_clip_leaves_nothing_behind() {
    use flui_layer::{ClipRectLayer, ShaderMaskLayer};
    use flui_painting::paint::Shader;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background, card) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let clip = Rect::from_xywh(30.0, 30.0, 20.0, 20.0);
    for backdrop_filter in [false, true] {
        let build = |present: bool| {
            backdrop_scene(&root, &background, &striped_backdrop, &|tree, root_id| {
                if !present {
                    return;
                }
                let boundary = tree.push_child(
                    root_id,
                    LayerNode::new(Layer::from(OffsetLayer::new(Offset::ZERO)))
                        .with_boundary(id(2), card.clone()),
                );
                let clipped =
                    tree.push_child(boundary, Layer::from(ClipRectLayer::hard_edge(clip)));
                let effect_bounds = Rect::from_xywh(10.0, 10.0, 80.0, 80.0);
                if backdrop_filter {
                    tree.push_child(
                        clipped,
                        Layer::from(BackdropFilterLayer::new(
                            ImageFilter::blur(4.0),
                            BlendMode::SrcOver,
                            effect_bounds,
                        )),
                    );
                } else {
                    let mask = tree.push_child(
                        clipped,
                        Layer::from(ShaderMaskLayer::new(
                            Shader::solid(Color::WHITE),
                            BlendMode::SrcOver,
                            effect_bounds,
                        )),
                    );
                    tree.push_child(mask, rect_picture(effect_bounds, Color::RED));
                }
            })
        };
        let (before, after) = (build(true), build(false));
        let (partial, full) = partial_and_full(&renderer, &before, &after);
        let stale = mismatches(&partial, &full, 0);
        assert!(
            stale.is_empty(),
            "backdrop filter {backdrop_filter}: stale pixels at {stale:?}"
        );
    }
}

/// A small change next to an unchanged foreground blur (an image-filter
/// layer) presents the same pixels as a full frame: the blur reads its input
/// past the damage and writes past it, so damage that meets it takes it in.
#[test]
fn a_change_beside_a_foreground_blur_matches_a_full_frame() {
    use flui_layer::ImageFilterLayer;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background, blurred, moving) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let build = |at: Offset<f64>| {
        backdrop_scene(&root, &background, &green_background, &|tree, root_id| {
            let boundary = tree.push_child(
                root_id,
                LayerNode::new(Layer::from(OffsetLayer::new(Offset::ZERO)))
                    .with_boundary(id(4), blurred.clone()),
            );
            let filter = tree.push_child(boundary, Layer::from(ImageFilterLayer::blur(4.0)));
            tree.push_child(
                filter,
                rect_picture(Rect::from_xywh(40.0, 40.0, 30.0, 30.0), Color::RED),
            );
            let mover = tree.push_child(
                root_id,
                LayerNode::new(Layer::from(OffsetLayer::new(at)))
                    .with_boundary(id(2), moving.clone()),
            );
            tree.push_child(
                mover,
                rect_picture(Rect::from_xywh(0.0, 0.0, 6.0, 6.0), Color::BLUE),
            );
        })
    };
    let (before, after) = (
        build(Offset::new(66.0, 50.0)),
        build(Offset::new(66.0, 54.0)),
    );
    let (partial, full) = partial_and_full(&renderer, &before, &after);
    let stale = mismatches(&partial, &full, 2);
    assert!(stale.is_empty(), "stale pixels at {stale:?}");
}

/// Removing an opacity layer with a destination-replacing blend, or a colour
/// filter that paints transparent pixels, leaves nothing behind wherever
/// their composite reached, not only under their child.
#[test]
fn a_removed_destination_affecting_layer_leaves_nothing_behind() {
    use flui_layer::{ColorFilterLayer, OpacityLayer};
    use flui_painting::paint::ColorFilter;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background, card) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    // The third field is what the drawn frame holds at (100, 100), far from
    // the child, when the composite is known to reach it: an unbounded
    // destination-replacing opacity layer replaces the whole viewport.
    let effects = [
        (
            "opacity Src",
            Layer::from(OpacityLayer::with_blend(1.0, Offset::ZERO, BlendMode::Src)),
            Some([0, 0, 0, 0]),
        ),
        (
            "opacity Clear",
            Layer::from(OpacityLayer::with_blend(
                1.0,
                Offset::ZERO,
                BlendMode::Clear,
            )),
            Some([0, 0, 0, 0]),
        ),
        (
            "color filter Src",
            Layer::from(ColorFilterLayer::new(ColorFilter::Mode {
                color: Color::rgba(255, 0, 255, 255),
                blend_mode: BlendMode::Src,
            })),
            None,
        ),
    ];
    for (name, effect, far_from_the_child) in effects {
        let build = |present: bool| {
            backdrop_scene(&root, &background, &green_background, &|tree, root_id| {
                if !present {
                    return;
                }
                let boundary = tree.push_child(
                    root_id,
                    LayerNode::new(Layer::from(OffsetLayer::new(Offset::new(40.0, 40.0))))
                        .with_boundary(id(2), card.clone()),
                );
                let layer = tree.push_child(boundary, effect.clone());
                tree.push_child(
                    layer,
                    rect_picture(Rect::from_xywh(0.0, 0.0, 16.0, 16.0), Color::RED),
                );
            })
        };
        let (before, after) = (build(true), build(false));
        let drawn = full_frame_pixels(&renderer, &before);
        if let Some(expected) = far_from_the_child {
            assert_eq!(
                px(&drawn, 100, 100),
                expected,
                "{name}: precondition: the composite replaced the viewport"
            );
        }
        let (partial, full) =
            damaged_and_full(&renderer, &before, &after, |plan| plan != FramePlan::Skip);
        let stale = mismatches(&partial, &full, 0);
        assert!(
            stale.is_empty(),
            "{name}: stale pixels at {stale:?}; the frame drawn had {:?} at (10, 10), \
             {:?} at (44, 44), {:?} at (100, 100)",
            px(&drawn, 10, 10),
            px(&drawn, 44, 44),
            px(&drawn, 100, 100)
        );
    }
}

/// An effect layer in the layer tree composites its whole region with the
/// mode it records, cut by its ancestors' clips.
///
/// - A half-opaque `Src` opacity layer replaces the viewport with half its
///   content: half-opaque red under the child, transparent elsewhere.
/// - An opaque `DstOver` opacity layer keeps the backdrop on top of its
///   child; an opacity layer that skipped the group would paint red.
/// - A shader mask in its default `Modulate` mode under a rect clip
///   composites its masked child `SrcOver`: the child shows unmultiplied by
///   the backdrop, the backdrop stays where the child left the mask's bounds
///   transparent, and nothing changes outside the clip. The mask's mode
///   belongs between its shader and its child; applied again at the
///   composite it would multiply the child by the backdrop and erase the
///   backdrop around it.
/// - A shader mask inside a translucent opacity layer composites under the
///   clip it was queued with, not over the opacity layer's whole region.
#[test]
fn an_effect_layer_composites_its_whole_region_with_its_mode() {
    use flui_layer::{ClipRectLayer, OpacityLayer, ShaderMaskLayer};
    use flui_painting::paint::Shader;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background, card) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let red_child = || rect_picture(Rect::from_xywh(0.0, 0.0, 16.0, 16.0), Color::RED);
    let boundary = |tree: &mut LayerTree, root_id, at: Offset<f64>| {
        tree.push_child(
            root_id,
            LayerNode::new(Layer::from(OffsetLayer::new(at))).with_boundary(id(2), card.clone()),
        )
    };
    let opacity = |alpha: f64, blend: BlendMode| {
        move |tree: &mut LayerTree, root_id| {
            let card = boundary(tree, root_id, Offset::new(40.0, 40.0));
            let layer = tree.push_child(
                card,
                Layer::from(OpacityLayer::with_blend(alpha, Offset::ZERO, blend)),
            );
            tree.push_child(layer, red_child());
        }
    };
    let src_opacity = opacity(0.5, BlendMode::Src);
    let dst_over_opacity = opacity(1.0, BlendMode::DstOver);
    // Device: the clip is (20, 20)-(50, 50), the mask (20, 20)-(80, 80), the
    // child (20, 20)-(36, 36).
    let clipped_mask = |tree: &mut LayerTree, root_id| {
        let card = boundary(tree, root_id, Offset::new(20.0, 20.0));
        let clipped = tree.push_child(
            card,
            Layer::from(ClipRectLayer::hard_edge(Rect::from_xywh(
                0.0, 0.0, 30.0, 30.0,
            ))),
        );
        let mask = tree.push_child(
            clipped,
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::WHITE),
                BlendMode::Modulate,
                Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
            )),
        );
        tree.push_child(mask, red_child());
    };
    // Device: the opacity layer holds a clip of (20, 20)-(50, 50) and a mask
    // of (20, 20)-(80, 80) whose child fills it.
    let mask_in_opacity = |tree: &mut LayerTree, root_id| {
        let card = boundary(tree, root_id, Offset::new(20.0, 20.0));
        let faded = tree.push_child(card, Layer::from(OpacityLayer::new(0.5)));
        let clipped = tree.push_child(
            faded,
            Layer::from(ClipRectLayer::hard_edge(Rect::from_xywh(
                0.0, 0.0, 30.0, 30.0,
            ))),
        );
        let mask = tree.push_child(
            clipped,
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::WHITE),
                BlendMode::SrcOver,
                Rect::from_xywh(0.0, 0.0, 60.0, 60.0),
            )),
        );
        tree.push_child(
            mask,
            rect_picture(Rect::from_xywh(0.0, 0.0, 60.0, 60.0), Color::RED),
        );
    };
    // Name, the layers the row adds under the root, the samples it must read.
    type Row<'a> = (
        &'static str,
        &'a dyn Fn(&mut LayerTree, flui_layer::LayerId),
        &'static [((u32, u32), [u8; 4])],
    );
    let rows: [Row<'_>; 4] = [
        (
            "half-opaque Src opacity layer",
            &src_opacity,
            &[((44, 44), [128, 0, 0, 128]), ((100, 100), [0, 0, 0, 0])],
        ),
        (
            "opaque DstOver opacity layer",
            &dst_over_opacity,
            &[((44, 44), GREEN), ((100, 100), GREEN)],
        ),
        (
            "Modulate shader mask under a clip",
            &clipped_mask,
            &[
                ((24, 24), RED),
                ((45, 45), GREEN),
                ((70, 70), GREEN),
                ((10, 10), GREEN),
            ],
        ),
        (
            "shader mask under a clip inside an opacity layer",
            &mask_in_opacity,
            &[((30, 30), [128, 127, 0, 255]), ((70, 70), GREEN)],
        ),
    ];
    let mut failed = Vec::new();
    for (name, extra, samples) in rows {
        let drawn = full_frame_pixels(
            &renderer,
            &backdrop_scene(&root, &background, &green_background, extra),
        );
        for &((x, y), expected) in samples {
            let got = px(&drawn, x, y);
            if !near(got, expected, 2) {
                failed.push(format!(
                    "{name}: ({x}, {y}) is {got:?}, expected {expected:?}"
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A change in the halo of a foreground blur under a shrinking transform
/// presents the same pixels as a full frame. The renderer blurs with the
/// filter's sigma in physical pixels whatever the transform, so its halo
/// reaches `ceil(sqrt(3) x sigma)` pixels even where the transform shrinks
/// the content to a quarter, and damage there must take in the blur.
#[test]
fn a_change_in_a_shrunk_blurs_halo_matches_a_full_frame() {
    use flui_layer::ImageFilterLayer;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background, blurred, moving) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let build = |at: Offset<f64>| {
        backdrop_scene(&root, &background, &green_background, &|tree, root_id| {
            let boundary = tree.push_child(
                root_id,
                LayerNode::new(Layer::from(TransformLayer::new(Matrix4::scaling(
                    0.25, 0.25, 1.0,
                ))))
                .with_boundary(id(4), blurred.clone()),
            );
            let filter = tree.push_child(boundary, Layer::from(ImageFilterLayer::blur(8.0)));
            // Device rect (40, 40)-(70, 70).
            tree.push_child(
                filter,
                rect_picture(Rect::from_xywh(160.0, 160.0, 120.0, 120.0), Color::RED),
            );
            let mover = tree.push_child(
                root_id,
                LayerNode::new(Layer::from(OffsetLayer::new(at)))
                    .with_boundary(id(2), moving.clone()),
            );
            tree.push_child(
                mover,
                rect_picture(Rect::from_xywh(0.0, 0.0, 4.0, 4.0), Color::BLUE),
            );
        })
    };
    // Beside the blur, 8 to 12 px past its edge: inside the renderer's
    // 14 px halo, outside a quarter-scaled three-sigma reach of 6 px.
    let (before, after) = (
        build(Offset::new(78.0, 50.0)),
        build(Offset::new(78.0, 56.0)),
    );
    let full_before = full_frame_pixels(&renderer, &before);
    assert_ne!(
        px(&full_before, 77, 62),
        GREEN,
        "precondition: the halo reaches past the quarter-scaled reach"
    );
    let (partial, full) = partial_and_full(&renderer, &before, &after);
    let stale = mismatches(&partial, &full, 2);
    assert!(stale.is_empty(), "stale pixels at {stale:?}");
}

/// An unchanged layer that composites over the whole viewport (an opacity
/// layer whose blend changes what a transparent source covers, a colour
/// filter or an image filter that paints transparent pixels, alone or inside
/// a composition) matches a full frame when an
/// unrelated boundary elsewhere changes. The layer's child is recorded under
/// the damage scissor but its result composites over the viewport, so a
/// partial frame would composite a truncated input over retained pixels
/// that already hold its result, and over a later sibling it would wipe.
///
/// A shader mask, which composites an offscreen of its own, keeps its
/// composite inside the damage scissor the same way: a translucent one
/// composited past it blends a second time over retained pixels that
/// already hold its result.
#[test]
fn a_change_beside_a_viewport_compositing_layer_matches_a_full_frame() {
    use flui_layer::{
        ClipRectLayer, ColorFilterLayer, ImageFilterLayer, OpacityLayer, ShaderMaskLayer,
    };
    use flui_painting::paint::effects::{ColorAdjustment, ColorMatrix};
    use flui_painting::paint::{ColorFilter, Shader};

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    // Identity plus an offset that turns transparent black into half-opaque
    // red.
    let mut offset = ColorMatrix::identity();
    offset.values[4] = 1.0;
    offset.values[19] = 0.5;
    let (root, background, card, later, moving) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let effects = [
        (
            "opacity Src",
            vec![Layer::from(OpacityLayer::with_blend(
                0.5,
                Offset::ZERO,
                BlendMode::Src,
            ))],
        ),
        (
            "opacity Clear",
            vec![Layer::from(OpacityLayer::with_blend(
                1.0,
                Offset::ZERO,
                BlendMode::Clear,
            ))],
        ),
        (
            "color filter SrcOver",
            vec![Layer::from(ColorFilterLayer::new(ColorFilter::Mode {
                color: Color::rgba(255, 0, 255, 128),
                blend_mode: BlendMode::SrcOver,
            }))],
        ),
        (
            "color filter Src",
            vec![Layer::from(ColorFilterLayer::new(ColorFilter::Mode {
                color: Color::rgba(255, 0, 255, 128),
                blend_mode: BlendMode::Src,
            }))],
        ),
        (
            "image filter matrix",
            vec![Layer::from(ImageFilterLayer::matrix(offset))],
        ),
        (
            "image filter colour adjustment",
            vec![Layer::from(ImageFilterLayer::new(
                ImageFilter::ColorAdjust(ColorAdjustment::Matrix(offset)),
            ))],
        ),
        (
            "image filter composing a matrix",
            vec![Layer::from(ImageFilterLayer::new(ImageFilter::Compose(
                vec![ImageFilter::blur(1.0), ImageFilter::Matrix(offset)],
            )))],
        ),
        (
            "Modulate shader mask under a clip",
            vec![
                Layer::from(ClipRectLayer::hard_edge(Rect::from_xywh(
                    0.0, 0.0, 30.0, 30.0,
                ))),
                Layer::from(ShaderMaskLayer::new(
                    Shader::solid(Color::WHITE),
                    BlendMode::Modulate,
                    Rect::from_xywh(-20.0, -20.0, 60.0, 60.0),
                )),
            ],
        ),
        (
            "translucent SrcOver shader mask",
            vec![Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::rgba(255, 255, 255, 128)),
                BlendMode::SrcOver,
                Rect::from_xywh(0.0, 0.0, 30.0, 30.0),
            ))],
        ),
    ];
    let mut failed = Vec::new();
    for (name, chain) in effects {
        let build = |at: Offset<f64>| {
            backdrop_scene(&root, &background, &green_background, &|tree, root_id| {
                let boundary = tree.push_child(
                    root_id,
                    LayerNode::new(Layer::from(OffsetLayer::new(Offset::new(40.0, 40.0))))
                        .with_boundary(id(2), card.clone()),
                );
                let layer = chain.iter().fold(boundary, |parent, effect| {
                    tree.push_child(parent, effect.clone())
                });
                tree.push_child(
                    layer,
                    rect_picture(Rect::from_xywh(0.0, 0.0, 16.0, 16.0), Color::RED),
                );
                let after = tree.push_child(
                    root_id,
                    LayerNode::new(Layer::from(OffsetLayer::new(Offset::new(90.0, 90.0))))
                        .with_boundary(id(5), later.clone()),
                );
                tree.push_child(
                    after,
                    rect_picture(Rect::from_xywh(0.0, 0.0, 16.0, 16.0), Color::BLUE),
                );
                let mover = tree.push_child(
                    root_id,
                    LayerNode::new(Layer::from(OffsetLayer::new(at)))
                        .with_boundary(id(4), moving.clone()),
                );
                tree.push_child(
                    mover,
                    rect_picture(Rect::from_xywh(0.0, 0.0, 4.0, 4.0), Color::BLUE),
                );
            })
        };
        let (before, after) = (
            build(Offset::new(10.0, 100.0)),
            build(Offset::new(10.0, 104.0)),
        );
        let (partial, full) =
            damaged_and_full(&renderer, &before, &after, |plan| plan != FramePlan::Skip);
        let stale = mismatches(&partial, &full, 0);
        if !stale.is_empty() {
            failed.push(format!("{name}: stale pixels at {stale:?}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A performance overlay narrower than its readouts shows the same pixels
/// after its numbers change as a full frame does: its ink stays inside the
/// bounds its damage covers, rather than leaving the previous frame's
/// numbers standing past them.
#[test]
fn a_changed_undersized_performance_overlay_matches_a_full_frame() {
    use flui_layer::PerformanceOverlayLayer;

    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (root, background) = (ContentToken::mint(), ContentToken::mint());
    let text = std::cell::RefCell::new(flui_painting::TextContext::new(
        &flui_painting::FontCollection::new(),
    ));
    let build = |fps: f64, frame_time_ms: f64| {
        backdrop_scene(&root, &background, &green_background, &|tree, root_id| {
            // The labels start 8 px in, the values 50 px in: a 30 px wide
            // overlay records the values outside its bounds, which the
            // engine's clip keeps from inking.
            let overlay = PerformanceOverlayLayer::record(
                &mut text.borrow_mut(),
                Rect::from_xywh(10.0, 10.0, 30.0, 40.0),
                flui_layer::PerformanceOverlayOption::all(),
                &flui_layer::PerformanceSample {
                    fps,
                    frame_time_ms,
                    diagnostic_line: None,
                },
            );
            tree.push_child(root_id, Layer::from(overlay));
        })
    };
    let (before, after) = (build(10.0, 88.8), build(99.0, 11.1));
    let (partial, full) = partial_and_full(&renderer, &before, &after);
    let stale = mismatches(&partial, &full, 0);
    assert!(stale.is_empty(), "stale pixels at {stale:?}");
}

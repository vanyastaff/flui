use std::sync::Arc;

use flui_foundation::geometry::Rect;
use flui_painting::BlendMode;

use super::WgpuPainter;

/// Headless GPU device + queue for painter tests.
fn test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
    crate::test_support::test_device_and_queue("Painter Test Device")
}

// ===== Color-readback helpers (BUG 1/2/3 regression tests) =====

/// Format used for all color-readback tests: plain UNorm so the stored bytes
/// equal the sRGB-encoded bytes the shader emits 1:1 (no OETF on store),
/// matching the production surface format chosen by `select_surface_format`
/// and the usual onscreen convention.
const READBACK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Render `draw` into a `size`×`size` UNorm target cleared to `clear`, then
/// return the tightly-packed RGBA bytes (`size*size*4`, row stride
/// `size*4`). Use [`pixel_at`] to sample an individual texel. Every pixel is
/// exposed so edge/column sampling (e.g. atlas-bleed checks) is possible.
fn render_to_rgba(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    size: u32,
    clear: wgpu::Color,
    draw: impl FnOnce(&mut WgpuPainter),
) -> Vec<u8> {
    let (target, target_view) = crate::test_support::create_target(
        device,
        "readback target",
        size,
        size,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );

    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(device),
        Arc::clone(queue),
        READBACK_FORMAT,
        (size, size),
    );

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("readback clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    painter.begin_frame().expect("public painter frame begins");
    draw(&mut painter);
    painter
        .render_to_view(&target_view, &mut encoder)
        .expect("painter.render must succeed for readback");
    painter
        .submit_encoder(encoder)
        .expect("managed painter submission");
    painter.finish_frame();

    crate::test_support::readback_bytes(device, queue, &target, size, size)
}

/// Sample one RGBA texel from a tightly-packed buffer produced by
/// [`render_to_rgba`].
fn pixel_at(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
    let off = (y as usize * size as usize + x as usize) * 4;
    [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
}

/// BUG 3 sharpness: a 2×1 image drawn exactly 1:1 must sample each texel
/// with its own color, not a blend with its neighbor.
///
/// With the (wrong) half-texel UV inset, `min_u` for a 2-wide image in a
/// 2048-wide atlas shifts right by `0.5/2048 ≈ 0.000244`, and `max_u`
/// shifts left symmetrically.  The left screen pixel's UV maps to roughly
/// `texel 0.25` (mix of 75% RED + 25% GREEN) rather than `texel 0.5` (pure
/// RED); the right pixel maps to roughly `texel 1.75` (25% RED + 75% GREEN)
/// rather than `texel 1.5` (pure GREEN).  The RED and GREEN channels would
/// both read ~191 instead of 255/0.
///
/// With exact texel-boundary UVs and a transparent gutter: left pixel maps to
/// `texel 0.5` → pure RED; right pixel maps to `texel 1.5` → pure GREEN.
fn atlas_image_is_sharp_at_one_to_one() {
    use flui_painting::paint::Image;

    // 2-pixel wide, 1-pixel tall render target (drawn 1:1).
    const W: u32 = 2;
    const H: u32 = 1;
    let (device, queue) = test_device_and_queue();

    // Left texel = RED, right texel = GREEN.
    let pixels: Vec<u8> = vec![
        255, 0, 0, 255, // left pixel: RED
        0, 255, 0, 255, // right pixel: GREEN
    ];
    let img = Image::from_rgba8(W, H, pixels);

    let rgba = render_to_rgba(&device, &queue, W, wgpu::Color::BLACK, |painter| {
        painter.draw_image(
            &img,
            Rect::from_xywh(0.0, 0.0, 2.0, 1.0),
            flui_painting::BlendMode::SrcOver,
        );
    });

    let left = pixel_at(&rgba, W, 0, 0);
    let right = pixel_at(&rgba, W, 1, 0);
    let (lr, lg) = (i32::from(left[0]), i32::from(left[1]));
    let (rr, rg) = (i32::from(right[0]), i32::from(right[1]));
    assert!(
        lr > 200 && lg < 55,
        "left pixel = (R={lr}, G={lg}): expected RED (~255,~0). \
             A blended value means uv_coords has a half-texel inset that \
             shifts sampling away from the texel center."
    );
    assert!(
        rg > 200 && rr < 55,
        "right pixel = (R={rr}, G={rg}): expected GREEN (~0,~255). \
             A blended value means uv_coords has a half-texel inset that \
             shifts sampling away from the texel center."
    );
}

// ===== Phase A per-draw blend mode (fixed-function Porter-Duff) =====
//
// These tests exercise the TESSELLATED path: a filled `Path` and stroked
// shapes always tessellate (never the instanced fast path), so they go
// through `pipeline_key_from_paint` → the blend pipeline keyed by
// `PipelineKey::blend_mode`. All values are premultiplied-alpha results on
// the UNorm readback target.

// ===== Shape-batcher characterisation readback safety net =====
//
// Locks down the relocated batcher slow path (non-axis-aligned rect with a
// non-SrcOver blend mode).  A regression in the moved branch would pass the
// instanced-path tests but silently break the tessellated-path segment seal.

// ===== T6 Characterisation readback safety-net =====
//
// These tests lock down the SDF-clip baking (clip_rrect / clip_rsuperellipse
// corner cutouts) and the nested save+clip+restore scissor restoration that
// had ZERO pixel coverage before T6. They are characterisation tests: they
// pass on the current (correct) code and will FAIL if the GpuStateStack
// extraction) breaks clip/scissor behaviour.
//
// Each test discriminates: the assertion would fail if the clip were a plain
// axis-aligned square (no SDF applied) or if save/restore leaked the scissor.

/// An offscreen result composited with a blend mode other than `SrcOver`
/// must actually use that mode.
///
/// `ShaderMaskLayer` carries its own `blend_mode()`; before this test the
/// layer path accepted it, threaded it into `render_masked`, and that
/// function dropped it on the floor — every masked layer composited
/// `SrcOver` regardless of what the caller asked for. The mode now rides on
/// `DrawItem::OffscreenTexture` and selects the composite pipeline, so a
/// `Clear` result erases what is under it rather than drawing over it.
///
/// The scene: an opaque red frame, then a full-surface offscreen result
/// composited with `Clear`. `Clear` ignores the source entirely and writes
/// zero, so the centre must come back transparent. `SrcOver` (the pre-fix
/// behaviour) would leave red.
fn an_offscreen_result_composites_with_its_own_blend_mode() {
    clear_offscreen_result(false);
    clear_offscreen_result(true);
}

fn clear_offscreen_result(nested: bool) {
    use flui_painting::Paint;

    const SIZE: u32 = 64;
    let (device, queue) = test_device_and_queue();

    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        if nested {
            painter.save_layer(
                None,
                &Paint::fill(flui_painting::styling::Color::rgba(255, 255, 255, 128)),
            );
        }
        // Step 1: opaque red, so the frame has something to erase.
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            &Paint::fill(flui_painting::styling::Color::rgb(255, 0, 0)),
        );

        // Step 2: a transparent offscreen texture, composited with Clear.
        // Clear must erase even when the source has zero alpha; discarding
        // transparent fragments would incorrectly preserve the destination.
        let mut pool = crate::texture_pool::TexturePool::new(Arc::clone(&device));
        let texture = pool.acquire(SIZE, SIZE, READBACK_FORMAT);
        {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("clear-composite source"),
            });
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear-composite source fill"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: texture.view(),
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            queue.submit(std::iter::once(encoder.finish()));
        }

        painter.queue_offscreen_result(
            texture,
            Rect::from_xywh(0.0, 0.0, f64::from(SIZE as f32), f64::from(SIZE as f32)),
            BlendMode::Clear,
        );
        if nested {
            painter.restore_layer();
        }
    });

    let center = pixel_at(&rgba, SIZE, SIZE / 2, SIZE / 2);
    assert_eq!(
        center,
        [0, 0, 0, if nested { 255 } else { 0 }],
        "centre pixel = {center:?}; a Clear composite of a transparent offscreen \
         must erase the red content (nested={nested})."
    );
}

/// Painter readbacks: an atlas image is sharp at 1:1, and an offscreen result
/// composites with its own blend mode.
#[test]
fn painter_images_and_offscreen_results_read_back_as_specified() {
    let cases: [(&str, fn()); 26] = [
        (
            "limited fragment uniforms clip recovery",
            limited_fragment_uniforms_clip_refusal_recovers,
        ),
        (
            "featureless direct AA refusal recovery",
            featureless_direct_aa_refusal_recovers,
        ),
        (
            "external completion after repeated submit",
            external_repeated_submission_keeps_allocation,
        ),
        (
            "external competing recording failures",
            external_competing_recording_failures,
        ),
        (
            "external nested opacity allocation",
            external_nested_opacity_preserves_recorded_allocation,
        ),
        (
            "external resource sampling update",
            external_resource_sampling_survives_update,
        ),
        (
            "external straight linear filtering has no halo",
            external_straight_linear_filtering_has_no_halo,
        ),
        (
            "external alpha and opacity",
            external_alpha_contract_scales_opacity,
        ),
        (
            "external invalid imports preserve old draw",
            external_invalid_registration_and_update_preserve_old_resource,
        ),
        (
            "external missing ID next frame",
            external_missing_id_and_unregister_recover,
        ),
        (
            "external latest same allocation",
            external_contents_are_live_within_the_same_allocation,
        ),
        (
            "external per-draw sampling",
            external_draw_sampling_overrides_registration,
        ),
        (
            "external recorded allocation",
            external_recorded_draw_survives_update_and_rebind,
        ),
        (
            "tess adjacent merge boundaries",
            tess_merge_preserves_order_and_clip_boundaries,
        ),
        (
            "frame submission limit recovery",
            cumulative_frame_submit_limit_survives_gpu_retirement,
        ),
        (
            "resize offscreen-only",
            offscreen_only_flushes_freeze_viewport_after_resize,
        ),
        (
            "recording quota shared by layers",
            recording_quota_is_shared_by_isolated_layers,
        ),
        (
            "recording clone peak",
            recording_quota_charges_filter_remap_clone,
        ),
        (
            "recording admission recovery",
            recording_quota_refuses_then_next_frame_renders,
        ),
        (
            "cached image scissor changes",
            cached_image_draws_keep_their_own_scissors,
        ),
        (
            "frozen viewport between flushes",
            viewport_bindings_survive_resize_before_submit,
        ),
        ("atlas sharpness", atlas_image_is_sharp_at_one_to_one),
        (
            "transient image lifetime",
            transient_images_keep_their_own_pixels,
        ),
        (
            "filtered image dimensions",
            filtered_images_keep_their_dimensions,
        ),
        (
            "low-alpha images",
            low_alpha_images_preserve_their_contribution,
        ),
        (
            "offscreen blend",
            an_offscreen_result_composites_with_its_own_blend_mode,
        ),
    ];
    let mut failures = Vec::new();
    for (name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "painter image rows failed: {failures:?}"
    );
}

fn low_alpha_images_preserve_their_contribution() {
    use flui_painting::paint::Image;

    const SIZE: u32 = 4;
    let (device, queue) = test_device_and_queue();
    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::WHITE, |painter| {
        for alpha in 1..=3u8 {
            let image = Image::from_rgba8(1, 1, vec![255, 0, 0, alpha]);
            painter.draw_image(
                &image,
                Rect::from_xywh(f64::from(alpha - 1), 0.0, 1.0, 1.0),
                BlendMode::SrcOver,
            );
        }
    });
    for alpha in 1..=3u8 {
        assert_eq!(
            pixel_at(&rgba, SIZE, u32::from(alpha - 1), 0),
            [255, 255 - alpha, 255 - alpha, 255],
            "alpha {alpha}/255 must contribute to the destination"
        );
    }
}

/// Equal bytes can describe different row layouts. A CPU filter must not make
/// the vertical image reuse the horizontal image's uploaded texture.
fn filtered_images_keep_their_dimensions() {
    use flui_painting::paint::{Image, image::ColorFilter};

    const SIZE: u32 = 4;
    let (device, queue) = test_device_and_queue();
    let pixels = vec![255, 0, 0, 255, 0, 255, 0, 255];
    let horizontal = Image::from_rgba8(2, 1, pixels.clone());
    let vertical = Image::from_rgba8(1, 2, pixels);
    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        for (image, dst) in [
            (&horizontal, Rect::from_xywh(0.0, 0.0, 2.0, 1.0)),
            (&vertical, Rect::from_xywh(3.0, 0.0, 1.0, 2.0)),
        ] {
            painter.draw_image_filtered(
                image,
                dst,
                ColorFilter::Mode {
                    color: flui_painting::styling::Color::TRANSPARENT,
                    blend_mode: BlendMode::Dst,
                },
                BlendMode::SrcOver,
            );
        }
    });
    assert_eq!(pixel_at(&rgba, SIZE, 1, 0), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&rgba, SIZE, 3, 1), [0, 255, 0, 255]);
}

/// Recording may outlive the caller's image handle. Each short-lived image
/// must still sample its own pixels when the accumulated commands replay.
fn transient_images_keep_their_own_pixels() {
    use flui_painting::paint::Image;

    const SIZE: u32 = 16;
    let (device, queue) = test_device_and_queue();
    let rgba = render_to_rgba(&device, &queue, SIZE, wgpu::Color::BLACK, |painter| {
        for index in 0..SIZE * SIZE {
            let image = Image::from_rgba8(1, 1, vec![index as u8, 255, 0, 255]);
            painter.draw_image(
                &image,
                Rect::from_xywh(f64::from(index % SIZE), f64::from(index / SIZE), 1.0, 1.0),
                BlendMode::SrcOver,
            );
        }
    });
    for index in 0..SIZE * SIZE {
        assert_eq!(
            pixel_at(&rgba, SIZE, index % SIZE, index / SIZE),
            [index as u8, 255, 0, 255],
            "image {index} must survive its caller dropping the CPU handle"
        );
    }
}

fn viewport_bindings_survive_resize_before_submit() {
    use flui_painting::{Paint, styling::Color};
    let (device, queue) = test_device_and_queue();
    let (first, first_view) = crate::test_support::create_target(
        &device,
        "first viewport",
        64,
        64,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let (second, second_view) = crate::test_support::create_target(
        &device,
        "second viewport",
        128,
        128,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    crate::test_support::clear_target(&device, &queue, &first_view, wgpu::Color::BLACK);
    crate::test_support::clear_target(&device, &queue, &second_view, wgpu::Color::BLACK);
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (64, 64),
    );
    painter.begin_frame().expect("painter frame must begin");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter.draw_rect(
        Rect::from_xywh(40.0, 8.0, 16.0, 16.0),
        &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
    );
    painter
        .render_to_view(&first_view, &mut encoder)
        .expect("first viewport encodes");
    painter.resize(128, 128);
    painter.draw_rect(
        Rect::from_xywh(80.0, 16.0, 32.0, 32.0),
        &Paint::fill(Color::rgb(0, 0, 255)).with_anti_alias(false),
    );
    painter
        .render_to_view(&second_view, &mut encoder)
        .expect("second viewport encodes");
    painter
        .submit_encoder(encoder)
        .expect("both viewport flushes submit together");
    painter.finish_frame();
    let first_pixels = crate::test_support::readback_bytes(&device, &queue, &first, 64, 64);
    let second_pixels = crate::test_support::readback_bytes(&device, &queue, &second, 128, 128);
    assert_eq!(
        pixel_at(&first_pixels, 64, 48, 16),
        [255, 0, 0, 255],
        "first flush retains 64px viewport after resize"
    );
    assert_eq!(
        pixel_at(&first_pixels, 64, 24, 8),
        [0, 0, 0, 255],
        "first geometry must not shrink under later viewport"
    );
    assert_eq!(
        pixel_at(&second_pixels, 128, 96, 32),
        [0, 0, 255, 255],
        "second flush uses resized viewport"
    );
    assert_eq!(
        pixel_at(&second_pixels, 128, 48, 16),
        [0, 0, 0, 255],
        "second geometry stays offcenter"
    );
}

fn cached_image_draws_keep_their_own_scissors() {
    use flui_painting::paint::{Clip, Image};
    let (device, queue) = test_device_and_queue();
    // A large solid source keeps sample points far from atlas gutter filtering.
    // A 1x1 source mixes its single texel with transparent gutter under linear sampling.
    let image = Image::from_rgba8(32, 32, [255, 0, 0, 255].repeat(32 * 32));
    let pixels = render_to_rgba(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        for clip in [
            Rect::from_xywh(0.0, 0.0, 24.0, 64.0),
            Rect::from_xywh(40.0, 0.0, 24.0, 64.0),
        ] {
            painter.save();
            painter.clip_rect(clip, Clip::HardEdge);
            painter.draw_image(
                &image,
                Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
                BlendMode::SrcOver,
            );
            painter.restore();
        }
    });
    assert_eq!(
        pixel_at(&pixels, 64, 8, 32),
        [255, 0, 0, 255],
        "first draw retains left scissor instead of last right scissor"
    );
    assert_eq!(
        pixel_at(&pixels, 64, 56, 32),
        [255, 0, 0, 255],
        "second draw fills right scissor"
    );
    assert_eq!(
        pixel_at(&pixels, 64, 32, 32),
        [0, 0, 0, 255],
        "neither draw may fill the gap between scissors"
    );
}

fn recording_quota_case(bytes: usize, elements: usize, draw: fn(&mut WgpuPainter)) {
    recording_failure_case(bytes, elements, false, draw);
}

fn recording_failure_case(
    bytes: usize,
    elements: usize,
    external_first: bool,
    draw: fn(&mut WgpuPainter),
) {
    use flui_painting::{Paint, styling::Color};
    let (device, queue) = test_device_and_queue();
    let (target, view) = crate::test_support::create_target(
        &device,
        "recording recovery",
        64,
        64,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLACK);
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (64, 64),
    );
    painter.begin_frame().expect("painter frame must begin");
    // Private admission seam, but recording/rendering are the consumer's public API.
    painter.current_segment = crate::command_ir::DrawSegment::with_budget(
        crate::recording_budget::RecordingBudget::new(bytes, elements),
    );
    draw(&mut painter);
    let mut failed = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let error = painter
        .render_to_view(&view, &mut failed)
        .expect_err("recording refusal reaches render");
    if external_first {
        assert!(
            matches!(
                error,
                crate::EngineError::ExternalTexture(crate::ExternalTextureError::UnknownTexture {
                    id: 99
                })
            ),
            "first missing resource remains authoritative: {error:?}"
        );
    } else {
        assert!(
            matches!(error, crate::EngineError::PreparedResourceLimit { .. }),
            "first quota refusal remains authoritative: {error:?}"
        );
    }
    drop(failed);
    painter.finish_frame();
    let unchanged = crate::test_support::readback_bytes(&device, &queue, &target, 64, 64);
    assert_eq!(
        pixel_at(&unchanged, 64, 32, 32),
        [0, 0, 0, 255],
        "failed unsubmitted frame leaves target untouched"
    );
    painter.begin_frame().expect("painter frame must begin");
    painter.draw_rect(
        Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
        &Paint::fill(Color::rgb(0, 255, 0)).with_anti_alias(false),
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_view(&view, &mut encoder)
        .expect("next admitted frame encodes");
    painter
        .submit_encoder(encoder)
        .expect("next admitted frame submits");
    painter.finish_frame();
    let pixels = crate::test_support::readback_bytes(&device, &queue, &target, 64, 64);
    assert_eq!(
        pixel_at(&pixels, 64, 32, 32),
        [0, 255, 0, 255],
        "next frame makes visible progress after quota refusal"
    );
}

fn recording_quota_refuses_then_next_frame_renders() {
    recording_quota_case(0, 0, |painter| {
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &flui_painting::Paint::fill(flui_painting::styling::Color::RED),
        );
    });
}
fn recording_quota_is_shared_by_isolated_layers() {
    recording_quota_case(1024 * 1024, 3, |painter| {
        use flui_painting::{Paint, styling::Color};
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 24.0, 64.0),
            &Paint::fill(Color::RED).with_anti_alias(false),
        );
        painter.save_layer(None, &Paint::fill(Color::rgba(255, 255, 255, 128)));
        painter.draw_rect(
            Rect::from_xywh(40.0, 0.0, 24.0, 64.0),
            &Paint::fill(Color::BLUE).with_anti_alias(false),
        );
        painter.restore_layer();
    });
}
fn recording_quota_charges_filter_remap_clone() {
    recording_quota_case(1024 * 1024, 3, |painter| {
        use crate::layer_state_stack::LayerStateStack;
        use flui_painting::{Paint, paint::effects::ImageFilter, styling::Color};
        {
            let mut dispatcher = crate::layer_dispatcher::LayerDispatcher::new(painter);
            dispatcher.push_image_filter(&ImageFilter::blur(1.0));
        }
        painter.draw_rect(
            Rect::from_xywh(16.0, 16.0, 24.0, 24.0),
            &Paint::fill(Color::RED).with_anti_alias(false),
        );
        painter.restore_layer();
    });
}

fn offscreen_only_flushes_freeze_viewport_after_resize() {
    let (device, queue) = test_device_and_queue();
    let (first, first_view) = crate::test_support::create_target(
        &device,
        "first offscreen viewport",
        64,
        64,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let (second, second_view) = crate::test_support::create_target(
        &device,
        "second offscreen viewport",
        128,
        128,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    crate::test_support::clear_target(&device, &queue, &first_view, wgpu::Color::BLACK);
    crate::test_support::clear_target(&device, &queue, &second_view, wgpu::Color::BLACK);
    let mut pool = crate::texture_pool::TexturePool::new(Arc::clone(&device));
    let source = pool.acquire(32, 32, READBACK_FORMAT);
    let second_source = pool.acquire(32, 32, READBACK_FORMAT);
    crate::test_support::clear_target(&device, &queue, source.view(), wgpu::Color::RED);
    crate::test_support::clear_target(&device, &queue, second_source.view(), wgpu::Color::BLUE);
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (32, 32),
    );
    painter.begin_frame().expect("offscreen frame begins");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    // The crate-private enqueue seam is the same one shader masks use; it is
    // necessary to isolate a frame with no ordinary DrawSegment at all.
    painter.resize(64, 64);
    painter.queue_offscreen_result(
        source,
        Rect::from_xywh(40.0, 8.0, 16.0, 16.0),
        BlendMode::SrcOver,
    );
    painter
        .render_to_view(&first_view, &mut encoder)
        .expect("first offscreen-only flush encodes");
    painter.resize(128, 128);
    painter.queue_offscreen_result(
        second_source,
        Rect::from_xywh(80.0, 16.0, 32.0, 32.0),
        BlendMode::SrcOver,
    );
    painter
        .render_to_view(&second_view, &mut encoder)
        .expect("resized offscreen-only flush encodes");
    painter
        .submit_encoder(encoder)
        .expect("both offscreen flushes submit together");
    painter.finish_frame();
    let first = crate::test_support::readback_bytes(&device, &queue, &first, 64, 64);
    let second = crate::test_support::readback_bytes(&device, &queue, &second, 128, 128);
    assert_eq!(
        pixel_at(&first, 64, 48, 16),
        [255, 0, 0, 255],
        "first resized offscreen-only quad uses 64-pixel viewport"
    );
    assert_eq!(
        pixel_at(&first, 64, 24, 8),
        [0, 0, 0, 255],
        "later viewport must not rescale first quad"
    );
    assert_eq!(
        pixel_at(&second, 128, 96, 32),
        [0, 0, 255, 255],
        "second offscreen-only quad uses 128-pixel viewport"
    );
    assert_eq!(
        pixel_at(&second, 128, 48, 16),
        [0, 0, 0, 255],
        "stale 64-pixel viewport must not double second quad placement"
    );
}

fn cumulative_frame_submit_limit_survives_gpu_retirement() {
    let (device, queue) = test_device_and_queue();
    let domain = crate::device_domain::DeviceDomain::with_limits(
        Arc::clone(&device),
        Arc::clone(&queue),
        crate::device_domain::PreparedIrLimits {
            frame_submissions: 64,
            ..Default::default()
        },
    );
    let mut painter = WgpuPainter::with_domain(domain, READBACK_FORMAT, (64, 64));
    painter.begin_frame().expect("first frame begins");
    assert!(
        matches!(
            painter.begin_frame(),
            Err(crate::EngineError::FrameAlreadyActive)
        ),
        "nested begin cannot reset an active frame's submission allowance"
    );
    for _ in 0..64 {
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .submit_encoder(encoder)
            .expect("within the frame's cumulative submission limit");
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU retires each empty submission");
    }
    let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    assert!(
        matches!(
            painter.submit_encoder(encoder),
            Err(crate::EngineError::PreparedResourceLimit {
                resource: "frame submissions",
                ..
            })
        ),
        "completed GPU work must not reset the cumulative frame limit"
    );
    painter.finish_frame();
    painter
        .begin_frame()
        .expect("next frame starts after exhausted frame is finished");
    let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .submit_encoder(encoder)
        .expect("next frame gets a fresh allowance");
    painter.finish_frame();
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("recovered frame retires");
}

fn tess_quad(painter: &mut WgpuPainter, rect: Rect<f64>, color: flui_painting::styling::Color) {
    use flui_foundation::geometry::Point;
    painter.draw_vertices(
        &[
            Point::new(rect.left(), rect.top()),
            Point::new(rect.right(), rect.top()),
            Point::new(rect.right(), rect.bottom()),
            Point::new(rect.left(), rect.bottom()),
        ],
        None,
        None,
        &[0, 1, 2, 0, 2, 3],
        &flui_painting::Paint::fill(color).with_anti_alias(false),
    );
}

fn tess_merge_preserves_order_and_clip_boundaries() {
    use flui_foundation::geometry::RRect;
    use flui_painting::{Paint, paint::Clip, styling::Color};
    let (device, queue) = test_device_and_queue();
    // Two explicit quads need 20 geometry entries. The small admission seam
    // leaves room for one batch and one ordered run, so compatible adjacency
    // must render successfully without allocating another descriptor.
    let pixels = render_to_rgba(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        painter.current_segment = crate::command_ir::DrawSegment::with_budget(
            crate::recording_budget::RecordingBudget::new(1024 * 1024, 22),
        );
        tess_quad(painter, Rect::from_xywh(0.0, 0.0, 24.0, 64.0), Color::RED);
        tess_quad(
            painter,
            Rect::from_xywh(40.0, 0.0, 24.0, 64.0),
            Color::GREEN,
        );
    });
    assert_eq!(pixel_at(&pixels, 64, 12, 32), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&pixels, 64, 52, 32), [0, 255, 0, 255]);
    let pixels = render_to_rgba(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        tess_quad(painter, Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::RED);
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(Color::GREEN),
        );
        tess_quad(
            painter,
            Rect::from_xywh(16.0, 16.0, 32.0, 32.0),
            Color::BLUE,
        );
    });
    assert_eq!(pixel_at(&pixels, 64, 8, 32), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&pixels, 64, 32, 32), [0, 0, 255, 255]);
    let pixels = render_to_rgba(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        painter.save();
        painter.clip_rect(Rect::from_xywh(0.0, 0.0, 24.0, 64.0), Clip::HardEdge);
        tess_quad(painter, Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::RED);
        painter.restore();
        painter.clip_rect(Rect::from_xywh(40.0, 0.0, 24.0, 64.0), Clip::HardEdge);
        tess_quad(painter, Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN);
    });
    assert_eq!(pixel_at(&pixels, 64, 12, 32), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&pixels, 64, 32, 32), [0, 0, 0, 255]);
    assert_eq!(pixel_at(&pixels, 64, 52, 32), [0, 255, 0, 255]);
    let pixels = render_to_rgba(&device, &queue, 64, wgpu::Color::BLACK, |painter| {
        painter.save();
        painter.clip_rrect(
            RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), 0.0),
            Clip::AntiAlias,
        );
        tess_quad(painter, Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::RED);
        painter.restore();
        painter.clip_rrect(
            RRect::from_rect_circular(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), 24.0),
            Clip::AntiAlias,
        );
        tess_quad(painter, Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN);
    });
    assert_eq!(pixel_at(&pixels, 64, 2, 2), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&pixels, 64, 32, 32), [0, 255, 0, 255]);
}

fn external_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    external_pixels_in_format(device, queue, width, pixels, READBACK_FORMAT)
}

fn external_pixels_in_format(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    pixels: &[u8],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    let mut upload = pixels.to_vec();
    if format == wgpu::TextureFormat::Bgra8Unorm {
        for pixel in upload.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("External contract texels"),
        size: wgpu::Extent3d {
            width,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &upload,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture
}

fn external_draw_sampling_overrides_registration() {
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    for (quality, default_linear, expected) in [
        (FilterQuality::None, true, [255, 0, 0, 255]),
        (FilterQuality::Low, false, [183, 0, 72, 255]),
    ] {
        let texture = external_pixels(&device, &queue, 2, &[255, 0, 0, 255, 0, 0, 255, 255]);
        let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLACK, |painter| {
            painter
                .external_texture_registry_mut()
                .register(
                    TextureId::new(10),
                    texture,
                    external_descriptor(
                        crate::ExternalAlpha::Straight,
                        if default_linear {
                            crate::ExternalSampling::Linear
                        } else {
                            crate::ExternalSampling::Nearest
                        },
                    ),
                )
                .expect("valid external texture");
            painter.draw_texture(
                TextureId::new(10),
                Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                None,
                quality,
                1.0,
            );
        });
        let actual = pixel_at(&pixels, 32, 12, 16);
        for channel in 0..4 {
            assert!(
                (i32::from(actual[channel]) - expected[channel]).abs() <= 1,
                "{quality:?}: {actual:?}, expected {expected:?}"
            );
        }
    }
}

fn external_recorded_draw_survives_update_and_rebind() {
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    for rebind in [false, true] {
        let red = external_pixels(&device, &queue, 1, &[255, 0, 0, 255]);
        let blue = external_pixels(&device, &queue, 1, &[0, 0, 255, 255]);
        let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLACK, |painter| {
            painter
                .external_texture_registry_mut()
                .register(
                    TextureId::new(11),
                    red,
                    external_descriptor(
                        crate::ExternalAlpha::Straight,
                        crate::ExternalSampling::Linear,
                    ),
                )
                .expect("valid external texture");
            painter.draw_texture(
                TextureId::new(11),
                Rect::from_xywh(0.0, 0.0, 16.0, 32.0),
                None,
                FilterQuality::None,
                1.0,
            );
            let registry = painter.external_texture_registry_mut();
            if rebind {
                registry.unregister(TextureId::new(11));
                registry
                    .register(
                        TextureId::new(11),
                        blue,
                        external_descriptor(
                            crate::ExternalAlpha::Straight,
                            crate::ExternalSampling::Linear,
                        ),
                    )
                    .expect("explicit rebind");
            } else {
                registry
                    .update(TextureId::new(11), blue)
                    .expect("compatible update");
            }
            painter.draw_texture(
                TextureId::new(11),
                Rect::from_xywh(16.0, 0.0, 16.0, 32.0),
                None,
                FilterQuality::None,
                1.0,
            );
        });
        assert_eq!(
            pixel_at(&pixels, 32, 8, 16),
            [255, 0, 0, 255],
            "recorded A must survive allocation replacement (rebind={rebind})"
        );
        assert_eq!(
            pixel_at(&pixels, 32, 24, 16),
            [0, 0, 255, 255],
            "later draw resolves B"
        );
    }
}

fn external_descriptor(
    alpha: crate::ExternalAlpha,
    sampling: crate::ExternalSampling,
) -> crate::ExternalTextureDescriptor {
    crate::ExternalTextureDescriptor {
        sampling,
        alpha,
        color: crate::ExternalColorEncoding::EncodedSrgb,
    }
}

fn external_resource_sampling_survives_update() {
    use flui_painting::paint::TextureId;
    let (device, queue) = test_device_and_queue();
    for (sampling, expected) in [
        (crate::ExternalSampling::Nearest, [255, 0, 0, 255]),
        (crate::ExternalSampling::Linear, [183, 0, 72, 255]),
    ] {
        for update in [false, true] {
            let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLACK, |painter| {
                let registry = painter.external_texture_registry_mut();
                registry
                    .register(
                        TextureId::new(12),
                        external_pixels(&device, &queue, 2, &[255, 0, 0, 255, 0, 0, 255, 255]),
                        external_descriptor(crate::ExternalAlpha::Straight, sampling),
                    )
                    .expect("resource sampler registration");
                if update {
                    registry
                        .update(
                            TextureId::new(12),
                            external_pixels(&device, &queue, 2, &[255, 0, 0, 255, 0, 0, 255, 255]),
                        )
                        .expect("update preserves resource sampler");
                }
                painter.draw_texture_with_resource_sampling(
                    TextureId::new(12),
                    Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                    None,
                    1.0,
                );
            });
            assert_external_pixel(pixel_at(&pixels, 32, 12, 16), expected);
        }
    }
}

fn assert_external_pixel(actual: [u8; 4], expected: [u8; 4]) {
    for channel in 0..4 {
        assert!(
            (i32::from(actual[channel]) - i32::from(expected[channel])).abs() <= 1,
            "{actual:?}, expected {expected:?}"
        );
    }
}

fn external_alpha_contract_scales_opacity() {
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    for (alpha, source_alpha, opacity, expected) in [
        (
            crate::ExternalAlpha::Premultiplied,
            128,
            1.0,
            [128, 0, 127, 255],
        ),
        (
            crate::ExternalAlpha::Premultiplied,
            128,
            0.5,
            [64, 0, 191, 255],
        ),
        (crate::ExternalAlpha::Straight, 128, 1.0, [64, 0, 127, 255]),
        (crate::ExternalAlpha::Straight, 128, 0.5, [32, 0, 191, 255]),
        // Opaque metadata means source alpha is ignored, even if its byte is zero.
        (crate::ExternalAlpha::Opaque, 0, 1.0, [128, 0, 0, 255]),
        (crate::ExternalAlpha::Opaque, 0, 0.5, [64, 0, 128, 255]),
    ] {
        let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLUE, |painter| {
            painter
                .external_texture_registry_mut()
                .register(
                    TextureId::new(13),
                    external_pixels(&device, &queue, 1, &[128, 0, 0, source_alpha]),
                    external_descriptor(alpha, crate::ExternalSampling::Nearest),
                )
                .expect("alpha source registration");
            painter.draw_texture(
                TextureId::new(13),
                Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                None,
                FilterQuality::None,
                opacity,
            );
        });
        assert_external_pixel(pixel_at(&pixels, 32, 16, 16), expected);
    }
}

fn external_invalid_registration_and_update_preserve_old_resource() {
    use crate::{EngineError, ExternalTextureError};
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    let descriptor = external_descriptor(
        crate::ExternalAlpha::Straight,
        crate::ExternalSampling::Nearest,
    );
    let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLACK, |painter| {
        let registry = painter.external_texture_registry_mut();
        registry
            .register(
                TextureId::new(14),
                external_pixels(&device, &queue, 2, &[255, 0, 0, 255, 0, 0, 255, 255]),
                descriptor,
            )
            .expect("original crop source");
        assert!(matches!(
            registry.register(
                TextureId::new(14),
                external_pixels(&device, &queue, 2, &[0, 0, 255, 255, 0, 0, 255, 255]),
                descriptor
            ),
            Err(EngineError::ExternalTexture(
                ExternalTextureError::DuplicateTexture { .. }
            ))
        ));
        assert!(matches!(
            registry.update(
                TextureId::new(14),
                external_pixels(&device, &queue, 1, &[0, 0, 255, 255])
            ),
            Err(EngineError::ExternalTexture(
                ExternalTextureError::IncompatibleReplacement { .. }
            ))
        ));
        assert!(matches!(
            registry.update(
                TextureId::new(99),
                external_pixels(&device, &queue, 1, &[0, 0, 255, 255])
            ),
            Err(EngineError::ExternalTexture(
                ExternalTextureError::UnknownTexture { .. }
            ))
        ));
        for (dimension, layers, samples, format, usage) in [
            (
                wgpu::TextureDimension::D2,
                1,
                1,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            (
                wgpu::TextureDimension::D2,
                1,
                1,
                READBACK_FORMAT,
                wgpu::TextureUsages::COPY_DST,
            ),
            (
                wgpu::TextureDimension::D2,
                2,
                1,
                READBACK_FORMAT,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            (
                wgpu::TextureDimension::D3,
                1,
                1,
                READBACK_FORMAT,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            (
                wgpu::TextureDimension::D2,
                1,
                4,
                READBACK_FORMAT,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
        ] {
            let make_texture = || {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Rejected import descriptor"),
                    size: wgpu::Extent3d {
                        width: 2,
                        height: 1,
                        depth_or_array_layers: layers,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let error = registry
                .register(TextureId::new(15), make_texture(), descriptor)
                .expect_err("unsupported import must be rejected before binding");
            assert_eq!(error.recoverability(), crate::Recoverability::Unrecoverable);
            assert!(matches!(error, EngineError::ExternalTexture(_)));
            registry
                .update(TextureId::new(14), make_texture())
                .expect_err("invalid update cannot replace original source");
        }
        // The fresh lookup and crop must still sample A after every rejected operation.
        painter.draw_texture(
            TextureId::new(14),
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            Some(Rect::from_xywh(0.0, 0.0, 1.0, 1.0)),
            FilterQuality::None,
            1.0,
        );
    });
    assert_eq!(pixel_at(&pixels, 32, 16, 16), [255, 0, 0, 255]);
}

fn external_missing_id_and_unregister_recover() {
    use crate::{EngineError, ExternalTextureError};
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    let (target, view) = crate::test_support::create_target(
        &device,
        "External failure recovery",
        32,
        32,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (32, 32),
    );
    painter.begin_frame().expect("recorded allocation frame");
    painter
        .external_texture_registry_mut()
        .register(
            TextureId::new(16),
            external_pixels(&device, &queue, 1, &[255, 0, 0, 255]),
            external_descriptor(
                crate::ExternalAlpha::Straight,
                crate::ExternalSampling::Nearest,
            ),
        )
        .expect("initial source");
    painter.draw_texture(
        TextureId::new(16),
        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
        None,
        FilterQuality::None,
        1.0,
    );
    assert!(
        painter
            .external_texture_registry_mut()
            .unregister(TextureId::new(16))
    );
    crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLACK);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_view(&view, &mut encoder)
        .expect("recorded lease survives unregister");
    painter
        .submit_encoder(encoder)
        .expect("managed external texture submission");
    painter.finish_frame();
    assert_eq!(
        pixel_at(
            &crate::test_support::readback_bytes(&device, &queue, &target, 32, 32),
            32,
            16,
            16
        ),
        [255, 0, 0, 255]
    );
    painter.begin_frame().expect("missing-ID frame");
    painter.draw_texture(
        TextureId::new(16),
        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
        None,
        FilterQuality::None,
        1.0,
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    assert!(matches!(
        painter.render_to_view(&view, &mut encoder),
        Err(EngineError::ExternalTexture(
            ExternalTextureError::UnknownTexture { .. }
        ))
    ));
    drop(encoder);
    painter.finish_frame();
    painter
        .begin_frame()
        .expect("valid frame after missing-ID error");
    painter
        .external_texture_registry_mut()
        .register(
            TextureId::new(16),
            external_pixels(&device, &queue, 1, &[0, 0, 255, 255]),
            external_descriptor(
                crate::ExternalAlpha::Straight,
                crate::ExternalSampling::Nearest,
            ),
        )
        .expect("fresh valid registration");
    painter.draw_texture(
        TextureId::new(16),
        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
        None,
        FilterQuality::None,
        1.0,
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_view(&view, &mut encoder)
        .expect("next valid frame renders");
    painter
        .submit_encoder(encoder)
        .expect("managed external texture submission");
    painter.finish_frame();
    assert_eq!(
        pixel_at(
            &crate::test_support::readback_bytes(&device, &queue, &target, 32, 32),
            32,
            16,
            16
        ),
        [0, 0, 255, 255]
    );
}

fn external_contents_are_live_within_the_same_allocation() {
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    let texture = external_pixels(&device, &queue, 1, &[255, 0, 0, 255]);
    let write_handle = texture.clone();
    let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLACK, |painter| {
        painter
            .external_texture_registry_mut()
            .register(
                TextureId::new(17),
                texture,
                external_descriptor(
                    crate::ExternalAlpha::Straight,
                    crate::ExternalSampling::Nearest,
                ),
            )
            .expect("live source registration");
        painter.draw_texture(
            TextureId::new(17),
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            None,
            FilterQuality::None,
            1.0,
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &write_handle,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 0, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    });
    assert_eq!(
        pixel_at(&pixels, 32, 16, 16),
        [0, 0, 255, 255],
        "allocation lease does not snapshot producer texels"
    );
}

fn external_nested_opacity_preserves_recorded_allocation() {
    use flui_painting::{
        Paint,
        paint::{FilterQuality, TextureId},
        styling::Color,
    };
    let (device, queue) = test_device_and_queue();
    let red = external_pixels(&device, &queue, 1, &[255, 0, 0, 255]);
    let blue = external_pixels(&device, &queue, 1, &[0, 0, 255, 255]);
    let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLACK, |painter| {
        painter
            .external_texture_registry_mut()
            .register(
                TextureId::new(18),
                red,
                external_descriptor(
                    crate::ExternalAlpha::Straight,
                    crate::ExternalSampling::Nearest,
                ),
            )
            .expect("nested source registration");
        painter.save_layer(None, &Paint::fill(Color::rgba(255, 255, 255, 128)));
        painter.save_layer(None, &Paint::fill(Color::rgba(255, 255, 255, 128)));
        painter.draw_texture(
            TextureId::new(18),
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            None,
            FilterQuality::None,
            1.0,
        );
        painter
            .external_texture_registry_mut()
            .update(TextureId::new(18), blue)
            .expect("future allocation replacement");
        painter.restore_layer();
        painter.restore_layer();
    });
    assert_external_pixel(pixel_at(&pixels, 32, 16, 16), [64, 0, 0, 255]);
}

fn external_competing_recording_failures() {
    use flui_painting::{
        Paint,
        paint::{FilterQuality, TextureId},
        styling::Color,
    };
    fn missing(painter: &mut WgpuPainter) {
        painter.draw_texture(
            TextureId::new(99),
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            None,
            FilterQuality::None,
            1.0,
        );
    }
    fn exceed(painter: &mut WgpuPainter) {
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(Color::RED),
        );
    }
    recording_failure_case(0, 0, false, |painter| {
        exceed(painter);
        missing(painter);
    });
    recording_failure_case(0, 0, true, |painter| {
        missing(painter);
        exceed(painter);
    });
    fn excessive_gradient(painter: &mut WgpuPainter) {
        use flui_painting::paint::{Shader, TileMode};
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(Color::RED).with_shader(Shader::LinearGradient {
                from: flui_foundation::geometry::Offset::new(0.0, 0.0),
                to: flui_foundation::geometry::Offset::new(64.0, 0.0),
                colors: vec![Color::RED; 257],
                stops: None,
                tile_mode: TileMode::Clamp,
            }),
        );
    }
    fn isolate(painter: &mut WgpuPainter) {
        // The preceding recorder is sealed before subsequent recording begins.
        painter.save_layer(None, &Paint::fill(Color::rgba(255, 255, 255, 128)));
    }
    recording_failure_case(1024 * 1024, 1024, false, |painter| {
        excessive_gradient(painter);
        isolate(painter);
        missing(painter);
        painter.restore_layer();
    });
    recording_failure_case(1024 * 1024, 1024, true, |painter| {
        missing(painter);
        isolate(painter);
        excessive_gradient(painter);
        painter.restore_layer();
    });
}

// Private lifetime observation is required here: wgpu itself retaining a texture
// backing would let a pixel-only test pass without the engine's completion lease.
fn external_repeated_submission_keeps_allocation() {
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    let (_, view) = crate::test_support::create_target(
        &device,
        "Repeated external submit",
        32,
        32,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (32, 32),
    );
    painter.begin_frame().expect("managed frame");
    let id = TextureId::new(24);
    painter
        .external_texture_registry_mut()
        .register(
            id,
            external_pixels(&device, &queue, 1, &[255, 0, 0, 255]),
            external_descriptor(
                crate::ExternalAlpha::Straight,
                crate::ExternalSampling::Nearest,
            ),
        )
        .expect("external allocation");
    let probe = painter
        .external_texture_registry()
        .get(id)
        .expect("registered texture")
        .lease()
        .lifetime_probe();
    for submit in 0..2 {
        painter.draw_texture(
            id,
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            None,
            FilterQuality::None,
            1.0,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .render_to_view(&view, &mut encoder)
            .expect("encoded draw");
        let index = painter.submit_encoder(encoder).expect("managed submission");
        if submit == 1 {
            painter.external_texture_registry_mut().unregister(id);
            painter.finish_frame();
            assert!(
                probe.is_alive(),
                "second submit owns a fresh lease after CPU finish"
            );
        }
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .expect("bounded completion");
        assert_eq!(
            probe.is_alive(),
            submit == 0,
            "only registry/cache retain allocation after first completed submit"
        );
    }
}

fn external_straight_linear_filtering_has_no_halo() {
    use flui_painting::paint::{FilterQuality, TextureId};
    let (device, queue) = test_device_and_queue();
    // Pixel centre x=12.5 on a 32px quad gives source coordinate 0.28125:
    // the opaque red texel contributes 0.71875, hence 183 red (92 at half opacity).
    // A straight-alpha interpolation followed by multiplication gives 132 red.
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8Unorm,
    ] {
        for (opacity, expected_red) in [(1.0, 183), (0.5, 92)] {
            for (alpha, transparent_rgb) in [
                (crate::ExternalAlpha::Premultiplied, [0, 0, 0]),
                (crate::ExternalAlpha::Straight, [0, 0, 0]),
                (crate::ExternalAlpha::Straight, [0, 0, 255]),
                (crate::ExternalAlpha::Straight, [0, 255, 0]),
                (crate::ExternalAlpha::Straight, [255, 255, 255]),
            ] {
                let source = external_pixels_in_format(
                    &device,
                    &queue,
                    2,
                    &[
                        255,
                        0,
                        0,
                        255,
                        transparent_rgb[0],
                        transparent_rgb[1],
                        transparent_rgb[2],
                        0,
                    ],
                    format,
                );
                let pixels = render_to_rgba(&device, &queue, 32, wgpu::Color::BLACK, |painter| {
                    painter
                        .external_texture_registry_mut()
                        .register(
                            TextureId::new(19),
                            source,
                            external_descriptor(alpha, crate::ExternalSampling::Linear),
                        )
                        .expect("filtered alpha source registration");
                    painter.draw_texture(
                        TextureId::new(19),
                        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                        None,
                        FilterQuality::Low,
                        opacity,
                    );
                });
                assert_external_pixel(pixel_at(&pixels, 32, 12, 16), [expected_red, 0, 0, 255]);
            }
        }
    }
}

/// A device without dual-source blending refuses unsupported direct coverage
/// instead of publishing wrong pixels, and the same public painter recovers.
fn featureless_direct_aa_refusal_recovers() {
    use flui_painting::{BlendMode, Paint, paint::Clip, styling::Color};
    let (device, queue) = test_device_and_queue();
    assert!(
        !device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING),
        "fixture requests no optional features"
    );
    let (target, view) = crate::test_support::create_target(
        &device,
        "featureless coverage recovery",
        32,
        32,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::RED);
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (32, 32),
    );
    painter.begin_frame().expect("refusal frame begins");
    painter.clip_rect(Rect::from_xywh(8.25, 0.0, 16.0, 32.0), Clip::AntiAlias);
    painter.draw_rect(
        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
        &Paint::fill(Color::BLUE)
            .with_anti_alias(false)
            .with_blend_mode(BlendMode::Clear),
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let error = painter
        .render_to_view(&view, &mut encoder)
        .expect_err("direct soft Clear has no capability");
    assert!(matches!(
        error,
        crate::EngineError::UnsupportedCoverageBlend {
            mode: BlendMode::Clear
        }
    ));
    assert_eq!(
        error.recoverability(),
        crate::error::Recoverability::Unrecoverable
    );
    drop(encoder);
    painter.finish_frame();
    let pixels = crate::test_support::readback_bytes(&device, &queue, &target, 32, 32);
    assert_eq!(
        pixel_at(&pixels, 32, 8, 16),
        [255, 0, 0, 255],
        "refused unsubmitted frame preserves target"
    );
    painter.begin_frame().expect("next valid frame begins");
    painter.draw_rect(
        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
        &Paint::fill(Color::GREEN).with_anti_alias(false),
    );
    let mut next = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_view(&view, &mut next)
        .expect("next frame encodes");
    painter.submit_encoder(next).expect("next frame submits");
    painter.finish_frame();
    let pixels = crate::test_support::readback_bytes(&device, &queue, &target, 32, 32);
    assert_eq!(pixel_at(&pixels, 32, 8, 16), [0, 255, 0, 255]);
}

/// A legal custom device's smaller fragment uniform count is admitted before
/// lazy mask layout creation; refusal leaves the reusable public painter valid.
fn limited_fragment_uniforms_clip_refusal_recovers() {
    use flui_painting::{Paint, paint::Clip, styling::Color};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .expect("GPU test adapter");
    let limits = wgpu::Limits {
        max_uniform_buffers_per_shader_stage: 2,
        ..wgpu::Limits::default()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Two fragment uniforms device"),
        required_limits: limits,
        ..wgpu::DeviceDescriptor::default()
    }))
    .expect("custom device with legal uniform count");
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let (target, view) = crate::test_support::create_target(
        &device,
        "limited uniform recovery",
        32,
        32,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLACK);
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (32, 32),
    );
    for color in [Color::RED, Color::GREEN] {
        painter.begin_frame().expect("ordinary frame begins");
        painter.draw_rect(
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            &Paint::fill(color).with_anti_alias(false),
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .render_to_view(&view, &mut encoder)
            .expect("ordinary frame encodes with two uniforms");
        painter
            .submit_encoder(encoder)
            .expect("ordinary frame submits");
        painter.finish_frame();
        let frame = crate::test_support::readback_bytes(&device, &queue, &target, 32, 32);
        let expected = if color == Color::RED {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        };
        assert_eq!(pixel_at(&frame, 32, 16, 16), expected);
        if color == Color::RED {
            painter.begin_frame().expect("clipped frame begins");
            painter.clip_rect(Rect::from_xywh(8.25, 0.0, 16.0, 32.0), Clip::AntiAlias);
            painter.draw_rect(
                Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
                &Paint::fill(Color::BLUE).with_anti_alias(false),
            );
            let mut failed =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            assert!(
                matches!(
                    painter.render_to_view(&view, &mut failed),
                    Err(crate::EngineError::PreparedResourceLimit {
                        resource: "clip fragment uniform buffers",
                        requested: 3,
                        limit: 2
                    })
                ),
                "mask layout requirement returns typed refusal before wgpu validation"
            );
            drop(failed);
            painter.finish_frame();
            let frame = crate::test_support::readback_bytes(&device, &queue, &target, 32, 32);
            assert_eq!(
                pixel_at(&frame, 32, 16, 16),
                [255, 0, 0, 255],
                "refused clipped frame preserves prior target"
            );
        }
    }
}

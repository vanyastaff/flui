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

    draw(&mut painter);
    painter
        .render_to_view(&target_view, &mut encoder)
        .expect("painter.render must succeed for readback");
    queue.submit(std::iter::once(encoder.finish()));

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
    let cases: [(&str, fn()); 10] = [
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
    painter.begin_frame();
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
    painter.begin_frame();
    // Private admission seam, but recording/rendering are the consumer's public API.
    painter.current_segment = crate::command_ir::DrawSegment::with_budget(
        crate::recording_budget::RecordingBudget::new(bytes, elements),
    );
    draw(&mut painter);
    let mut failed = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    assert!(
        matches!(
            painter.render_to_view(&view, &mut failed),
            Err(crate::error::EngineError::PreparedResourceLimit { .. })
        ),
        "recording quota refusal must reach render, not silently skip"
    );
    drop(failed);
    painter.finish_frame();
    let unchanged = crate::test_support::readback_bytes(&device, &queue, &target, 64, 64);
    assert_eq!(
        pixel_at(&unchanged, 64, 32, 32),
        [0, 0, 0, 255],
        "failed unsubmitted frame leaves target untouched"
    );
    painter.begin_frame();
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

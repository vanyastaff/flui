use super::*;
use flui_foundation::geometry::Matrix4;
use flui_painting::{
    Paint,
    paint::{Image, ImageRepeat},
    styling::Color,
};

pub(super) fn filtered_image_device_limits_refuse_and_recover() {
    use flui_painting::paint::ColorFilter;
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .expect("GPU test adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Three bind groups image device"),
        required_limits: wgpu::Limits {
            max_bind_groups: 3,
            ..wgpu::Limits::default()
        },
        ..wgpu::DeviceDescriptor::default()
    }))
    .expect("custom image device");
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let (target, view) = crate::test_support::create_target(
        &device,
        "Filtered image limit recovery",
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
    painter
        .begin_frame()
        .expect("limited filtered frame begins");
    painter.draw_image_region(
        &image(),
        Rect::from_xywh(0.0, 0.0, 4.0, 4.0),
        Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
        None,
        ImageRepeat::NoRepeat,
        Some(ColorFilter::Mode {
            color: Color::RED,
            blend_mode: BlendMode::Src,
        }),
        None,
    );
    let mut rejected = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    assert!(
        matches!(
            painter.render_to_texture(&target, &mut rejected),
            Err(crate::EngineError::PreparedResourceLimit {
                resource: "image filter bind groups",
                requested: 4,
                limit: 3
            })
        ),
        "filter layout must be refused before GPU pipeline creation"
    );
    drop(rejected);
    painter.finish_frame();
    painter.begin_frame().expect("unfiltered frame begins");
    painter.save();
    painter.clip_rrect(
        flui_foundation::geometry::RRect::from_rect_circular(
            Rect::from_xywh(14.25, 14.25, 12.0, 12.0),
            3.0,
        ),
        flui_painting::paint::Clip::AntiAlias,
    );
    painter.draw_image(
        &Image::solid_color(4, 4, Color::GREEN),
        Rect::from_xywh(16.0, 16.0, 8.0, 8.0),
        BlendMode::Src,
    );
    painter.restore();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_texture(&target, &mut encoder)
        .expect("unfiltered images fit the same device");
    painter
        .submit_encoder(encoder)
        .expect("valid frame submits");
    painter.finish_frame();
    let pixels = crate::test_support::readback_bytes(&device, &queue, &target, 32, 32);
    assert_eq!(pixel_at(&pixels, 32, 4, 4), [0, 0, 0, 255]);
    assert_eq!(pixel_at(&pixels, 32, 20, 20), [0, 255, 0, 255]);
}

pub(super) fn filtered_image_matrix_range() {
    use flui_painting::paint::{ColorFilter, ColorMatrix};
    for coefficient in [f32::NAN, f32::INFINITY, f32::MAX] {
        for offset in [0.0, 0.25, 1.0] {
            let mut matrix = ColorMatrix::identity();
            matrix.values[..5].copy_from_slice(&[
                coefficient,
                coefficient,
                -coefficient,
                -coefficient,
                offset,
            ]);
            let pixels = draw_clear(32, wgpu::Color::BLUE, |p| {
                p.draw_image_region(
                    &image(),
                    Rect::from_xywh(0.0, 0.0, 4.0, 4.0),
                    Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
                    None,
                    ImageRepeat::NoRepeat,
                    Some(ColorFilter::Matrix(matrix)),
                    None,
                );
                p.draw_rect(
                    Rect::from_xywh(16.0, 16.0, 8.0, 8.0),
                    &Paint::fill(Color::RED),
                );
            });
            let expected = if coefficient.is_finite() {
                [(offset * 255.0).round() as u8, 255, 255, 255]
            } else {
                [0, 0, 255, 255]
            };
            assert_eq!(
                pixel_at(&pixels, 32, 4, 4),
                expected,
                "matrix coefficient {coefficient}"
            );
            assert_eq!(
                pixel_at(&pixels, 32, 20, 20),
                [255, 0, 0, 255],
                "a valid following draw remains deliverable"
            );
        }
    }
}

pub(super) fn filtered_image_preparation_failure_recovers() {
    use crate::device_domain::{DeviceDomain, PreparedCost, PreparedIrLimits};
    use flui_painting::paint::ColorFilter;
    let (device, queue) = test_device_and_queue();
    // Target bindings fit; the additional filter uniform cannot. This private
    // seam exercises resource admission through the ordinary painter API.
    let domain = DeviceDomain::with_limits(
        Arc::clone(&device),
        Arc::clone(&queue),
        PreparedIrLimits {
            cost: PreparedCost {
                gpu_bytes: 128,
                ..PreparedIrLimits::default().cost
            },
            ..Default::default()
        },
    );
    let mut painter = WgpuPainter::with_domain(domain, READBACK_FORMAT, (32, 32));
    let (target, view) = crate::test_support::create_target(
        &device,
        "Filter preparation recovery",
        32,
        32,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    painter.begin_frame().expect("failing frame begins");
    let mut rejected = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter.draw_image_region(
        &image(),
        Rect::from_xywh(0.0, 0.0, 4.0, 4.0),
        Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
        None,
        ImageRepeat::NoRepeat,
        Some(ColorFilter::Mode {
            color: Color::RED,
            blend_mode: BlendMode::Src,
        }),
        None,
    );
    assert!(
        matches!(
            painter.render_to_texture(&target, &mut rejected),
            Err(crate::EngineError::PreparedResourceLimit {
                resource: "GPU payload bytes",
                ..
            })
        ),
        "filter preparation must fail through the resource contract"
    );
    drop(rejected);
    painter.finish_frame();
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("rejected frame resource retirement");

    painter.begin_frame().expect("recovery frame begins");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Recovery backdrop"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    painter.draw_image(
        &Image::solid_color(4, 4, Color::GREEN),
        Rect::from_xywh(16.0, 16.0, 8.0, 8.0),
        BlendMode::SrcOver,
    );
    painter
        .render_to_texture(&target, &mut encoder)
        .expect("unfiltered frame fits the same allowance");
    painter
        .submit_encoder(encoder)
        .expect("recovery frame submits");
    painter.finish_frame();
    let pixels = crate::test_support::readback_bytes(&device, &queue, &target, 32, 32);
    assert_eq!(
        pixel_at(&pixels, 32, 4, 4),
        [0, 0, 0, 255],
        "rejected draw must not leak into the next frame"
    );
    assert_eq!(pixel_at(&pixels, 32, 20, 20), [0, 255, 0, 255]);
}

pub(super) fn filtered_image_independent_clip_coverage() {
    use flui_foundation::geometry::RRect;
    use flui_painting::paint::{Clip, ColorFilter};
    let source = Image::from_rgba8(4, 4, [255, 0, 0, 0].repeat(16));
    let filtered = |p: &mut WgpuPainter, mode, color| {
        p.save();
        p.clip_rrect(
            RRect::from_rect_circular(Rect::from_xywh(6.25, 6.25, 12.0, 12.0), 3.0),
            Clip::AntiAlias,
        );
        p.draw_image_region(
            &source,
            Rect::from_xywh(0.0, 0.0, 4.0, 4.0),
            Rect::from_xywh(4.0, 4.0, 20.0, 20.0),
            None,
            ImageRepeat::NoRepeat,
            Some(ColorFilter::Mode {
                color,
                blend_mode: BlendMode::Src,
            }),
            Some(&Paint::fill(Color::WHITE).with_blend_mode(mode)),
        );
        p.restore();
    };
    let reference = draw_clear(32, wgpu::Color::TRANSPARENT, |p| {
        filtered(p, BlendMode::SrcOver, Color::WHITE);
    });
    let actual = draw(32, |p| {
        filtered(p, BlendMode::Src, Color::GREEN.with_opacity(0.5));
    });
    let mut partial = 0;
    for y in 4..24 {
        for x in 4..24 {
            let coverage = pixel_at(&reference, 32, x, y)[3];
            let pixel = pixel_at(&actual, 32, x, y);
            let green = (f32::from(coverage) * 0.5).round() as u8;
            let blue = 255 - coverage;
            near(pixel, [0, green, blue, green.saturating_add(blue)]);
            partial += usize::from((1..255).contains(&coverage));
        }
    }
    assert!(partial > 0, "the oracle must exercise fractional coverage");
}

fn draw(size: u32, draw: impl FnOnce(&mut WgpuPainter)) -> Vec<u8> {
    draw_clear(size, wgpu::Color::BLUE, draw)
}
fn draw_clear(size: u32, clear: wgpu::Color, draw: impl FnOnce(&mut WgpuPainter)) -> Vec<u8> {
    let (device, queue) = test_device_and_queue();
    let (target, view) = crate::test_support::create_target(
        &device,
        "Image contract",
        size,
        size,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (size, size),
    );
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
    painter.begin_frame().expect("image frame begins");
    draw(&mut painter);
    painter
        .render_to_texture(&target, &mut encoder)
        .expect("image frame renders");
    painter
        .submit_encoder(encoder)
        .expect("image submission completes");
    painter.finish_frame();
    crate::test_support::readback_bytes(&device, &queue, &target, size, size)
}
fn image() -> Image {
    Image::from_rgba8(4, 4, [255, 255, 255, 255].repeat(16))
}
fn near(actual: [u8; 4], expected: [u8; 4]) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "actual {actual:?}, expected {expected:?}"
    );
}
pub(super) fn ambient_shear() {
    let rgba = draw(32, |p| {
        let mut matrix = Matrix4::IDENTITY;
        matrix.m[4] = 1.0;
        matrix.m[12] = 8.0;
        matrix.m[13] = 4.0;
        p.save();
        p.transform(&matrix);
        p.draw_image(
            &image(),
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            BlendMode::SrcOver,
        );
        p.restore();
        p.draw_rect(
            Rect::from_xywh(1.0, 1.0, 2.0, 2.0),
            &Paint::fill(Color::RED),
        );
    });
    near(pixel_at(&rgba, 32, 9, 11), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 16, 8), [255, 255, 255, 255]);
    near(pixel_at(&rgba, 32, 1, 1), [255, 0, 0, 255]);
}
pub(super) fn rotated_uv() {
    let rgba = draw(32, |p| {
        let image = Image::from_rgba8(
            4,
            4,
            (0..16)
                .flat_map(|i| {
                    if i % 4 < 2 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 255, 0, 255]
                    }
                })
                .collect(),
        );
        let mut matrix = Matrix4::IDENTITY;
        matrix.m[0] = 0.0;
        matrix.m[1] = 1.0;
        matrix.m[4] = -1.0;
        matrix.m[5] = 0.0;
        matrix.m[12] = 16.0;
        matrix.m[13] = 4.0;
        p.transform(&matrix);
        p.draw_image(
            &image,
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            BlendMode::SrcOver,
        );
    });
    near(pixel_at(&rgba, 32, 12, 5), [255, 0, 0, 255]);
    near(pixel_at(&rgba, 32, 12, 10), [0, 255, 0, 255]);
}
pub(super) fn tint_and_alpha() {
    let rgba = draw(32, |p| {
        p.draw_image_region(
            &image(),
            Rect::from_xywh(0.0, 0.0, 4.0, 4.0),
            Rect::from_xywh(4.0, 4.0, 8.0, 8.0),
            None,
            ImageRepeat::NoRepeat,
            None,
            Some(&Paint::fill(Color::rgba(0, 255, 0, 128))),
        );
    });
    near(pixel_at(&rgba, 32, 8, 8), [0, 128, 127, 255]);
}
pub(super) fn src_transparency() {
    let rgba = draw(32, |p| {
        let image = Image::from_rgba8(4, 4, [255, 0, 0, 0].repeat(16));
        p.draw_image(&image, Rect::from_xywh(4.0, 4.0, 8.0, 8.0), BlendMode::Src);
        p.draw_rect(
            Rect::from_xywh(20.0, 20.0, 3.0, 3.0),
            &Paint::fill(Color::GREEN),
        );
    });
    near(pixel_at(&rgba, 32, 8, 8), [0, 0, 0, 0]);
    near(pixel_at(&rgba, 32, 2, 2), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 21, 21), [0, 255, 0, 255]);
}

pub(super) fn feathered_src_transparency() {
    use flui_foundation::geometry::RRect;
    use flui_painting::paint::Clip;
    let clipped = |p: &mut WgpuPainter, source: &Image, mode: BlendMode| {
        p.save();
        p.clip_rrect(
            RRect::from_rect_circular(Rect::from_xywh(6.25, 6.25, 12.0, 12.0), 3.0),
            Clip::AntiAlias,
        );
        p.draw_image(source, Rect::from_xywh(4.0, 4.0, 20.0, 20.0), mode);
        p.restore();
    };
    // Opaque SrcOver on a transparent target measures geometric clip coverage
    // independently of the destination-replacing operator under test.
    let reference = draw_clear(32, wgpu::Color::TRANSPARENT, |p| {
        clipped(p, &image(), BlendMode::SrcOver);
    });
    let actual = draw(32, |p| {
        clipped(
            p,
            &Image::from_rgba8(4, 4, [255, 0, 0, 0].repeat(16)),
            BlendMode::Src,
        );
        p.draw_rect(
            Rect::from_xywh(24.0, 24.0, 3.0, 3.0),
            &Paint::fill(Color::GREEN),
        );
    });
    let mut partial = 0;
    for y in 5..20 {
        for x in 5..20 {
            let coverage = pixel_at(&reference, 32, x, y)[3];
            if (10..=245).contains(&coverage) {
                partial += 1;
                near(
                    pixel_at(&actual, 32, x, y),
                    [0, 0, 255 - coverage, 255 - coverage],
                );
            }
        }
    }
    assert!(
        partial > 0,
        "feathered clip contains partial-coverage pixels"
    );
    near(pixel_at(&actual, 32, 10, 10), [0, 0, 0, 0]);
    near(pixel_at(&actual, 32, 2, 2), [0, 0, 255, 255]);
    near(pixel_at(&actual, 32, 25, 25), [0, 255, 0, 255]);
}

fn replay_canvas(painter: &mut WgpuPainter, canvas: flui_painting::Canvas) {
    let list = canvas.finish();
    let mut dispatcher = crate::layer_dispatcher::LayerDispatcher::new(painter);
    for command in list.commands() {
        crate::dispatch::dispatch_command(command, &mut dispatcher);
    }
}
fn striped() -> Image {
    Image::from_rgba8(
        8,
        4,
        (0..32)
            .flat_map(|i| match i % 8 {
                0 | 1 => [255, 0, 0, 255],
                2..=5 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            })
            .collect(),
    )
}
pub(super) fn decoration_cover() {
    use flui_painting::{
        BoxFit,
        decoration::{DecorationPaintOptions, paint_box_decoration},
        styling::{BoxDecoration, DecorationImage},
    };
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        paint_box_decoration(
            &mut canvas,
            Rect::from_xywh(8.0, 8.0, 4.0, 4.0),
            &BoxDecoration::with_image(DecorationImage::new(striped()).with_fit(BoxFit::Cover)),
            DecorationPaintOptions::default(),
        );
        canvas.draw_rect(
            Rect::from_xywh(22.0, 22.0, 3.0, 3.0),
            &Paint::fill(Color::RED),
        );
        replay_canvas(p, canvas);
    });
    near(pixel_at(&rgba, 32, 8, 9), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 9, 9), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 7, 9), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 23, 23), [255, 0, 0, 255]);
}
pub(super) fn decoration_filter_opacity() {
    use flui_painting::{
        BoxFit,
        decoration::{DecorationPaintOptions, paint_box_decoration},
        paint::ColorFilter,
        styling::{BoxDecoration, DecorationImage},
    };
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        let decoration = DecorationImage::new(image())
            .with_fit(BoxFit::Fill)
            .with_opacity(0.5)
            .with_color_filter(ColorFilter::Mode {
                color: Color::GREEN,
                blend_mode: BlendMode::Src,
            });
        paint_box_decoration(
            &mut canvas,
            Rect::from_xywh(4.0, 4.0, 8.0, 8.0),
            &BoxDecoration::with_image(decoration),
            DecorationPaintOptions::default(),
        );
        replay_canvas(p, canvas);
    });
    near(pixel_at(&rgba, 32, 8, 8), [0, 128, 127, 255]);
}
pub(super) fn canvas_image_paint() {
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_image(
            image(),
            Rect::from_xywh(4.0, 4.0, 8.0, 8.0),
            Some(&Paint::fill(Color::rgba(0, 255, 0, 128))),
        );
        replay_canvas(p, canvas);
    });
    near(pixel_at(&rgba, 32, 8, 8), [0, 128, 127, 255]);
}
fn render_image_crop(fit: flui_painting::BoxFit, scale: f64) {
    use flui_foundation::geometry::Size;
    use flui_objects::RenderImage;
    use flui_painting::{Alignment, ImagePlacement};
    use flui_rendering::testing::{RenderTester, box_node};
    let mut image =
        RenderImage::from_image(striped(), ImagePlacement::fitted(fit, Alignment::CENTER));
    // Configure before arena admission; no owner exists to receive an update impact.
    let _ = image.set_scale(scale);
    let run = RenderTester::mount(box_node(image))
        .with_size(Size::new(4.0, 4.0))
        .run_frame();
    let renderer =
        pollster::block_on(crate::HeadlessRenderer::new()).expect("image producer GPU adapter");
    let rgba = renderer
        .render_layer_tree(run.layer_tree().expect("image producer painted"), (16, 16))
        .expect("image producer pixels");
    near(pixel_at(&rgba, 16, 0, 1), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 16, 1, 1), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 16, 5, 1), pixel_at(&rgba, 16, 15, 15));
}

pub(super) fn render_image_scaled_cover() {
    render_image_crop(flui_painting::BoxFit::Cover, 2.0);
}
pub(super) fn render_image_subnormal_scale_cover() {
    render_image_crop(flui_painting::BoxFit::Cover, f64::from_bits(1));
}
pub(super) fn render_image_natural_crop() {
    render_image_crop(flui_painting::BoxFit::None, 1.0);
}

pub(super) fn decoration_repeat_phase() {
    use flui_painting::{
        BoxFit,
        decoration::{DecorationPaintOptions, paint_box_decoration},
        styling::{BoxDecoration, DecorationImage},
    };
    let source = Image::from_rgba8(
        4,
        2,
        (0..8)
            .flat_map(|i| {
                if i % 4 < 2 {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                }
            })
            .collect(),
    );
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        let image = DecorationImage::new(source)
            .with_fit(BoxFit::Contain)
            .with_repeat(ImageRepeat::RepeatX);
        paint_box_decoration(
            &mut canvas,
            Rect::from_xywh(4.0, 4.0, 10.0, 4.0),
            &BoxDecoration::with_image(image),
            DecorationPaintOptions::default(),
        );
        replay_canvas(p, canvas);
    });
    // Fitted 8x4 tiles are centred at x=5: the first visible texel is the
    // preceding tile's green tail; x=6 is the next tile's red prefix.
    near(pixel_at(&rgba, 32, 4, 5), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 6, 5), [255, 0, 0, 255]);
    near(pixel_at(&rgba, 32, 10, 5), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 3, 5), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 14, 5), [0, 0, 255, 255]);
}
pub(super) fn atlas_affine_and_paint() {
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        let mut ambient = Matrix4::IDENTITY;
        ambient.m[4] = 1.0;
        ambient.m[12] = 8.0;
        ambient.m[13] = 4.0;
        canvas.transform(ambient);
        let mut sprite = Matrix4::IDENTITY;
        sprite.m[0] = 2.0;
        sprite.m[5] = 2.0;
        canvas.draw_atlas(
            image(),
            vec![Rect::from_xywh(0.0, 0.0, 4.0, 4.0)],
            vec![sprite],
            None,
            BlendMode::SrcOver,
            Some(&Paint::fill(Color::rgba(0, 255, 0, 128))),
        );
        replay_canvas(p, canvas);
    });
    near(pixel_at(&rgba, 32, 9, 11), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 16, 8), [0, 128, 127, 255]);
}
pub(super) fn advanced_affine() {
    let rgba = draw_clear(32, wgpu::Color::WHITE, |p| {
        let mut matrix = Matrix4::IDENTITY;
        matrix.m[4] = 1.0;
        matrix.m[12] = 8.0;
        matrix.m[13] = 4.0;
        p.transform(&matrix);
        p.draw_image(
            &Image::from_rgba8(4, 4, [255, 0, 0, 255].repeat(16)),
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            BlendMode::Multiply,
        );
    });
    near(pixel_at(&rgba, 32, 9, 11), [255, 255, 255, 255]);
    near(pixel_at(&rgba, 32, 16, 8), [255, 0, 0, 255]);
}

pub(super) fn standalone_source_crop() {
    let source = Image::from_rgba8(
        1024,
        4,
        (0..4096)
            .flat_map(|i| match i % 1024 {
                0..=255 => [255, 0, 0, 255],
                256..=767 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            })
            .collect(),
    );
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_image_region(
            source,
            Rect::from_xywh(500.0, 0.0, 4.0, 4.0),
            Rect::from_xywh(4.0, 4.0, 8.0, 8.0),
            None,
        );
        replay_canvas(p, canvas);
    });
    near(pixel_at(&rgba, 32, 5, 7), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 10, 7), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 3, 7), [0, 0, 255, 255]);
}
pub(super) fn degenerate_image_quad_keeps_sibling() {
    let rgba = draw(32, |p| {
        p.save();
        // A finite singular affine is admitted by the painter; image recording
        // omits its zero-area quad without poisoning the next draw.
        p.scale(0.0, 1.0);
        p.draw_image(
            &image(),
            Rect::from_xywh(4.0, 4.0, 8.0, 8.0),
            BlendMode::SrcOver,
        );
        p.restore();
        p.draw_image(
            &image(),
            Rect::from_xywh(20.0, 20.0, 4.0, 4.0),
            BlendMode::SrcOver,
        );
    });
    near(pixel_at(&rgba, 32, 8, 8), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 21, 21), [255, 255, 255, 255]);
}

fn original_edge(width: u32) {
    let source = Image::from_rgba8(width, 4, [0, 255, 0, 255].repeat(width as usize * 4));
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_image_region(
            source,
            Rect::from_xywh(0.0, 0.0, 4.0, 4.0),
            Rect::from_xywh(4.0, 4.0, 8.0, 8.0),
            None,
        );
        replay_canvas(p, canvas);
    });
    // The same cropped content and scale must have the same original-image
    // edge samples whether the source fits an atlas or needs its own texture.
    near(pixel_at(&rgba, 32, 7, 4), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 4, 7), [0, 255, 0, 255]);
    // The first cache entry can touch the atlas's top/left edges. Its right
    // and bottom still border other atlas texels and must clamp independently.
    near(pixel_at(&rgba, 32, 11, 7), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 7, 11), [0, 255, 0, 255]);
    near(pixel_at(&rgba, 32, 3, 7), [0, 0, 255, 255]);
}
pub(super) fn packed_original_edge() {
    original_edge(4);
}
pub(super) fn standalone_original_edge() {
    original_edge(1024);
}

pub(super) fn crop_keeps_original_neighbors() {
    let source = Image::from_rgba8(
        4,
        4,
        [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ]
        .repeat(4),
    );
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_image_region(
            source,
            Rect::from_xywh(1.0, 0.0, 1.0, 4.0),
            Rect::from_xywh(8.0, 8.0, 4.0, 4.0),
            None,
        );
        replay_canvas(p, canvas);
    });
    // Fractional taps at each crop boundary still interpolate the original
    // adjacent red/blue texel instead of clamping the crop to solid green.
    near(pixel_at(&rgba, 32, 8, 9), [96, 159, 0, 255]);
    near(pixel_at(&rgba, 32, 11, 9), [0, 159, 96, 255]);
    near(pixel_at(&rgba, 32, 7, 9), [0, 0, 255, 255]);
}

fn decoration_cover_outside_alignment(
    alignment: flui_painting::Alignment,
    vertical: bool,
    expected: [[u8; 4]; 2],
    repeat: flui_painting::paint::ImageRepeat,
) {
    use flui_painting::{
        BoxFit,
        decoration::{DecorationPaintOptions, paint_box_decoration},
        styling::{BoxDecoration, DecorationImage},
    };
    let (width, height) = if vertical { (4, 8) } else { (8, 4) };
    let source = Image::from_rgba8(width, height, [255, 255, 255, 255].repeat(32));
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        paint_box_decoration(
            &mut canvas,
            Rect::from_xywh(8.0, 8.0, 4.0, 4.0),
            &BoxDecoration::with_image(
                DecorationImage::new(source)
                    .with_fit(BoxFit::Cover)
                    .with_alignment(alignment)
                    .with_repeat(repeat),
            ),
            DecorationPaintOptions::default(),
        );
        canvas.draw_rect(
            Rect::from_xywh(22.0, 22.0, 3.0, 3.0),
            &Paint::fill(Color::RED),
        );
        replay_canvas(p, canvas);
    });
    let points = if vertical {
        [(9, 9), (9, 11)]
    } else {
        [(9, 9), (11, 9)]
    };
    for (point, expected) in points.into_iter().zip(expected) {
        near(pixel_at(&rgba, 32, point.0, point.1), expected);
    }
    near(pixel_at(&rgba, 32, 7, 9), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 23, 23), [255, 0, 0, 255]);
}
pub(super) fn decoration_cover_positive_overshoot() {
    decoration_cover_outside_alignment(
        flui_painting::Alignment::new(2.0, 0.0),
        false,
        [[255; 4], [0, 0, 255, 255]],
        flui_painting::paint::ImageRepeat::NoRepeat,
    );
}
pub(super) fn decoration_cover_negative_overshoot() {
    decoration_cover_outside_alignment(
        flui_painting::Alignment::new(-2.0, 0.0),
        false,
        [[0, 0, 255, 255], [255; 4]],
        flui_painting::paint::ImageRepeat::NoRepeat,
    );
}
pub(super) fn decoration_cover_vertical_overshoot() {
    decoration_cover_outside_alignment(
        flui_painting::Alignment::new(0.0, 2.0),
        true,
        [[255; 4], [0, 0, 255, 255]],
        flui_painting::paint::ImageRepeat::NoRepeat,
    );
}
pub(super) fn decoration_cover_fully_outside() {
    decoration_cover_outside_alignment(
        flui_painting::Alignment::new(4.0, 0.0),
        false,
        [[0, 0, 255, 255]; 2],
        flui_painting::paint::ImageRepeat::NoRepeat,
    );
}

pub(super) fn decoration_cover_repeat_overshoot() {
    decoration_cover_outside_alignment(
        flui_painting::Alignment::new(2.0, 0.0),
        false,
        [[255; 4], [0, 0, 255, 255]],
        flui_painting::paint::ImageRepeat::Repeat,
    );
}
pub(super) fn decoration_cover_repeat_x_overshoot() {
    decoration_cover_outside_alignment(
        flui_painting::Alignment::new(-2.0, 0.0),
        false,
        [[0, 0, 255, 255], [255; 4]],
        flui_painting::paint::ImageRepeat::RepeatX,
    );
}
pub(super) fn decoration_cover_repeat_y_overshoot() {
    decoration_cover_outside_alignment(
        flui_painting::Alignment::new(0.0, 2.0),
        true,
        [[255; 4], [0, 0, 255, 255]],
        flui_painting::paint::ImageRepeat::RepeatY,
    );
}
pub(super) fn decoration_none_crop_keeps_other_axis_repeat() {
    use flui_painting::{
        Alignment, BoxFit,
        decoration::{DecorationPaintOptions, paint_box_decoration},
        paint::ImageRepeat,
        styling::{BoxDecoration, DecorationImage},
    };
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        let mut texels = [255, 255, 255, 255].repeat(8);
        texels.extend([0, 255, 0, 255].repeat(8));
        let image = Image::from_rgba8(8, 2, texels);
        paint_box_decoration(
            &mut canvas,
            Rect::from_xywh(8.0, 8.0, 4.0, 6.0),
            &BoxDecoration::with_image(
                DecorationImage::new(image)
                    .with_fit(BoxFit::None)
                    .with_alignment(Alignment::new(2.0, 0.0))
                    .with_repeat(ImageRepeat::RepeatY),
            ),
            DecorationPaintOptions::default(),
        );
        canvas.draw_rect(
            Rect::from_xywh(22.0, 22.0, 3.0, 3.0),
            &Paint::fill(Color::RED),
        );
        replay_canvas(p, canvas);
    });
    for y in 8..14 {
        let expected = if y % 2 == 0 {
            [255; 4]
        } else {
            [0, 255, 0, 255]
        };
        near(pixel_at(&rgba, 32, 9, y), expected);
    }
    near(pixel_at(&rgba, 32, 11, 10), [0, 0, 255, 255]);
    near(pixel_at(&rgba, 32, 23, 23), [255, 0, 0, 255]);
}

/// A nine-slice keeps logical corners and stretches only its centre: a 9x9
/// asset at half scale (2-texel red border, white centre) has 4-pixel corners
/// in a 32-pixel box. Stretching the whole image would tint (6, 6); corners
/// drawn one pixel per texel would tint (2, 2).
pub(super) fn sliced_placement_keeps_logical_corners() {
    use flui_foundation::geometry::Size;
    use flui_painting::ImagePlacement;
    let bordered = Image::from_rgba8(
        9,
        9,
        (0..81)
            .flat_map(|i| {
                let (x, y) = (i % 9, i / 9);
                if (2..7).contains(&x) && (2..7).contains(&y) {
                    [255, 255, 255, 255]
                } else {
                    [255, 0, 0, 255]
                }
            })
            .collect::<Vec<u8>>(),
    );
    let rgba = draw(32, |p| {
        let mut canvas = flui_painting::Canvas::new();
        ImagePlacement::sliced(Rect::from_ltrb(2.0, 2.0, 7.0, 7.0)).paint(
            &mut canvas,
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            &bordered,
            Size::new(18.0, 18.0),
            None,
            None,
        );
        replay_canvas(p, canvas);
    });
    near(pixel_at(&rgba, 32, 2, 2), [255, 0, 0, 255]);
    near(pixel_at(&rgba, 32, 6, 6), [255, 255, 255, 255]);
    near(pixel_at(&rgba, 32, 16, 16), [255, 255, 255, 255]);
    near(pixel_at(&rgba, 32, 29, 29), [255, 0, 0, 255]);
}

use super::*;
use flui_foundation::geometry::Matrix4;
use flui_painting::{
    Paint,
    paint::{Image, ImageRepeat},
    styling::Color,
};

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
fn render_image_crop(fit: flui_objects::ImageFit, scale: f64) {
    use flui_foundation::geometry::Size;
    use flui_objects::{ImageAlignment, RenderImage};
    use flui_rendering::testing::{RenderTester, box_node};
    let mut image = RenderImage::from_image(striped(), fit, ImageAlignment::Center);
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
    render_image_crop(flui_objects::ImageFit::Cover, 2.0);
}
pub(super) fn render_image_natural_crop() {
    render_image_crop(flui_objects::ImageFit::None, 1.0);
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

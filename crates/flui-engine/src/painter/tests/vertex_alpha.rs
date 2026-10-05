use super::*;
use flui_foundation::geometry::Point;
use flui_painting::{Canvas, Paint, styling::Color};

const SIZE: u32 = 32;
const INDICES: [u16; 3] = [0, 1, 2];

fn triangle() -> [Point<f64>; 3] {
    [
        Point::new(4.0, 4.0),
        Point::new(28.0, 4.0),
        Point::new(4.0, 28.0),
    ]
}

fn draw_mesh(painter: &mut WgpuPainter, colors: Option<&[Color]>, paint: &Paint, canvas: bool) {
    if canvas {
        let mut recording = Canvas::new();
        recording.draw_vertices(
            triangle().to_vec(),
            colors.map(<[Color]>::to_vec),
            None,
            INDICES.to_vec(),
            paint,
        );
        let list = recording.finish();
        let mut dispatcher = crate::layer_dispatcher::LayerDispatcher::new(painter);
        for command in list.commands() {
            crate::dispatch::dispatch_command(command, &mut dispatcher);
        }
    } else {
        painter.draw_vertices(&triangle(), colors, None, &INDICES, paint);
    }
}

fn render(draw: impl FnOnce(&mut WgpuPainter)) -> Vec<u8> {
    let (device, queue) = test_device_and_queue();
    let (target, view) = crate::test_support::create_target(
        &device,
        "vertex color contract",
        SIZE,
        SIZE,
        READBACK_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        READBACK_FORMAT,
        (SIZE, SIZE),
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vertex color backdrop"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    painter.begin_frame().expect("vertex frame begins");
    draw(&mut painter);
    painter
        .render_to_texture(&target, &mut encoder)
        .expect("vertex frame renders");
    painter
        .submit_encoder(encoder)
        .expect("vertex submission admitted");
    painter.finish_frame();
    crate::test_support::readback_bytes(&device, &queue, &target, SIZE, SIZE)
}

fn expect_pixel(rgba: &[u8], x: u32, y: u32, expected: [u8; 4]) {
    let actual = pixel_at(rgba, SIZE, x, y);
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 1),
        "pixel ({x}, {y}): actual {actual:?}, expected {expected:?}"
    );
}

fn mesh_case(colors: Option<&[Color]>, paint: &Paint, expected: [u8; 4], canvas: bool) {
    let rgba = render(|painter| draw_mesh(painter, colors, paint, canvas));
    // Both samples are well inside/outside the aliased triangle: no edge AA.
    expect_pixel(&rgba, 10, 10, expected);
    expect_pixel(&rgba, 25, 25, [0, 0, 255, 255]);
}

pub(super) fn uniform_translucent() {
    mesh_case(
        Some(&[Color::rgba(255, 0, 0, 128); 3]),
        &Paint::fill(Color::RED),
        [128, 0, 127, 255],
        false,
    );
}

pub(super) fn canvas_uniform_translucent() {
    mesh_case(
        Some(&[Color::rgba(255, 0, 0, 128); 3]),
        &Paint::fill(Color::RED),
        [128, 0, 127, 255],
        true,
    );
}

fn mixed(canvas: bool) {
    // At (10.5, 10.5), barycentric weights are 11/24, 6.5/24, 6.5/24.
    // Straight alpha interpolates to (128 + 255) * 6.5/24 = 103.729...
    mesh_case(
        Some(&[
            Color::rgba(255, 0, 0, 0),
            Color::rgba(255, 0, 0, 128),
            Color::RED,
        ]),
        &Paint::fill(Color::RED),
        [104, 0, 151, 255],
        canvas,
    );
}
pub(super) fn mixed_alpha() {
    mixed(false);
}
pub(super) fn canvas_mixed_alpha() {
    mixed(true);
}

fn transparent(canvas: bool) {
    mesh_case(
        Some(&[Color::rgba(255, 0, 0, 0); 3]),
        &Paint::fill(Color::RED),
        [0, 0, 255, 255],
        canvas,
    );
}
pub(super) fn zero_alpha() {
    transparent(false);
}
pub(super) fn canvas_zero_alpha() {
    transparent(true);
}

fn opaque(canvas: bool) {
    // Paint alpha and RGB are ignored when colors are supplied.
    mesh_case(
        Some(&[Color::RED; 3]),
        &Paint::fill(Color::rgba(0, 255, 0, 0)),
        [255, 0, 0, 255],
        canvas,
    );
}
pub(super) fn all_opaque() {
    opaque(false);
}
pub(super) fn canvas_all_opaque() {
    opaque(true);
}

fn opaque_under_fractional_clip(supplied_colors: bool) {
    let rgba = render(|painter| {
        let mut recording = Canvas::new();
        recording.save();
        recording.clip_rect_ext(
            flui_foundation::geometry::Rect::from_xywh(10.5, 0.0, 21.5, 32.0),
            flui_painting::paint::ClipOp::Intersect,
            flui_painting::paint::Clip::AntiAlias,
        );
        recording.draw_vertices(
            triangle().to_vec(),
            supplied_colors.then(|| vec![Color::RED; 3]),
            None,
            INDICES.to_vec(),
            &Paint::fill(Color::RED),
        );
        recording.restore();
        let list = recording.finish();
        let mut dispatcher = crate::layer_dispatcher::LayerDispatcher::new(painter);
        for command in list.commands() {
            crate::dispatch::dispatch_command(command, &mut dispatcher);
        }
    });
    // The fragment center lies exactly on the clip edge: half coverage must
    // composite red over blue and keep the opaque destination's alpha.
    expect_pixel(&rgba, 10, 10, [128, 0, 127, 255]);
    expect_pixel(&rgba, 9, 10, [0, 0, 255, 255]);
    expect_pixel(&rgba, 11, 10, [255, 0, 0, 255]);
}

pub(super) fn opaque_vertex_colors_under_fractional_clip() {
    opaque_under_fractional_clip(true);
}

pub(super) fn opaque_paint_under_fractional_clip() {
    opaque_under_fractional_clip(false);
}

fn fallback(canvas: bool) {
    mesh_case(
        None,
        &Paint::fill(Color::rgba(255, 0, 0, 128)),
        [128, 0, 127, 255],
        canvas,
    );
    mesh_case(None, &Paint::fill(Color::RED), [255, 0, 0, 255], canvas);
}
pub(super) fn paint_fallback() {
    fallback(false);
}
pub(super) fn canvas_paint_fallback() {
    fallback(true);
}

fn explicit(mode: BlendMode, expected: [u8; 4], canvas: bool) {
    mesh_case(
        Some(&[Color::rgba(255, 0, 0, 128); 3]),
        &Paint::fill(Color::RED).with_blend_mode(mode),
        expected,
        canvas,
    );
}
pub(super) fn src() {
    explicit(BlendMode::Src, [128, 0, 0, 128], false);
}
pub(super) fn canvas_src() {
    explicit(BlendMode::Src, [128, 0, 0, 128], true);
}
pub(super) fn clear() {
    explicit(BlendMode::Clear, [0, 0, 0, 0], false);
}
pub(super) fn canvas_clear() {
    explicit(BlendMode::Clear, [0, 0, 0, 0], true);
}
pub(super) fn plus() {
    explicit(BlendMode::Plus, [128, 0, 255, 255], false);
}
pub(super) fn canvas_plus() {
    explicit(BlendMode::Plus, [128, 0, 255, 255], true);
}
pub(super) fn multiply() {
    explicit(BlendMode::Multiply, [0, 0, 127, 255], false);
}
pub(super) fn canvas_multiply() {
    explicit(BlendMode::Multiply, [0, 0, 127, 255], true);
}

fn grouped(canvas: bool) {
    let rgba = render(|painter| {
        painter.save_layer(None, &Paint::fill(Color::rgba(255, 255, 255, 128)));
        draw_mesh(
            painter,
            Some(&[Color::rgba(255, 0, 0, 128); 3]),
            &Paint::fill(Color::RED),
            canvas,
        );
        painter.restore_layer();
    });
    expect_pixel(&rgba, 10, 10, [64, 0, 191, 255]);
    expect_pixel(&rgba, 25, 25, [0, 0, 255, 255]);
}
pub(super) fn parent_opacity() {
    grouped(false);
}
pub(super) fn canvas_parent_opacity() {
    grouped(true);
}

pub(super) fn invalid_input_keeps_sibling() {
    let rgba = render(|painter| {
        painter.draw_vertices(
            &triangle(),
            Some(&[Color::RED; 2]),
            None,
            &INDICES,
            &Paint::fill(Color::RED),
        );
        painter.draw_vertices(&[], None, None, &INDICES, &Paint::fill(Color::RED));
        painter.draw_vertices(&triangle(), None, None, &[], &Paint::fill(Color::RED));
        painter.draw_rect(
            Rect::from_xywh(22.0, 22.0, 4.0, 4.0),
            &Paint::fill(Color::GREEN),
        );
    });
    expect_pixel(&rgba, 10, 10, [0, 0, 255, 255]);
    expect_pixel(&rgba, 23, 23, [0, 255, 0, 255]);
}

//! Solid shader readbacks use a separately drawn plain-color paint as the oracle.
use super::*;
use flui_foundation::geometry::{Matrix4, Point, RRect};
use flui_painting::{
    Canvas, Paint,
    paint::{Clip, Shader},
    styling::Color,
};

const SIZE: u32 = 48;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Rect,
    RRect,
    Circle,
}

#[derive(Clone, Copy, Debug)]
enum Route {
    Direct,
    Canvas,
}

fn bounds() -> Rect<f64> {
    Rect::from_xywh(4.0, 4.0, 8.0, 8.0)
}

fn record(
    p: &mut WgpuPainter,
    route: Route,
    shape: Shape,
    paint: &Paint,
    offset: f64,
    scoped: bool,
) {
    let mut matrix = Matrix4::IDENTITY;
    matrix.m[12] = offset;
    if scoped {
        matrix.m[4] = 0.5;
    }
    let group = Paint::fill(Color::rgba(255, 255, 255, 128));
    let clip = Rect::from_xywh(6.0, 6.0, 4.0, 4.0);
    match route {
        Route::Direct => {
            p.save();
            p.transform(&matrix);
            if scoped {
                p.clip_rect(clip, Clip::HardEdge);
                p.save_layer(None, &group);
            }
            match shape {
                Shape::Rect => p.draw_rect(bounds(), paint),
                Shape::RRect => p.draw_rrect(RRect::from_rect_circular(bounds(), 2.0), paint),
                Shape::Circle => p.draw_circle(Point::new(8.0, 8.0), 4.0, paint),
            }
            if scoped {
                p.restore_layer();
            }
            p.restore();
        }
        Route::Canvas => {
            let mut canvas = Canvas::new();
            canvas.save();
            canvas.transform(matrix);
            if scoped {
                canvas.clip_rect_ext(
                    clip,
                    flui_painting::paint::ClipOp::Intersect,
                    Clip::HardEdge,
                );
                canvas.save_layer(None, &group);
            }
            match shape {
                Shape::Rect => canvas.draw_rect(bounds(), paint),
                Shape::RRect => canvas.draw_rrect(RRect::from_rect_circular(bounds(), 2.0), paint),
                Shape::Circle => canvas.draw_circle(Point::new(8.0, 8.0), 4.0, paint),
            }
            if scoped {
                canvas.restore();
            }
            canvas.restore();
            let list = canvas.finish();
            let mut dispatcher = crate::layer_dispatcher::LayerDispatcher::new(p);
            for command in list.commands() {
                crate::dispatch::dispatch_command(command, &mut dispatcher);
            }
        }
    }
}

fn render(draw: impl FnOnce(&mut WgpuPainter)) -> Vec<u8> {
    let (device, queue) = test_device_and_queue();
    let (target, view) = crate::test_support::create_target(
        &device,
        "Solid shader contract",
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
                    load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    painter.begin_frame().expect("solid shader frame begins");
    draw(&mut painter);
    painter
        .render_to_texture(&target, &mut encoder)
        .expect("solid shader frame renders");
    painter
        .submit_encoder(encoder)
        .expect("solid shader submission completes");
    painter.finish_frame();
    crate::test_support::readback_bytes(&device, &queue, &target, SIZE, SIZE)
}

fn near(actual: [u8; 4], expected: [u8; 4], context: &str) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "{context}: actual RGBA {actual:?}, expected RGBA {expected:?}"
    );
}

fn compare(
    route: Route,
    shape: Shape,
    source: Color,
    paint_alpha: u8,
    mode: BlendMode,
    scoped: bool,
) -> [u8; 4] {
    let plain = Paint::fill(source).with_blend_mode(mode);
    // Deliberately contradictory RGB and alpha prove the shader supplies all RGBA.
    let solid = Paint::fill(Color::rgba(255, 0, 0, paint_alpha))
        .with_shader(Shader::solid(source))
        .with_blend_mode(mode);
    let rgba = render(|p| {
        record(p, route, shape, &plain, 0.0, scoped);
        record(p, route, shape, &solid, 24.0, scoped);
        p.draw_rect(
            Rect::from_xywh(1.0, 20.0, 3.0, 3.0),
            &Paint::fill(Color::RED),
        );
    });
    let x = if scoped { 12 } else { 8 };
    let expected = pixel_at(&rgba, SIZE, x, 8);
    let actual = pixel_at(&rgba, SIZE, x + 24, 8);
    near(
        actual,
        expected,
        &format!(
            "{route:?} {shape:?} source={source:?} paint alpha={paint_alpha} blend={mode:?} scoped={scoped}"
        ),
    );
    near(
        pixel_at(&rgba, SIZE, 2, 21),
        [255, 0, 0, 255],
        "plain RED remains RED after scope restoration",
    );
    near(
        pixel_at(&rgba, SIZE, 25, 1),
        [0, 255, 0, 255],
        "outside shape remains GREEN",
    );
    if scoped {
        near(
            pixel_at(&rgba, SIZE, 33, 8),
            [0, 255, 0, 255],
            "inside shape but outside captured clip remains GREEN",
        );
    }
    actual
}

fn effective_color_case(route: Route, shape: Shape, alpha: u8, paint_alpha: u8) {
    let actual = compare(
        route,
        shape,
        Color::rgba(0, 0, 255, alpha),
        paint_alpha,
        BlendMode::SrcOver,
        false,
    );
    near(
        actual,
        [0, 255 - alpha, alpha, 255],
        "BLUE Solid SrcOver on opaque GREEN",
    );
}

fn scoped_case(route: Route, alpha: u8) {
    // The served rect path already supports this affine shear. Rounded
    // shapes are covered at identity by the effective-color rows.
    let actual = compare(
        route,
        Shape::Rect,
        Color::rgba(0, 0, 255, alpha),
        255,
        BlendMode::SrcOver,
        true,
    );
    let effective_alpha = (f32::from(alpha) * (128.0 / 255.0)).round() as u8;
    near(
        actual,
        [0, 255 - effective_alpha, effective_alpha, 255],
        "shader alpha times parent group opacity",
    );
}

fn blend_case(route: Route, mode: BlendMode, alpha: u8) {
    let actual = compare(
        route,
        Shape::Rect,
        Color::rgba(0, 0, 255, alpha),
        255,
        mode,
        false,
    );
    if mode == BlendMode::Src {
        near(
            actual,
            [0, 0, alpha, alpha],
            "Src preserves source premultiplied RGBA, including transparent Solid",
        );
    }
}

// Each concrete input is a plain fn row so the parent family's existing
// panic-containing runner reports it and continues with the remaining rows.
macro_rules! solid_rows {
    ($( $name:ident => $body:expr; )*) => {
        $(fn $name() { $body; })*
        pub(super) const CASES: &[(&str, fn())] = &[
            $((stringify!($name), $name)),*
        ];
    };
}

solid_rows! {
    solid_direct_rect_opaque => effective_color_case(Route::Direct, Shape::Rect, 255, 255);
    solid_direct_rect_half_alpha => effective_color_case(Route::Direct, Shape::Rect, 128, 255);
    solid_direct_rect_transparent => effective_color_case(Route::Direct, Shape::Rect, 0, 255);
    solid_direct_rect_contradictory_paint_alpha => effective_color_case(Route::Direct, Shape::Rect, 255, 0);
    solid_direct_rrect_opaque => effective_color_case(Route::Direct, Shape::RRect, 255, 255);
    solid_direct_rrect_half_alpha => effective_color_case(Route::Direct, Shape::RRect, 128, 255);
    solid_direct_rrect_transparent => effective_color_case(Route::Direct, Shape::RRect, 0, 255);
    solid_direct_rrect_contradictory_paint_alpha => effective_color_case(Route::Direct, Shape::RRect, 255, 0);
    solid_direct_circle_opaque => effective_color_case(Route::Direct, Shape::Circle, 255, 255);
    solid_direct_circle_half_alpha => effective_color_case(Route::Direct, Shape::Circle, 128, 255);
    solid_direct_circle_transparent => effective_color_case(Route::Direct, Shape::Circle, 0, 255);
    solid_direct_circle_contradictory_paint_alpha => effective_color_case(Route::Direct, Shape::Circle, 255, 0);
    solid_direct_shear_clip_parent_opaque => scoped_case(Route::Direct, 255);
    solid_direct_shear_clip_parent_half_alpha => scoped_case(Route::Direct, 128);
    solid_direct_src_half_alpha => blend_case(Route::Direct, BlendMode::Src, 128);
    solid_direct_src_transparent => blend_case(Route::Direct, BlendMode::Src, 0);
    solid_direct_multiply_half_alpha => blend_case(Route::Direct, BlendMode::Multiply, 128);
    solid_direct_multiply_transparent => blend_case(Route::Direct, BlendMode::Multiply, 0);
    solid_canvas_rect_opaque => effective_color_case(Route::Canvas, Shape::Rect, 255, 255);
    solid_canvas_rect_half_alpha => effective_color_case(Route::Canvas, Shape::Rect, 128, 255);
    solid_canvas_rect_transparent => effective_color_case(Route::Canvas, Shape::Rect, 0, 255);
    solid_canvas_rect_contradictory_paint_alpha => effective_color_case(Route::Canvas, Shape::Rect, 255, 0);
    solid_canvas_rrect_opaque => effective_color_case(Route::Canvas, Shape::RRect, 255, 255);
    solid_canvas_rrect_half_alpha => effective_color_case(Route::Canvas, Shape::RRect, 128, 255);
    solid_canvas_rrect_transparent => effective_color_case(Route::Canvas, Shape::RRect, 0, 255);
    solid_canvas_rrect_contradictory_paint_alpha => effective_color_case(Route::Canvas, Shape::RRect, 255, 0);
    solid_canvas_circle_opaque => effective_color_case(Route::Canvas, Shape::Circle, 255, 255);
    solid_canvas_circle_half_alpha => effective_color_case(Route::Canvas, Shape::Circle, 128, 255);
    solid_canvas_circle_transparent => effective_color_case(Route::Canvas, Shape::Circle, 0, 255);
    solid_canvas_circle_contradictory_paint_alpha => effective_color_case(Route::Canvas, Shape::Circle, 255, 0);
    solid_canvas_shear_clip_parent_opaque => scoped_case(Route::Canvas, 255);
    solid_canvas_shear_clip_parent_half_alpha => scoped_case(Route::Canvas, 128);
    solid_canvas_src_half_alpha => blend_case(Route::Canvas, BlendMode::Src, 128);
    solid_canvas_src_transparent => blend_case(Route::Canvas, BlendMode::Src, 0);
    solid_canvas_multiply_half_alpha => blend_case(Route::Canvas, BlendMode::Multiply, 128);
    solid_canvas_multiply_transparent => blend_case(Route::Canvas, BlendMode::Multiply, 0);
}

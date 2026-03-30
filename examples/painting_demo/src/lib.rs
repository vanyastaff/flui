//! FLUI Painting Demo
//!
//! Demonstrates flui-painting and flui-engine rendering primitives in a browser
//! via WebGPU. Build with:
//!   cd examples/painting_demo && wasm-pack build --target web --out-dir pkg
//! Then serve with any HTTP server and open index.html.

use std::sync::Arc;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub async fn main() {
    console_error_panic_hook::set_once();

    web_sys::console::log_1(&"FLUI Painting Demo starting...".into());

    // Get the canvas element
    let window = web_sys::window().expect("no global window");
    let document = window.document().expect("no document");
    let canvas = document
        .get_element_by_id("flui-canvas")
        .expect("no canvas element with id 'flui-canvas'");
    let canvas: web_sys::HtmlCanvasElement = canvas
        .dyn_into()
        .expect("element is not an HtmlCanvasElement");

    let width = canvas.width();
    let height = canvas.height();

    // Create wgpu instance targeting WebGPU
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..Default::default()
    });

    // Create surface from the canvas
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .expect("failed to create surface from canvas");

    // Request adapter
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .expect("no suitable GPU adapter found");

    web_sys::console::log_1(
        &format!("Adapter: {:?}", adapter.get_info().name).into(),
    );

    // Request device
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .expect("failed to request device");

    let device = Arc::new(device);
    let queue = Arc::new(queue);

    // Configure the surface
    let config = surface
        .get_default_config(&adapter, width, height)
        .expect("surface not supported by adapter");
    surface.configure(&device, &config);

    // Create WgpuPainter
    let mut painter = flui_engine::WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        config.format,
        (width, height),
    );

    // Draw demo content using the Painter trait
    draw_demo(&mut painter);

    // Render to the surface
    let output = surface
        .get_current_texture()
        .expect("failed to get current texture");
    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("painting_demo_encoder"),
        });

    // Clear the background
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.1,
                        g: 0.1,
                        b: 0.18,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    // Flush painter batches to the GPU
    if let Err(e) = painter.render(&view, &mut encoder) {
        web_sys::console::error_1(&format!("Painter render error: {e}").into());
    }

    queue.submit(std::iter::once(encoder.finish()));
    output.present();

    web_sys::console::log_1(&"FLUI Painting Demo rendered successfully!".into());
}

/// Draw various painting primitives to demonstrate the engine.
fn draw_demo(painter: &mut flui_engine::WgpuPainter) {
    use flui_engine::Painter;
    use flui_types::geometry::{Point, RRect, Rect, px};
    use flui_types::painting::Paint;
    use flui_types::styling::Color;

    // === 1. Colored rectangles ===
    let red_paint = Paint::fill(Color::RED);
    painter.rect(
        Rect::from_xywh(px(50.0), px(50.0), px(200.0), px(100.0)),
        &red_paint,
    );

    let blue_paint = Paint::fill(Color::BLUE);
    painter.rect(
        Rect::from_xywh(px(300.0), px(50.0), px(200.0), px(100.0)),
        &blue_paint,
    );

    let green_paint = Paint::fill(Color::GREEN);
    painter.rect(
        Rect::from_xywh(px(550.0), px(50.0), px(200.0), px(100.0)),
        &green_paint,
    );

    // === 2. Rounded rectangle ===
    let purple_paint = Paint::fill(Color::rgba(128, 0, 255, 255));
    let rrect = RRect::from_rect_circular(
        Rect::from_xywh(px(50.0), px(200.0), px(300.0), px(100.0)),
        px(20.0),
    );
    painter.rrect(rrect, &purple_paint);

    // === 3. Circle ===
    let yellow_paint = Paint::fill(Color::rgba(255, 255, 0, 255));
    painter.circle(Point::new(px(550.0), px(250.0)), 60.0, &yellow_paint);

    // === 4. Stroked rectangle (border) ===
    let stroke_paint = Paint::stroke(Color::WHITE, 3.0);
    painter.rect(
        Rect::from_xywh(px(50.0), px(350.0), px(700.0), px(200.0)),
        &stroke_paint,
    );

    // === 5. Line ===
    let line_paint = Paint::stroke(Color::rgba(255, 128, 0, 255), 2.0);
    painter.line(
        Point::new(px(100.0), px(400.0)),
        Point::new(px(700.0), px(500.0)),
        &line_paint,
    );

    // === 6. Text ===
    let text_paint = Paint::fill(Color::WHITE);
    painter.text(
        "Hello, FLUI!",
        Point::new(px(250.0), px(450.0)),
        32.0,
        &text_paint,
    );

    // === 7. Additional shapes ===
    // Cyan circle
    let cyan_paint = Paint::fill(Color::rgba(0, 255, 255, 255));
    painter.circle(Point::new(px(450.0), px(250.0)), 40.0, &cyan_paint);

    // Small orange rounded rect
    let orange_paint = Paint::fill(Color::rgba(255, 165, 0, 255));
    let small_rrect = RRect::from_rect_circular(
        Rect::from_xywh(px(650.0), px(200.0), px(100.0), px(100.0)),
        px(15.0),
    );
    painter.rrect(small_rrect, &orange_paint);

    web_sys::console::log_1(&"Drew demo primitives".into());
}

//! FLUI Painting Demo — Comprehensive
//!
//! Demonstrates ALL flui-painting and flui-engine rendering primitives in a
//! browser via WebGPU. Build with:
//!   cd examples/painting_demo && wasm-pack build --target web --out-dir pkg
//! Then serve with any HTTP server and open index.html.

use std::sync::Arc;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub async fn main() {
    console_error_panic_hook::set_once();
    web_sys::console::log_1(&"FLUI Painting Demo starting...".into());

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

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    // SAFETY: The boxed JsValue is heap-allocated and lives for the duration of
    // the unsafe block; NonNull is non-null by construction via Box::into_raw.
    // The resulting surface is used immediately in this function and the canvas
    // outlives both the surface and the wgpu instance (canvas is owned by the
    // calling JS context for the page lifetime).
    #[allow(unsafe_code)]
    let surface = unsafe {
        use std::ptr::NonNull;
        let obj: JsValue = canvas.clone().into();
        let ptr = NonNull::new_unchecked(Box::into_raw(Box::new(obj)) as *mut std::ffi::c_void);
        let handle = raw_window_handle::WebCanvasWindowHandle::new(ptr);
        let raw_window = raw_window_handle::RawWindowHandle::WebCanvas(handle);
        let raw_display =
            raw_window_handle::RawDisplayHandle::Web(raw_window_handle::WebDisplayHandle::new());
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display),
            raw_window_handle: raw_window,
        };
        instance.create_surface_unsafe(target)
    }
    .expect("failed to create surface from canvas");

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .expect("no suitable GPU adapter found");

    web_sys::console::log_1(&format!("Adapter: {:?}", adapter.get_info().name).into());

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("failed to request device");

    let device = Arc::new(device);
    let queue = Arc::new(queue);

    let config = surface
        .get_default_config(&adapter, width, height)
        .expect("surface not supported by adapter");
    surface.configure(&device, &config);

    let mut painter = flui_engine::WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        config.format,
        (width, height),
    );

    draw_all_demos(&mut painter);

    let output = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        other => {
            web_sys::console::error_1(&format!("get_current_texture failed: {other:?}").into());
            return;
        }
    };
    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("painting_demo_encoder"),
    });

    // Clear background
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.06,
                        g: 0.06,
                        b: 0.10,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    if let Err(e) = painter.render(&view, &mut encoder) {
        web_sys::console::error_1(&format!("Painter render error: {e}").into());
    }

    queue.submit(std::iter::once(encoder.finish()));
    output.present();

    web_sys::console::log_1(&"FLUI Painting Demo rendered successfully!".into());
}

/// Master function that draws all demo sections.
fn draw_all_demos(painter: &mut flui_engine::WgpuPainter) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let section_gap = 30.0;
    let mut y_offset = 20.0;

    let label_paint =
        flui_types::painting::Paint::fill(flui_types::styling::Color::rgba(0, 210, 255, 255));

    // === Section 1: Basic Shapes ===
    painter.text("1. Basic Shapes", pt(30.0, y_offset), 22.0, &label_paint);
    y_offset += 35.0;
    draw_basic_shapes(painter, y_offset);
    y_offset += 130.0 + section_gap;

    // === Section 2: Rounded Rectangles ===
    painter.text(
        "2. Rounded Rectangles",
        pt(30.0, y_offset),
        22.0,
        &label_paint,
    );
    y_offset += 35.0;
    draw_rounded_rects(painter, y_offset);
    y_offset += 110.0 + section_gap;

    // === Section 3: Circles & Ovals ===
    painter.text("3. Circles & Ovals", pt(30.0, y_offset), 22.0, &label_paint);
    y_offset += 35.0;
    draw_circles_and_ovals(painter, y_offset);
    y_offset += 130.0 + section_gap;

    // === Section 4: Lines & Strokes ===
    painter.text("4. Lines & Strokes", pt(30.0, y_offset), 22.0, &label_paint);
    y_offset += 35.0;
    draw_lines_and_strokes(painter, y_offset);
    y_offset += 100.0 + section_gap;

    // === Section 5: Dashed Lines ===
    painter.text("5. Dashed Lines", pt(30.0, y_offset), 22.0, &label_paint);
    y_offset += 35.0;
    draw_dashed_lines(painter, y_offset);
    y_offset += 80.0 + section_gap;

    // === Section 6: Paths & Polygons ===
    painter.text(
        "6. Paths & Polygons",
        pt(30.0, y_offset),
        22.0,
        &label_paint,
    );
    y_offset += 35.0;
    draw_paths(painter, y_offset);
    y_offset += 150.0 + section_gap;

    // === Section 7: Arcs ===
    painter.text("7. Arcs", pt(30.0, y_offset), 22.0, &label_paint);
    y_offset += 35.0;
    draw_arcs(painter, y_offset);
    y_offset += 130.0 + section_gap;

    // === Section 8: Gradients ===
    painter.text(
        "8. Gradients (Linear, Radial, Sweep)",
        pt(30.0, y_offset),
        22.0,
        &label_paint,
    );
    y_offset += 35.0;
    draw_gradients(painter, y_offset);
    y_offset += 130.0 + section_gap;

    // === Section 9: Transforms ===
    painter.text(
        "9. Transforms (translate, rotate, scale)",
        pt(30.0, y_offset),
        22.0,
        &label_paint,
    );
    y_offset += 35.0;
    draw_transforms(painter, y_offset);
    y_offset += 160.0 + section_gap;

    // === Section 10: Clipping ===
    painter.text("10. Clipping", pt(30.0, y_offset), 22.0, &label_paint);
    y_offset += 35.0;
    draw_clipping(painter, y_offset);
    y_offset += 130.0 + section_gap;

    // === Section 11: Double Rounded Rect (drrect) ===
    painter.text(
        "11. Double Rounded Rect (Frame)",
        pt(30.0, y_offset),
        22.0,
        &label_paint,
    );
    y_offset += 35.0;
    draw_drrect(painter, y_offset);
    y_offset += 130.0 + section_gap;

    // === Section 12: Text ===
    painter.text("12. Text Rendering", pt(30.0, y_offset), 22.0, &label_paint);
    y_offset += 35.0;
    draw_text(painter, y_offset);
    y_offset += 130.0 + section_gap;

    // === Section 13: Opacity & Blending ===
    painter.text(
        "13. Opacity & Blending",
        pt(30.0, y_offset),
        22.0,
        &label_paint,
    );
    y_offset += 35.0;
    draw_opacity(painter, y_offset);
    let _ = y_offset;

    web_sys::console::log_1(&"All demo sections drawn".into());
}

// ============================================================
// Helper functions
// ============================================================

use flui_types::geometry::{Offset, Pixels, Point, RRect, Rect, px};
use flui_types::painting::{Paint, Shader, path::Path};
use flui_types::styling::Color;

fn pt(x: f32, y: f32) -> Point<Pixels> {
    Point::new(px(x), px(y))
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect<Pixels> {
    Rect::from_xywh(px(x), px(y), px(w), px(h))
}

fn ofs(dx: f32, dy: f32) -> Offset<Pixels> {
    Offset::new(px(dx), px(dy))
}

// ============================================================
// Demo sections
// ============================================================

/// 1. Basic filled rectangles
fn draw_basic_shapes(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let colors = [
        (Color::RED, "Red"),
        (Color::GREEN, "Green"),
        (Color::BLUE, "Blue"),
        (Color::YELLOW, "Yellow"),
        (Color::CYAN, "Cyan"),
        (Color::MAGENTA, "Magenta"),
    ];

    let label_paint = Paint::fill(Color::WHITE);
    for (i, (color, name)) in colors.iter().enumerate() {
        let x = 30.0 + i as f32 * 190.0;
        painter.rect(rect(x, y, 170.0, 90.0), &Paint::fill(*color));
        painter.text(name, pt(x + 55.0, y + 100.0), 14.0, &label_paint);
    }
}

/// 2. Rounded rects with varying radii
fn draw_rounded_rects(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let radii = [5.0, 15.0, 30.0, 50.0];
    let colors = [
        Color::rgba(128, 0, 255, 255),
        Color::rgba(255, 100, 50, 255),
        Color::rgba(50, 200, 100, 255),
        Color::rgba(200, 50, 200, 255),
    ];

    let label_paint = Paint::fill(Color::WHITE);
    for (i, (radius, color)) in radii.iter().zip(colors.iter()).enumerate() {
        let x = 30.0 + i as f32 * 280.0;
        let rrect = RRect::from_rect_circular(rect(x, y, 250.0, 80.0), px(*radius));
        painter.rrect(rrect, &Paint::fill(*color));
        painter.text(
            &format!("r={radius}"),
            pt(x + 100.0, y + 90.0),
            13.0,
            &label_paint,
        );
    }
}

/// 3. Circles and ovals
fn draw_circles_and_ovals(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    // Filled circles
    let circle_colors = [
        Color::rgba(255, 80, 80, 255),
        Color::rgba(80, 255, 80, 255),
        Color::rgba(80, 80, 255, 255),
        Color::rgba(255, 200, 50, 255),
    ];
    for (i, color) in circle_colors.iter().enumerate() {
        let cx = 90.0 + i as f32 * 130.0;
        painter.circle(pt(cx, y + 55.0), 50.0, &Paint::fill(*color));
    }

    // Stroked circles over filled ones
    for (i, color) in circle_colors.iter().enumerate() {
        let cx = 90.0 + i as f32 * 130.0;
        painter.circle(pt(cx, y + 55.0), 50.0, &Paint::stroke(*color, 2.0));
    }

    // Ovals
    let oval_colors = [
        Color::rgba(255, 128, 0, 200),
        Color::rgba(0, 200, 200, 200),
        Color::rgba(200, 100, 255, 200),
    ];
    for (i, color) in oval_colors.iter().enumerate() {
        let x = 600.0 + i as f32 * 200.0;
        painter.oval(rect(x, y + 5.0, 170.0, 100.0), &Paint::fill(*color));
    }
}

/// 4. Lines and stroked shapes
fn draw_lines_and_strokes(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    // Lines of varying widths
    let widths = [1.0, 2.0, 3.0, 5.0, 8.0];
    let line_colors = [
        Color::rgba(255, 50, 50, 255),
        Color::rgba(50, 255, 50, 255),
        Color::rgba(50, 50, 255, 255),
        Color::rgba(255, 255, 50, 255),
        Color::rgba(255, 50, 255, 255),
    ];
    let label_paint = Paint::fill(Color::LIGHT_GRAY);
    for (i, (w, color)) in widths.iter().zip(line_colors.iter()).enumerate() {
        let ly = y + i as f32 * 18.0;
        painter.line(pt(30.0, ly), pt(500.0, ly), &Paint::stroke(*color, *w));
        painter.text(&format!("{w}px"), pt(510.0, ly - 6.0), 12.0, &label_paint);
    }

    // Stroked shapes
    painter.rect(
        rect(600.0, y, 150.0, 80.0),
        &Paint::stroke(Color::WHITE, 2.0),
    );
    painter.text("stroke rect", pt(620.0, y + 85.0), 12.0, &label_paint);

    let rrect = RRect::from_rect_circular(rect(780.0, y, 150.0, 80.0), px(15.0));
    painter.rrect(rrect, &Paint::stroke(Color::rgba(100, 200, 255, 255), 3.0));
    painter.text("stroke rrect", pt(800.0, y + 85.0), 12.0, &label_paint);

    painter.circle(
        pt(1030.0, y + 40.0),
        40.0,
        &Paint::stroke(Color::rgba(255, 200, 100, 255), 3.0),
    );
    painter.text("stroke circle", pt(990.0, y + 85.0), 12.0, &label_paint);
}

/// 5. Dashed lines
fn draw_dashed_lines(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let patterns: &[(&[f32], &str)] = &[
        (&[10.0, 5.0], "10-5"),
        (&[20.0, 10.0], "20-10"),
        (&[5.0, 5.0, 15.0, 5.0], "5-5-15-5"),
        (&[2.0, 8.0], "2-8 (dots)"),
    ];

    let label_paint = Paint::fill(Color::LIGHT_GRAY);
    for (i, (intervals, name)) in patterns.iter().enumerate() {
        let ly = y + i as f32 * 18.0;
        let dash_paint =
            Paint::stroke(Color::rgba(200, 200, 255, 255), 2.0).with_dash(intervals.to_vec(), 0.0);
        painter.line(pt(30.0, ly), pt(600.0, ly), &dash_paint);
        painter.text(name, pt(620.0, ly - 6.0), 12.0, &label_paint);
    }
}

/// 6. Paths & polygons (star, triangle, pentagon, custom bezier)
fn draw_paths(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let label_paint = Paint::fill(Color::LIGHT_GRAY);

    // Triangle
    let tri = Path::polygon(&[pt(90.0, y), pt(30.0, y + 120.0), pt(150.0, y + 120.0)]);
    painter.draw_path(&tri, &Paint::fill(Color::rgba(255, 100, 50, 255)));
    painter.text("Triangle", pt(50.0, y + 135.0), 12.0, &label_paint);

    // Star (5-pointed)
    let star = make_star(pt(280.0, y + 65.0), 60.0, 25.0, 5);
    painter.draw_path(&star, &Paint::fill(Color::rgba(255, 220, 50, 255)));
    painter.text("Star", pt(260.0, y + 135.0), 12.0, &label_paint);

    // Pentagon
    let pentagon = make_regular_polygon(pt(450.0, y + 65.0), 55.0, 5);
    painter.draw_path(&pentagon, &Paint::fill(Color::rgba(50, 200, 150, 255)));
    painter.text("Pentagon", pt(420.0, y + 135.0), 12.0, &label_paint);

    // Hexagon
    let hexagon = make_regular_polygon(pt(610.0, y + 65.0), 55.0, 6);
    painter.draw_path(&hexagon, &Paint::fill(Color::rgba(150, 50, 255, 255)));
    painter.text("Hexagon", pt(580.0, y + 135.0), 12.0, &label_paint);

    // Octagon (stroked)
    let octagon = make_regular_polygon(pt(770.0, y + 65.0), 55.0, 8);
    painter.draw_path(
        &octagon,
        &Paint::stroke(Color::rgba(255, 100, 200, 255), 2.0),
    );
    painter.text("Octagon", pt(740.0, y + 135.0), 12.0, &label_paint);

    // Custom zigzag path
    let mut zigzag = Path::new();
    zigzag.move_to(pt(880.0, y + 120.0));
    zigzag.line_to(pt(920.0, y));
    zigzag.line_to(pt(960.0, y + 60.0));
    zigzag.line_to(pt(1000.0, y));
    zigzag.line_to(pt(1040.0, y + 120.0));
    zigzag.close();
    painter.draw_path(&zigzag, &Paint::fill(Color::rgba(100, 200, 255, 200)));
    painter.text("Zigzag", pt(935.0, y + 135.0), 12.0, &label_paint);
}

/// 7. Arcs
fn draw_arcs(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    let label_paint = Paint::fill(Color::LIGHT_GRAY);

    // Pie slices (use_center = true)
    let arc_data: &[(f32, f32, Color, &str)] = &[
        (0.0, FRAC_PI_2, Color::rgba(255, 80, 80, 220), "90 deg"),
        (0.0, PI, Color::rgba(80, 255, 80, 220), "180 deg"),
        (PI * 0.25, PI, Color::rgba(80, 80, 255, 220), "45-225 deg"),
        (0.0, TAU * 0.75, Color::rgba(255, 200, 50, 220), "270 deg"),
    ];

    for (i, (start, sweep, color, name)) in arc_data.iter().enumerate() {
        let x = 80.0 + i as f32 * 200.0;
        let r = rect(x - 50.0, y, 100.0, 100.0);
        painter.draw_arc(r, *start, *sweep, true, &Paint::fill(*color));
        painter.text(name, pt(x - 25.0, y + 110.0), 12.0, &label_paint);
    }

    // Open arcs (use_center = false)
    for (i, (start, sweep, color, _)) in arc_data.iter().enumerate() {
        let x = 880.0 + i as f32 * 80.0;
        let r = rect(x - 30.0, y + 10.0, 60.0, 60.0);
        painter.draw_arc(r, *start, *sweep, false, &Paint::stroke(*color, 3.0));
    }
    painter.text("Open arcs", pt(870.0, y + 110.0), 12.0, &label_paint);
}

/// 8. Gradients
fn draw_gradients(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let label_paint = Paint::fill(Color::LIGHT_GRAY);

    // Linear gradient (horizontal rainbow)
    let linear = Shader::simple_linear(
        ofs(30.0, y),
        ofs(330.0, y),
        vec![
            Color::RED,
            Color::YELLOW,
            Color::GREEN,
            Color::CYAN,
            Color::BLUE,
        ],
    );
    painter.rect(
        rect(30.0, y, 300.0, 100.0),
        &Paint::fill(Color::WHITE).with_shader(linear),
    );
    painter.text("Linear (rainbow)", pt(100.0, y + 110.0), 12.0, &label_paint);

    // Linear gradient (diagonal)
    let diag = Shader::simple_linear(
        ofs(370.0, y),
        ofs(640.0, y + 100.0),
        vec![Color::rgba(255, 0, 128, 255), Color::rgba(0, 128, 255, 255)],
    );
    painter.rect(
        rect(370.0, y, 270.0, 100.0),
        &Paint::fill(Color::WHITE).with_shader(diag),
    );
    painter.text(
        "Linear (diagonal)",
        pt(430.0, y + 110.0),
        12.0,
        &label_paint,
    );

    // Radial gradient
    let radial = Shader::simple_radial(
        ofs(750.0, y + 50.0),
        50.0,
        vec![
            Color::WHITE,
            Color::rgba(255, 100, 50, 255),
            Color::rgba(50, 0, 100, 255),
        ],
    );
    painter.circle(
        pt(750.0, y + 50.0),
        50.0,
        &Paint::fill(Color::WHITE).with_shader(radial),
    );
    painter.text("Radial", pt(725.0, y + 110.0), 12.0, &label_paint);

    // Sweep gradient
    let sweep = Shader::simple_sweep(
        ofs(920.0, y + 50.0),
        vec![
            Color::RED,
            Color::YELLOW,
            Color::GREEN,
            Color::CYAN,
            Color::BLUE,
            Color::MAGENTA,
            Color::RED,
        ],
    );
    painter.circle(
        pt(920.0, y + 50.0),
        50.0,
        &Paint::fill(Color::WHITE).with_shader(sweep),
    );
    painter.text("Sweep", pt(900.0, y + 110.0), 12.0, &label_paint);

    // Gradient on rounded rect
    let rrect_grad = Shader::simple_linear(
        ofs(1000.0, y),
        ofs(1170.0, y + 100.0),
        vec![
            Color::rgba(255, 50, 200, 255),
            Color::rgba(50, 200, 255, 255),
        ],
    );
    let rrect = RRect::from_rect_circular(rect(1000.0, y, 170.0, 100.0), px(20.0));
    painter.rrect(rrect, &Paint::fill(Color::WHITE).with_shader(rrect_grad));
    painter.text("Gradient rrect", pt(1020.0, y + 110.0), 12.0, &label_paint);
}

/// 9. Transforms
fn draw_transforms(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let label_paint = Paint::fill(Color::LIGHT_GRAY);

    // Translate
    painter.save();
    painter.translate(ofs(100.0, y + 70.0));
    painter.rect(
        rect(-40.0, -30.0, 80.0, 60.0),
        &Paint::fill(Color::rgba(200, 80, 80, 200)),
    );
    painter.restore();
    painter.text("Translate", pt(65.0, y + 130.0), 12.0, &label_paint);

    // Rotate (multiple rotated rects fan)
    let center_x = 300.0;
    let center_y = y + 70.0;
    let rot_colors = [
        Color::rgba(255, 50, 50, 100),
        Color::rgba(50, 255, 50, 100),
        Color::rgba(50, 50, 255, 100),
        Color::rgba(255, 255, 50, 100),
        Color::rgba(255, 50, 255, 100),
        Color::rgba(50, 255, 255, 100),
    ];
    for (i, color) in rot_colors.iter().enumerate() {
        painter.save();
        painter.translate(ofs(center_x, center_y));
        painter.rotate(i as f32 * std::f32::consts::PI / 6.0);
        painter.rect(rect(-50.0, -15.0, 100.0, 30.0), &Paint::fill(*color));
        painter.restore();
    }
    painter.text("Rotate", pt(275.0, y + 145.0), 12.0, &label_paint);

    // Scale
    painter.save();
    painter.translate(ofs(500.0, y + 70.0));
    for (i, s) in [0.5_f32, 0.75, 1.0, 1.25].iter().enumerate() {
        painter.save();
        painter.scale(*s, *s);
        let alpha = 100 + i as u8 * 40;
        painter.rect(
            rect(-30.0, -20.0, 60.0, 40.0),
            &Paint::fill(Color::rgba(100, 200, 255, alpha)),
        );
        painter.restore();
    }
    painter.restore();
    painter.text("Scale", pt(480.0, y + 145.0), 12.0, &label_paint);

    // Combined: translate + rotate + scale
    painter.save();
    painter.translate(ofs(700.0, y + 70.0));
    painter.rotate(0.3);
    painter.scale(1.5, 0.8);
    painter.rect(
        rect(-40.0, -25.0, 80.0, 50.0),
        &Paint::fill(Color::rgba(255, 150, 50, 200)),
    );
    painter.restore();
    painter.text("Combined", pt(670.0, y + 145.0), 12.0, &label_paint);

    // Nested transforms
    painter.save();
    painter.translate(ofs(900.0, y + 70.0));
    painter.rect(
        rect(-50.0, -50.0, 100.0, 100.0),
        &Paint::stroke(Color::GRAY, 1.0),
    );
    painter.save();
    painter.rotate(std::f32::consts::FRAC_PI_4);
    painter.rect(
        rect(-35.0, -35.0, 70.0, 70.0),
        &Paint::fill(Color::rgba(150, 100, 255, 180)),
    );
    painter.save();
    painter.rotate(std::f32::consts::FRAC_PI_4);
    painter.rect(
        rect(-20.0, -20.0, 40.0, 40.0),
        &Paint::fill(Color::rgba(255, 100, 150, 180)),
    );
    painter.restore();
    painter.restore();
    painter.restore();
    painter.text("Nested", pt(880.0, y + 145.0), 12.0, &label_paint);
}

/// 10. Clipping
fn draw_clipping(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let label_paint = Paint::fill(Color::LIGHT_GRAY);

    // Clip rect with gradient
    painter.save();
    painter.clip_rect(rect(30.0, y, 200.0, 100.0));
    let grad = Shader::simple_linear(
        ofs(0.0, y),
        ofs(500.0, y + 200.0),
        vec![Color::RED, Color::BLUE],
    );
    painter.rect(
        rect(0.0, y - 50.0, 500.0, 250.0),
        &Paint::fill(Color::WHITE).with_shader(grad),
    );
    painter.restore();
    painter.rect(
        rect(30.0, y, 200.0, 100.0),
        &Paint::stroke(Color::WHITE, 1.0),
    );
    painter.text("clip_rect", pt(80.0, y + 110.0), 12.0, &label_paint);

    // Clip rect with circles
    painter.save();
    painter.clip_rect(rect(300.0, y, 200.0, 100.0));
    for i in 0..8 {
        let cx = 300.0 + i as f32 * 30.0;
        let color = Color::rgba(
            (i * 35) as u8,
            (255 - i * 30) as u8,
            (i * 25 + 50) as u8,
            255,
        );
        painter.circle(pt(cx, y + 50.0), 40.0, &Paint::fill(color));
    }
    painter.restore();
    painter.rect(
        rect(300.0, y, 200.0, 100.0),
        &Paint::stroke(Color::WHITE, 1.0),
    );
    painter.text("Clipped circles", pt(340.0, y + 110.0), 12.0, &label_paint);

    // Nested clips
    painter.save();
    painter.clip_rect(rect(570.0, y, 250.0, 100.0));
    painter.rect(
        rect(570.0, y, 250.0, 100.0),
        &Paint::fill(Color::rgba(40, 40, 60, 255)),
    );
    painter.save();
    painter.clip_rect(rect(590.0, y + 10.0, 100.0, 80.0));
    painter.rect(
        rect(550.0, y - 10.0, 300.0, 120.0),
        &Paint::fill(Color::rgba(255, 80, 80, 200)),
    );
    painter.restore();
    painter.save();
    painter.clip_rect(rect(700.0, y + 10.0, 100.0, 80.0));
    painter.rect(
        rect(550.0, y - 10.0, 300.0, 120.0),
        &Paint::fill(Color::rgba(80, 80, 255, 200)),
    );
    painter.restore();
    painter.restore();
    painter.rect(
        rect(570.0, y, 250.0, 100.0),
        &Paint::stroke(Color::WHITE, 1.0),
    );
    painter.text("Nested clips", pt(640.0, y + 110.0), 12.0, &label_paint);
}

/// 11. Double rounded rect
fn draw_drrect(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let label_paint = Paint::fill(Color::LIGHT_GRAY);

    // Thick border frame
    let outer1 = RRect::from_rect_circular(rect(30.0, y, 200.0, 100.0), px(20.0));
    let inner1 = RRect::from_rect_circular(rect(45.0, y + 15.0, 170.0, 70.0), px(10.0));
    painter.draw_drrect(outer1, inner1, &Paint::fill(Color::rgba(255, 100, 50, 255)));
    painter.text("Thick frame", pt(80.0, y + 110.0), 12.0, &label_paint);

    // Gradient frame
    let outer2 = RRect::from_rect_circular(rect(280.0, y, 200.0, 100.0), px(30.0));
    let inner2 = RRect::from_rect_circular(rect(290.0, y + 10.0, 180.0, 80.0), px(20.0));
    let frame_grad = Shader::simple_linear(
        ofs(280.0, y),
        ofs(480.0, y + 100.0),
        vec![
            Color::rgba(255, 50, 200, 255),
            Color::rgba(50, 200, 255, 255),
        ],
    );
    painter.draw_drrect(
        outer2,
        inner2,
        &Paint::fill(Color::WHITE).with_shader(frame_grad),
    );
    painter.text("Gradient frame", pt(320.0, y + 110.0), 12.0, &label_paint);

    // Thin outline frame
    let outer3 = RRect::from_rect_circular(rect(530.0, y, 200.0, 100.0), px(15.0));
    let inner3 = RRect::from_rect_circular(rect(534.0, y + 4.0, 192.0, 92.0), px(12.0));
    painter.draw_drrect(
        outer3,
        inner3,
        &Paint::fill(Color::rgba(100, 255, 150, 255)),
    );
    painter.text("Thin frame", pt(580.0, y + 110.0), 12.0, &label_paint);

    // Asymmetric radii frame
    let outer4 = RRect::from_rect_circular(rect(780.0, y, 200.0, 100.0), px(40.0));
    let inner4 = RRect::from_rect_circular(rect(800.0, y + 20.0, 160.0, 60.0), px(5.0));
    painter.draw_drrect(outer4, inner4, &Paint::fill(Color::rgba(255, 200, 50, 255)));
    painter.text("Asymmetric", pt(835.0, y + 110.0), 12.0, &label_paint);
}

/// 12. Text rendering
fn draw_text(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let sizes = [12.0, 16.0, 20.0, 28.0, 36.0];
    let mut text_y = y;
    for size in sizes {
        let paint = Paint::fill(Color::WHITE);
        painter.text(
            &format!("Font size {size}px — Hello FLUI!"),
            pt(30.0, text_y),
            size,
            &paint,
        );
        text_y += size + 8.0;
    }
}

/// 13. Opacity & blending
fn draw_opacity(painter: &mut flui_engine::WgpuPainter, y: f32) {
    // Painter trait deleted in Mythos U5; methods are inherent on WgpuPainter

    let label_paint = Paint::fill(Color::LIGHT_GRAY);

    // Decreasing alpha rectangles
    let alphas = [200, 150, 100, 50];
    for (i, alpha) in alphas.iter().enumerate() {
        let x = 30.0 + i as f32 * 60.0;
        let color = Color::rgba(255, 50, 50, *alpha);
        painter.rect(rect(x, y, 120.0, 80.0), &Paint::fill(color));
    }
    painter.text("Decreasing alpha", pt(60.0, y + 90.0), 12.0, &label_paint);

    // RGB overlap (additive-like with transparency)
    let overlap_x = 400.0;
    painter.circle(
        pt(overlap_x, y + 30.0),
        45.0,
        &Paint::fill(Color::rgba(255, 0, 0, 120)),
    );
    painter.circle(
        pt(overlap_x + 35.0, y + 60.0),
        45.0,
        &Paint::fill(Color::rgba(0, 255, 0, 120)),
    );
    painter.circle(
        pt(overlap_x + 70.0, y + 30.0),
        45.0,
        &Paint::fill(Color::rgba(0, 0, 255, 120)),
    );
    painter.text(
        "RGB overlap",
        pt(overlap_x + 5.0, y + 110.0),
        12.0,
        &label_paint,
    );

    // Alpha gradient bars
    for i in 0..10 {
        let x = 600.0 + i as f32 * 50.0;
        let alpha = ((i + 1) as f32 * 25.5) as u8;
        painter.rect(
            rect(x, y, 45.0, 80.0),
            &Paint::fill(Color::rgba(100, 200, 255, alpha)),
        );
    }
    painter.text("Alpha gradient", pt(750.0, y + 90.0), 12.0, &label_paint);
}

// ============================================================
// Geometry helpers
// ============================================================

fn make_star(center: Point<Pixels>, outer_r: f32, inner_r: f32, points: usize) -> Path {
    let mut pts = Vec::with_capacity(points * 2);
    for i in 0..(points * 2) {
        let angle = (i as f32) * std::f32::consts::PI / points as f32 - std::f32::consts::FRAC_PI_2;
        let r = if i % 2 == 0 { outer_r } else { inner_r };
        pts.push(pt(
            center.x.0 + angle.cos() * r,
            center.y.0 + angle.sin() * r,
        ));
    }
    Path::polygon(&pts)
}

fn make_regular_polygon(center: Point<Pixels>, radius: f32, sides: usize) -> Path {
    let mut pts = Vec::with_capacity(sides);
    for i in 0..sides {
        let angle = (i as f32) * std::f32::consts::TAU / sides as f32 - std::f32::consts::FRAC_PI_2;
        pts.push(pt(
            center.x.0 + angle.cos() * radius,
            center.y.0 + angle.sin() * radius,
        ));
    }
    Path::polygon(&pts)
}

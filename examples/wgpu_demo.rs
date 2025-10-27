//! WGPU Standalone Demo
//!
//! Demonstrates GPU-accelerated rendering using the wgpu backend.
//!
//! Run with: cargo run --example wgpu_demo --features flui_engine/wgpu

use flui_engine::{WgpuRenderer, WgpuPainter, Painter, Paint, RRect};
use flui_types::{Rect, Point, Offset, Size};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
};

fn main() {
    // Initialize logging
    env_logger::init();

    // Create event loop and window
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let window_attributes = winit::window::Window::default_attributes()
        .with_title("Flui WGPU Demo - GPU-Accelerated Rendering")
        .with_inner_size(winit::dpi::PhysicalSize::new(1200, 900));
    let window = Arc::new(
        event_loop.create_window(window_attributes)
            .expect("Failed to create window")
    );

    // Create WGPU renderer (async initialization)
    let renderer = pollster::block_on(async {
        WgpuRenderer::new(Some(window.clone()))
            .await
            .expect("Failed to create WGPU renderer")
    });

    let renderer = Arc::new(Mutex::new(renderer));
    let mut painter = WgpuPainter::new(renderer.clone());

    println!("🎨 Flui WGPU Demo Started!");
    println!("Press ESC to exit");

    // Animation state
    let mut frame = 0u32;
    let start_time = Instant::now();

    // FPS tracking
    let mut last_frame_time = Instant::now();
    let mut frame_times = Vec::with_capacity(60);
    let mut current_fps = 0.0;

    // Run event loop
    let _ = event_loop.run(move |event, target| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    println!("Window closed");
                    target.exit();
                }
                WindowEvent::Resized(size) => {
                    renderer.lock().resize(size.width, size.height);
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    // Measure frame time
                    let now = Instant::now();
                    let frame_time = now.duration_since(last_frame_time).as_secs_f32();
                    last_frame_time = now;

                    // Track frame times for smoothed FPS (rolling average)
                    frame_times.push(frame_time);
                    if frame_times.len() > 60 {
                        frame_times.remove(0);
                    }

                    // Calculate average FPS from last 60 frames
                    if !frame_times.is_empty() {
                        let avg_frame_time: f32 = frame_times.iter().sum::<f32>() / frame_times.len() as f32;
                        current_fps = if avg_frame_time > 0.0 { 1.0 / avg_frame_time } else { 0.0 };
                    }

                    frame += 1;

                    // Calculate elapsed time in seconds
                    let elapsed_time = start_time.elapsed().as_secs_f32();

                    painter.begin_frame();

                    // Draw animated shapes
                    draw_demo_shapes(&mut painter, frame, current_fps, elapsed_time);

                    // Flush to GPU
                    if let Err(e) = painter.end_frame() {
                        eprintln!("Render error: {:?}", e);
                    }

                    window.request_redraw();
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if event.state == winit::event::ElementState::Pressed {
                        if let winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape) = event.physical_key {
                            println!("ESC pressed - exiting");
                            target.exit();
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    });
}

/// Draw demo shapes with animation
fn draw_demo_shapes(painter: &mut WgpuPainter, frame: u32, fps: f32, time: f32) {
    // time is now real elapsed seconds, not frame-dependent

    // Title text with shadow effect
    painter.text_with_shadow(
        "Flui WGPU - GPU Accelerated Rendering",
        Point::new(30.0, 25.0),
        36.0,
        &Paint {
            color: [1.0, 1.0, 1.0, 1.0],
            ..Default::default()
        },
        Offset::new(3.0, 3.0),
        [0.0, 0.0, 0.0, 0.5],
    );

    // FPS counter with real-time measurement
    painter.text(
        &format!("FPS: {:.1} | Frame: {} | VSync: OFF | Press ESC to exit", fps, frame),
        Point::new(20.0, 860.0),
        14.0,
        &Paint {
            color: [0.7, 0.7, 0.7, 1.0],
            ..Default::default()
        },
    );

    // Section 1: Text Variations
    draw_text_section(painter, time);

    // Section 2: Shadows
    draw_shadow_section(painter, time);

    // Section 3: Gradients
    draw_gradient_section(painter);

    // Section 4: GPU Transforms
    draw_transform_section(painter, time);

    // Section 5: Opacity & Blending
    draw_opacity_section(painter, time);

    // Section 6: Borders & Strokes
    draw_stroke_section(painter);
}

/// Section 1: Text Variations
fn draw_text_section(painter: &mut WgpuPainter, time: f32) {
    let x = 30.0;
    let y_start = 80.0;

    painter.text(
        "1. TEXT RENDERING",
        Point::new(x, y_start),
        20.0,
        &Paint {
            color: [0.3, 0.8, 1.0, 1.0], // Cyan
            ..Default::default()
        },
    );

    // Different sizes
    let sizes = [10.0, 14.0, 18.0, 24.0, 32.0];
    for (i, &size) in sizes.iter().enumerate() {
        painter.text(
            &format!("Text at {}px", size as u32),
            Point::new(x + 20.0, y_start + 40.0 + i as f32 * 35.0),
            size,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );
    }

    // Colored text
    let colors = [
        ([1.0, 0.3, 0.3, 1.0], "Red"),
        ([0.3, 1.0, 0.3, 1.0], "Green"),
        ([0.3, 0.3, 1.0, 1.0], "Blue"),
        ([1.0, 1.0, 0.3, 1.0], "Yellow"),
        ([1.0, 0.3, 1.0, 1.0], "Magenta"),
    ];

    for (i, &(color, name)) in colors.iter().enumerate() {
        painter.text(
            name,
            Point::new(x + 250.0, y_start + 40.0 + i as f32 * 35.0),
            18.0,
            &Paint {
                color,
                ..Default::default()
            },
        );
    }

    // Animated pulsing text
    let pulse = 1.0 + (time * 3.0).sin() * 0.2;
    painter.save();
    painter.translate(Offset::new(x + 400.0, y_start + 100.0));
    painter.scale(pulse, pulse);
    painter.text(
        "Pulsing!",
        Point::ZERO,
        24.0,
        &Paint {
            color: [1.0, 0.5, 0.0, 1.0],
            ..Default::default()
        },
    );
    painter.restore();
}

/// Section 2: Shadow Effects
fn draw_shadow_section(painter: &mut WgpuPainter, time: f32) {
    let x = 30.0;
    let y_start = 290.0;

    painter.text(
        "2. SHADOW EFFECTS",
        Point::new(x, y_start),
        20.0,
        &Paint {
            color: [0.3, 0.8, 1.0, 1.0],
            ..Default::default()
        },
    );

    // Drop shadow rectangle
    painter.text_with_shadow(
        "Drop Shadow",
        Point::new(x + 20.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
        Offset::new(3.0, 3.0),
        [0.0, 0.0, 0.0, 0.5],
    );

    painter.rect_with_shadow(
        Rect::from_xywh(x + 20.0, y_start + 70.0, 120.0, 80.0),
        &Paint {
            color: [0.2, 0.5, 0.9, 1.0],
            ..Default::default()
        },
        Offset::new(4.0, 4.0),
        10.0,
        [0.0, 0.0, 0.0, 0.4],
    );

    // Smooth glow effect
    painter.text(
        "Soft Glow",
        Point::new(x + 180.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.circle_with_glow(
        Point::new(x + 240.0, y_start + 110.0),
        25.0,
        &Paint {
            color: [1.0, 0.3, 0.3, 1.0],
            ..Default::default()
        },
        40.0,
        0.8,
    );

    // Animated floating shadow
    let float_offset = (time * 2.0).sin() * 10.0;
    painter.text(
        "Floating",
        Point::new(x + 350.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.rect_with_shadow(
        Rect::from_xywh(x + 350.0, y_start + 70.0 + float_offset, 120.0, 80.0),
        &Paint {
            color: [0.9, 0.5, 0.2, 1.0],
            ..Default::default()
        },
        Offset::new(6.0, 12.0 - float_offset * 0.3),
        15.0,
        [0.0, 0.0, 0.0, 0.4],
    );
}

/// Section 3: Gradient Effects
fn draw_gradient_section(painter: &mut WgpuPainter) {
    let x = 620.0;
    let y_start = 80.0;

    painter.text(
        "3. GRADIENTS",
        Point::new(x, y_start),
        20.0,
        &Paint {
            color: [0.3, 0.8, 1.0, 1.0],
            ..Default::default()
        },
    );

    // Horizontal gradient
    painter.text(
        "Horizontal",
        Point::new(x + 20.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.horizontal_gradient(
        Rect::from_xywh(x + 20.0, y_start + 70.0, 150.0, 60.0),
        [1.0, 0.2, 0.2, 1.0],
        [0.2, 0.2, 1.0, 1.0],
    );

    // Vertical gradient
    painter.text(
        "Vertical",
        Point::new(x + 200.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.vertical_gradient(
        Rect::from_xywh(x + 200.0, y_start + 70.0, 100.0, 80.0),
        [0.2, 1.0, 0.2, 1.0],
        [1.0, 1.0, 0.2, 1.0],
    );

    // Radial gradient
    painter.text(
        "Radial",
        Point::new(x + 340.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.radial_gradient(
        Point::new(x + 400.0, y_start + 110.0),
        5.0,
        55.0,
        [1.0, 0.5, 0.0, 1.0],
        [1.0, 0.0, 0.5, 0.5],
    );
}

/// Section 4: GPU Transforms
fn draw_transform_section(painter: &mut WgpuPainter, time: f32) {
    let x = 620.0;
    let y_start = 290.0;

    painter.text(
        "4. GPU TRANSFORMS",
        Point::new(x, y_start),
        20.0,
        &Paint {
            color: [0.3, 0.8, 1.0, 1.0],
            ..Default::default()
        },
    );

    // Rotating square
    painter.text(
        "Rotate",
        Point::new(x + 40.0, y_start + 40.0),
        14.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.save();
    painter.translate(Offset::new(x + 80.0, y_start + 100.0));
    painter.rotate(time);
    painter.rect(
        Rect::from_center_size(Point::ZERO, Size::new(60.0, 60.0)),
        &Paint {
            color: [1.0, 0.3, 0.3, 1.0],
            ..Default::default()
        },
    );
    painter.restore();

    // Scaling circle
    let scale = 1.0 + (time * 2.0).sin() * 0.3;
    painter.text(
        "Scale",
        Point::new(x + 180.0, y_start + 40.0),
        14.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.save();
    painter.translate(Offset::new(x + 220.0, y_start + 100.0));
    painter.scale(scale, scale);
    painter.circle(
        Point::ZERO,
        30.0,
        &Paint {
            color: [0.3, 1.0, 0.3, 1.0],
            ..Default::default()
        },
    );
    painter.restore();

    // Combined transforms
    painter.text(
        "Combined",
        Point::new(x + 310.0, y_start + 40.0),
        14.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    painter.save();
    painter.translate(Offset::new(x + 370.0, y_start + 100.0));
    painter.rotate(time * 0.5);
    painter.scale(1.0 + (time * 3.0).cos() * 0.2, 1.0 + (time * 3.0).sin() * 0.2);
    painter.rrect(
        RRect {
            rect: Rect::from_center_size(Point::ZERO, Size::new(50.0, 50.0)),
            corner_radius: 10.0,
        },
        &Paint {
            color: [0.3, 0.3, 1.0, 1.0],
            ..Default::default()
        },
    );
    painter.restore();
}

/// Section 5: Opacity & Blending
fn draw_opacity_section(painter: &mut WgpuPainter, time: f32) {
    let x = 30.0;
    let y_start = 520.0;

    painter.text(
        "5. OPACITY & BLENDING",
        Point::new(x, y_start),
        20.0,
        &Paint {
            color: [0.3, 0.8, 1.0, 1.0],
            ..Default::default()
        },
    );

    // Opacity levels
    painter.text(
        "Opacity Levels",
        Point::new(x + 20.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    for i in 0..5 {
        let opacity = 0.2 + i as f32 * 0.2;
        painter.save();
        painter.set_opacity(opacity);
        painter.rect(
            Rect::from_xywh(x + 20.0 + i as f32 * 60.0, y_start + 70.0, 50.0, 50.0),
            &Paint {
                color: [0.5, 0.5, 1.0, 1.0],
                ..Default::default()
            },
        );
        painter.restore();

        painter.text(
            &format!("{:.0}%", opacity * 100.0),
            Point::new(x + 28.0 + i as f32 * 60.0, y_start + 130.0),
            12.0,
            &Paint {
                color: [0.7, 0.7, 0.7, 1.0],
                ..Default::default()
            },
        );
    }

    // RGB blending
    painter.text(
        "RGB Blend",
        Point::new(x + 380.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    for i in 0..3 {
        let angle = time + (i as f32) * std::f32::consts::TAU / 3.0;
        let offset_x = x + 450.0 + angle.cos() * 30.0;
        let offset_y = y_start + 95.0 + angle.sin() * 30.0;

        painter.save();
        painter.set_opacity(0.6);
        painter.circle(
            Point::new(offset_x, offset_y),
            30.0,
            &Paint {
                color: match i {
                    0 => [1.0, 0.0, 0.0, 1.0],
                    1 => [0.0, 1.0, 0.0, 1.0],
                    _ => [0.0, 0.0, 1.0, 1.0],
                },
                ..Default::default()
            },
        );
        painter.restore();
    }
}

/// Section 6: Borders & Strokes
fn draw_stroke_section(painter: &mut WgpuPainter) {
    let x = 30.0;
    let y_start = 700.0;

    painter.text(
        "6. BORDERS & STROKES",
        Point::new(x, y_start),
        20.0,
        &Paint {
            color: [0.3, 0.8, 1.0, 1.0],
            ..Default::default()
        },
    );

    // Stroke widths
    painter.text(
        "Stroke Widths",
        Point::new(x + 20.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    for i in 0..6 {
        painter.line(
            Point::new(x + 20.0, y_start + 70.0 + i as f32 * 18.0),
            Point::new(x + 300.0, y_start + 70.0 + i as f32 * 18.0),
            &Paint {
                color: [0.7, 0.7, 0.7, 1.0],
                stroke_width: 0.5 + i as f32 * 0.5,
                ..Default::default()
            },
        );

        painter.text(
            &format!("{:.1}px", 0.5 + i as f32 * 0.5),
            Point::new(x + 310.0, y_start + 65.0 + i as f32 * 18.0),
            12.0,
            &Paint {
                color: [0.7, 0.7, 0.7, 1.0],
                ..Default::default()
            },
        );
    }

    // Rounded rectangles with different radii
    painter.text(
        "Corner Radii",
        Point::new(x + 450.0, y_start + 40.0),
        16.0,
        &Paint {
            color: [0.9, 0.9, 0.9, 1.0],
            ..Default::default()
        },
    );

    for i in 0..4 {
        let radius = i as f32 * 8.0;
        painter.rrect(
            RRect {
                rect: Rect::from_xywh(x + 450.0 + i as f32 * 85.0, y_start + 70.0, 70.0, 70.0),
                corner_radius: radius,
            },
            &Paint {
                color: [0.4, 0.6, 0.8, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            &format!("{}px", radius as u32),
            Point::new(x + 468.0 + i as f32 * 85.0, y_start + 150.0),
            12.0,
            &Paint {
                color: [0.7, 0.7, 0.7, 1.0],
                ..Default::default()
            },
        );
    }
}

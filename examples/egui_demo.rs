//! Egui Standalone Demo
//!
//! Demonstrates CPU rendering using the egui backend with glam transforms.
//!
//! Run with: cargo run --example egui_demo --features flui_engine/egui

use flui_engine::{EguiPainter, Painter, Paint, RRect};
use flui_types::{Rect, Point, Offset, Size};
use std::time::Instant;

fn main() {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 900.0])
            .with_title("Flui Egui Demo - CPU Rendering with Transform Support"),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Flui Egui Demo",
        options,
        Box::new(|_cc| Ok(Box::new(DemoApp::new()))),
    );
}

struct DemoApp {
    start_time: Instant,
    frame: u32,
    frame_times: Vec<f32>,
    last_frame_time: Instant,
}

impl DemoApp {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            frame: 0,
            frame_times: Vec::with_capacity(60),
            last_frame_time: Instant::now(),
        }
    }

    fn calculate_fps(&mut self) -> f32 {
        let now = Instant::now();
        let frame_time = now.duration_since(self.last_frame_time).as_secs_f32();
        self.last_frame_time = now;

        self.frame_times.push(frame_time);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }

        if !self.frame_times.is_empty() {
            let avg = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
            if avg > 0.0 { 1.0 / avg } else { 0.0 }
        } else {
            0.0
        }
    }
}

impl eframe::App for DemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let fps = self.calculate_fps();
        let time = self.start_time.elapsed().as_secs_f32();
        self.frame += 1;

        // Request continuous repaint for smooth animation
        ctx.request_repaint();

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(25, 25, 25)))
            .show(ctx, |ui| {
                // Get painter from egui
                let painter = ui.painter();
                let mut flui_painter = EguiPainter::new(painter);

                // Draw all demo shapes
                draw_demo_shapes(&mut flui_painter, self.frame, fps, time);
            });
    }
}

/// Draw demo shapes with animation
fn draw_demo_shapes(painter: &mut EguiPainter, frame: u32, fps: f32, time: f32) {
    // Title text with shadow effect
    draw_text_with_shadow(
        painter,
        "Flui Egui - CPU Rendering with Transform Support",
        Point::new(30.0, 25.0),
        36.0,
        [1.0, 1.0, 1.0, 1.0],
    );

    // FPS counter with real-time measurement
    painter.text(
        &format!("FPS: {:.1} | Frame: {} | Backend: Egui + Glam | Press ESC to exit", fps, frame),
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

    // Section 4: CPU Transforms
    draw_transform_section(painter, time);

    // Section 5: Opacity & Blending
    draw_opacity_section(painter, time);

    // Section 6: Borders & Strokes
    draw_stroke_section(painter);
}

/// Draw text with shadow effect
fn draw_text_with_shadow(
    painter: &mut EguiPainter,
    text: &str,
    pos: Point,
    size: f32,
    color: [f32; 4],
) {
    // Shadow (offset and darker)
    painter.text(
        text,
        Point::new(pos.x + 3.0, pos.y + 3.0),
        size,
        &Paint {
            color: [0.0, 0.0, 0.0, 0.5],
            ..Default::default()
        },
    );

    // Main text
    painter.text(
        text,
        pos,
        size,
        &Paint {
            color,
            ..Default::default()
        },
    );
}

/// Section 1: Text Variations
fn draw_text_section(painter: &mut EguiPainter, time: f32) {
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
fn draw_shadow_section(painter: &mut EguiPainter, time: f32) {
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
    draw_text_with_shadow(
        painter,
        "Drop Shadow",
        Point::new(x + 20.0, y_start + 40.0),
        16.0,
        [0.9, 0.9, 0.9, 1.0],
    );

    draw_rect_with_shadow(
        painter,
        Rect::from_xywh(x + 20.0, y_start + 70.0, 120.0, 80.0),
        [0.2, 0.5, 0.9, 1.0],
        4.0,
        4.0,
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

    draw_circle_with_glow(
        painter,
        Point::new(x + 240.0, y_start + 110.0),
        25.0,
        [1.0, 0.3, 0.3, 1.0],
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

    draw_rect_with_shadow(
        painter,
        Rect::from_xywh(x + 350.0, y_start + 70.0 + float_offset, 120.0, 80.0),
        [0.9, 0.5, 0.2, 1.0],
        6.0,
        12.0 - float_offset * 0.3,
    );
}

/// Section 3: Gradient Effects
fn draw_gradient_section(painter: &mut EguiPainter) {
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

    draw_horizontal_gradient(
        painter,
        Rect::from_xywh(x + 20.0, y_start + 70.0, 150.0, 60.0),
        [1.0, 0.2, 0.2, 1.0],
        [0.2, 0.2, 1.0, 1.0],
        50,
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

    draw_vertical_gradient(
        painter,
        Rect::from_xywh(x + 200.0, y_start + 70.0, 100.0, 80.0),
        [0.2, 1.0, 0.2, 1.0],
        [1.0, 1.0, 0.2, 1.0],
        60,
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

    draw_radial_gradient(
        painter,
        Point::new(x + 400.0, y_start + 110.0),
        [1.0, 0.5, 0.0, 1.0],
        [1.0, 0.0, 0.5, 0.5],
        30,
    );
}

/// Section 4: CPU Transforms
fn draw_transform_section(painter: &mut EguiPainter, time: f32) {
    let x = 620.0;
    let y_start = 290.0;

    painter.text(
        "4. CPU TRANSFORMS",
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
fn draw_opacity_section(painter: &mut EguiPainter, time: f32) {
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
fn draw_stroke_section(painter: &mut EguiPainter) {
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

/// Helper: Draw rectangle with shadow
fn draw_rect_with_shadow(
    painter: &mut EguiPainter,
    rect: Rect,
    color: [f32; 4],
    offset_x: f32,
    offset_y: f32,
) {
    // Shadow layers - more layers for smoother shadow
    for i in 0..8 {
        let progress = i as f32 / 7.0;
        let blur = progress * 6.0;
        let opacity = 0.08 * (1.0 - progress);

        painter.save();
        painter.set_opacity(opacity);
        painter.rect(
            Rect::from_xywh(
                rect.left() + offset_x + blur * 0.5,
                rect.top() + offset_y + blur * 0.5,
                rect.width() + blur * 2.0,
                rect.height() + blur * 2.0,
            ),
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.restore();
    }

    // Main shape
    painter.rect(rect, &Paint { color, ..Default::default() });
}

/// Helper: Draw circle with glow effect
fn draw_circle_with_glow(painter: &mut EguiPainter, center: Point, radius: f32, color: [f32; 4]) {
    // Draw radial gradient from outside to inside
    for i in (0..40).rev() {
        let t = i as f32 / 39.0;
        let falloff = 1.0 - t;
        let eased = falloff * falloff * falloff;

        let current_color = [
            color[0],
            color[1],
            color[2],
            color[3] * eased * 0.8,
        ];

        let current_radius = radius + (1.0 - eased) * 40.0;

        painter.circle(center, current_radius, &Paint {
            color: current_color,
            ..Default::default()
        });
    }

    // Solid core
    painter.circle(center, radius, &Paint { color, ..Default::default() });
}

/// Helper: Draw horizontal gradient
fn draw_horizontal_gradient(
    painter: &mut EguiPainter,
    rect: Rect,
    start_color: [f32; 4],
    end_color: [f32; 4],
    steps: usize,
) {
    let step_width = rect.width() / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let color = [
            start_color[0] * (1.0 - t) + end_color[0] * t,
            start_color[1] * (1.0 - t) + end_color[1] * t,
            start_color[2] * (1.0 - t) + end_color[2] * t,
            start_color[3] * (1.0 - t) + end_color[3] * t,
        ];
        painter.rect(
            Rect::from_xywh(rect.left() + i as f32 * step_width, rect.top(), step_width + 1.0, rect.height()),
            &Paint { color, ..Default::default() },
        );
    }
}

/// Helper: Draw vertical gradient
fn draw_vertical_gradient(
    painter: &mut EguiPainter,
    rect: Rect,
    start_color: [f32; 4],
    end_color: [f32; 4],
    steps: usize,
) {
    let step_height = rect.height() / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let color = [
            start_color[0] * (1.0 - t) + end_color[0] * t,
            start_color[1] * (1.0 - t) + end_color[1] * t,
            start_color[2] * (1.0 - t) + end_color[2] * t,
            start_color[3] * (1.0 - t) + end_color[3] * t,
        ];
        painter.rect(
            Rect::from_xywh(rect.left(), rect.top() + i as f32 * step_height, rect.width(), step_height + 1.0),
            &Paint { color, ..Default::default() },
        );
    }
}

/// Helper: Draw radial gradient
fn draw_radial_gradient(
    painter: &mut EguiPainter,
    center: Point,
    inner_color: [f32; 4],
    outer_color: [f32; 4],
    steps: usize,
) {
    for i in (0..steps).rev() {
        let t = i as f32 / (steps - 1) as f32;
        let radius = 5.0 + t * 50.0;
        let color = [
            inner_color[0] * (1.0 - t) + outer_color[0] * t,
            inner_color[1] * (1.0 - t) + outer_color[1] * t,
            inner_color[2] * (1.0 - t) + outer_color[2] * t,
            inner_color[3] * (1.0 - t) + outer_color[3] * t,
        ];
        painter.circle(center, radius, &Paint { color, ..Default::default() });
    }
}

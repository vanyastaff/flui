//! Unified App API Example
//!
//! Demonstrates the new unified API for creating applications.
//! The backend (WGPU/Egui) is selected automatically based on features.
//!
//! Run with:
//! - WGPU: cargo run --example unified_app --features flui_engine/wgpu
//! - Egui: cargo run --example unified_app --features flui_engine/egui

use flui_engine::{App, AppLogic, Painter, Paint};
use flui_types::{Point, Rect, Offset, Size};
use std::f32::consts::PI;

/// Simple drawing application
struct DrawingApp {
    time: f32,
    frame: u32,
}

impl DrawingApp {
    fn new() -> Self {
        Self {
            time: 0.0,
            frame: 0,
        }
    }
}

impl AppLogic for DrawingApp {
    fn setup(&mut self) {
        println!("🎨 Flui Unified App Started!");
        println!("Press ESC to exit");
    }

    fn update(&mut self, delta_time: f32) {
        self.time += delta_time;
        self.frame += 1;
    }

    fn render(&mut self, painter: &mut dyn Painter) {
        let time = self.time;

        // Title
        painter.text_with_shadow(
            "Flui Unified App - Backend Agnostic",
            Point::new(30.0, 25.0),
            32.0,
            &Paint {
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
            Offset::new(2.0, 2.0),
            [0.0, 0.0, 0.0, 0.5],
        );

        // FPS counter
        painter.text(
            &format!("Frame: {} | Press ESC to exit", self.frame),
            Point::new(20.0, 560.0),
            14.0,
            &Paint {
                color: [0.7, 0.7, 0.7, 1.0],
                ..Default::default()
            },
        );

        // Animated rotating rectangles with shadows
        for i in 0..5 {
            let angle = time + i as f32 * (PI * 2.0 / 5.0);
            let radius = 150.0;
            let x = 400.0 + radius * angle.cos();
            let y = 300.0 + radius * angle.sin();

            painter.save();
            painter.translate(Offset::new(x, y));
            painter.rotate(time * 2.0 + i as f32);

            painter.rect_with_shadow(
                Rect::from_center_size(Point::ZERO, Size::new(60.0, 60.0)),
                &Paint {
                    color: [
                        0.3 + (i as f32 / 5.0) * 0.7,
                        0.5,
                        1.0 - (i as f32 / 5.0) * 0.7,
                        1.0,
                    ],
                    ..Default::default()
                },
                Offset::new(5.0, 5.0),
                10.0,
                [0.0, 0.0, 0.0, 0.4],
            );

            painter.restore();
        }

        // Glowing circles
        let glow_colors = [
            [1.0, 0.2, 0.2, 1.0], // Red
            [0.2, 1.0, 0.2, 1.0], // Green
            [0.2, 0.2, 1.0, 1.0], // Blue
        ];

        for (i, color) in glow_colors.iter().enumerate() {
            let offset = (time * 2.0 + i as f32 * (PI * 2.0 / 3.0)).sin() * 20.0;
            painter.circle_with_glow(
                Point::new(150.0 + i as f32 * 150.0, 200.0 + offset),
                20.0,
                &Paint {
                    color: *color,
                    ..Default::default()
                },
                30.0,
                0.8,
            );
        }

        // Gradients showcase
        let y_start = 350.0;

        // Horizontal gradient
        painter.horizontal_gradient(
            Rect::from_xywh(50.0, y_start, 200.0, 60.0),
            [1.0, 0.3, 0.3, 1.0],
            [0.3, 0.3, 1.0, 1.0],
        );

        painter.text(
            "Horizontal",
            Point::new(60.0, y_start + 70.0),
            12.0,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );

        // Vertical gradient
        painter.vertical_gradient(
            Rect::from_xywh(280.0, y_start, 100.0, 80.0),
            [0.3, 1.0, 0.3, 1.0],
            [1.0, 1.0, 0.3, 1.0],
        );

        painter.text(
            "Vertical",
            Point::new(290.0, y_start + 90.0),
            12.0,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );

        // Radial gradient
        painter.radial_gradient(
            Point::new(480.0, y_start + 40.0),
            5.0,
            45.0,
            [1.0, 0.5, 0.0, 1.0],
            [1.0, 0.0, 0.5, 0.3],
        );

        painter.text(
            "Radial",
            Point::new(450.0, y_start + 90.0),
            12.0,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );

        // New shapes: ellipse, arc, polygon
        let shapes_y = 470.0;

        // Ellipse
        let pulse = 1.0 + (time * 3.0).sin() * 0.3;
        painter.ellipse(
            Point::new(100.0, shapes_y),
            30.0 * pulse,
            50.0,
            &Paint {
                color: [1.0, 0.7, 0.2, 1.0],
                stroke_width: 2.0,
                ..Default::default()
            },
        );

        painter.text(
            "Ellipse",
            Point::new(70.0, shapes_y + 70.0),
            12.0,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );

        // Arc
        let arc_angle = time.rem_euclid(PI * 2.0);
        painter.arc(
            Point::new(250.0, shapes_y),
            40.0,
            0.0,
            arc_angle,
            &Paint {
                color: [0.2, 1.0, 0.7, 1.0],
                stroke_width: 3.0,
                ..Default::default()
            },
        );

        painter.text(
            "Arc",
            Point::new(230.0, shapes_y + 70.0),
            12.0,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );

        // Polygon (triangle)
        let triangle = [
            Point::new(400.0, shapes_y - 30.0),
            Point::new(370.0, shapes_y + 30.0),
            Point::new(430.0, shapes_y + 30.0),
        ];

        painter.polygon(
            &triangle,
            &Paint {
                color: [1.0, 0.3, 1.0, 1.0],
                stroke_width: 2.0,
                ..Default::default()
            },
        );

        painter.text(
            "Polygon",
            Point::new(375.0, shapes_y + 70.0),
            12.0,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );

        // Polyline (wave)
        let mut wave_points = Vec::new();
        for i in 0..20 {
            let t = i as f32 / 19.0;
            let x = 530.0 + t * 150.0;
            let y = shapes_y + (time * 3.0 + t * PI * 4.0).sin() * 20.0;
            wave_points.push(Point::new(x, y));
        }

        painter.polyline(
            &wave_points,
            &Paint {
                color: [0.3, 0.8, 1.0, 1.0],
                stroke_width: 2.0,
                ..Default::default()
            },
        );

        painter.text(
            "Polyline",
            Point::new(580.0, shapes_y + 70.0),
            12.0,
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                ..Default::default()
            },
        );
    }
}

fn main() -> Result<(), String> {
    // Create app with unified API
    App::new()
        .title("Flui Unified App Example")
        .size(800, 600)
        .vsync(false)  // Disable VSync for maximum FPS
        .msaa(true)    // Enable anti-aliasing
        .run(DrawingApp::new())
}

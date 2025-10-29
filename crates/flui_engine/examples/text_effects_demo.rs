//! Comprehensive Text Effects Demo
//!
//! Demonstrates various text transformation effects using flui_types::text_path helpers:
//! - Wave text
//! - Circle/Arc text
//! - Spiral text
//! - Combined effects

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint};
use flui_types::{arc_position, spiral_position, wave_offset, wave_rotation, Event, Offset, Point};
use std::f32::consts::{PI, TAU};

struct TextEffectsApp;

impl AppLogic for TextEffectsApp {
    fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Window(window_event) => {
                if let flui_types::WindowEvent::CloseRequested = window_event {
                    return false;
                }
            }
            _ => {}
        }
        true
    }

    fn update(&mut self, _delta_time: f32) {}

    fn render(&mut self, painter: &mut dyn Painter) {
        // Background
        painter.rect(
            flui_types::Rect::from_xywh(0.0, 0.0, 1200.0, 800.0),
            &Paint {
                color: [0.08, 0.08, 0.12, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            }
        );

        // Title
        painter.text(
            "TEXT TRANSFORMATION EFFECTS",
            Point::new(350.0, 30.0),
            28.0,
            &Paint { color: [1.0, 1.0, 1.0, 1.0], ..Default::default() }
        );

        painter.text(
            "Using flui_types::text_path helper functions",
            Point::new(380.0, 70.0),
            16.0,
            &Paint { color: [0.7, 0.7, 0.7, 1.0], ..Default::default() }
        );

        // Effect 1: Wave Text
        draw_label(painter, "WAVE TEXT:", Point::new(50.0, 120.0));
        draw_wave_text(
            painter,
            "WAVE EFFECT",
            Point::new(100.0, 160.0),
            32.0,
            [0.4, 0.8, 1.0, 1.0],
        );

        // Effect 2: Circle Text
        draw_label(painter, "CIRCLE TEXT:", Point::new(50.0, 280.0));
        draw_circle_text(
            painter,
            "CIRCULAR PATH TEXT",
            Point::new(250.0, 420.0),
            120.0,
            28.0,
            [1.0, 0.6, 0.4, 1.0],
        );

        // Effect 3: Spiral Text
        draw_label(painter, "SPIRAL TEXT:", Point::new(650.0, 120.0));
        draw_spiral_text(
            painter,
            "SPIRAL PATH TEXT",
            Point::new(900.0, 280.0),
            32.0,
            [0.8, 0.4, 1.0, 1.0],
        );

        // Effect 4: Wave with rotation
        draw_label(painter, "WAVE + ROTATION:", Point::new(650.0, 420.0));
        draw_wave_rotation_text(
            painter,
            "WAVE ROTATE",
            Point::new(700.0, 460.0),
            32.0,
            [0.4, 1.0, 0.6, 1.0],
        );

        // Bottom info
        painter.text(
            "All effects use only low-level primitives: Matrix4, text_path helpers, and painter.save/restore",
            Point::new(200.0, 750.0),
            14.0,
            &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
        );
    }
}

fn draw_label(painter: &mut dyn Painter, text: &str, position: Point) {
    painter.text(
        text,
        position,
        16.0,
        &Paint { color: [0.6, 0.6, 0.6, 1.0], ..Default::default() }
    );
}

/// Wave text effect using wave_offset helper
fn draw_wave_text(
    painter: &mut dyn Painter,
    text: &str,
    base_position: Point,
    font_size: f32,
    color: [f32; 4],
) {
    let char_width = font_size * 0.6;

    for (i, ch) in text.chars().enumerate() {
        let wave_y = wave_offset(i, 0.5, 15.0);

        painter.text(
            &ch.to_string(),
            Point::new(
                base_position.x + i as f32 * char_width,
                base_position.y + wave_y,
            ),
            font_size,
            &Paint { color, ..Default::default() }
        );
    }
}

/// Circle text effect using arc_position helper
fn draw_circle_text(
    painter: &mut dyn Painter,
    text: &str,
    center: Point,
    radius: f32,
    font_size: f32,
    color: [f32; 4],
) {
    for (i, ch) in text.chars().enumerate() {
        let transform = arc_position(i, text.len(), radius, -PI / 2.0, TAU);

        painter.save();
        painter.translate(Offset::new(center.x + transform.position.x, center.y + transform.position.y));
        painter.rotate(transform.rotation);
        painter.text(
            &ch.to_string(),
            Point::new(-font_size * 0.3, 0.0), // Center the character
            font_size,
            &Paint { color, ..Default::default() }
        );
        painter.restore();
    }
}

/// Spiral text effect using spiral_position helper
fn draw_spiral_text(
    painter: &mut dyn Painter,
    text: &str,
    center: Point,
    font_size: f32,
    color: [f32; 4],
) {
    for (i, ch) in text.chars().enumerate() {
        let transform = spiral_position(i, text.len(), 30.0, 80.0, 2.5);

        painter.save();
        painter.translate(Offset::new(center.x + transform.position.x, center.y + transform.position.y));
        painter.rotate(transform.rotation);
        painter.text(
            &ch.to_string(),
            Point::new(-font_size * 0.3, 0.0),
            font_size,
            &Paint { color, ..Default::default() }
        );
        painter.restore();
    }
}

/// Wave text with rotation using wave_offset and wave_rotation helpers
fn draw_wave_rotation_text(
    painter: &mut dyn Painter,
    text: &str,
    base_position: Point,
    font_size: f32,
    color: [f32; 4],
) {
    let char_width = font_size * 0.6;

    for (i, ch) in text.chars().enumerate() {
        let wave_y = wave_offset(i, 0.4, 12.0);
        let rotation = wave_rotation(i, 0.3, 0.3);

        painter.save();
        painter.translate(Offset::new(
            base_position.x + i as f32 * char_width,
            base_position.y + wave_y,
        ));
        painter.rotate(rotation);
        painter.text(
            &ch.to_string(),
            Point::new(-font_size * 0.3, 0.0),
            font_size,
            &Paint { color, ..Default::default() }
        );
        painter.restore();
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Text Effects Demo ===");
    println!("Demonstrates various text transformation effects:");
    println!("  - Wave text (wave_offset)");
    println!("  - Circle text (arc_position)");
    println!("  - Spiral text (spiral_position)");
    println!("  - Combined effects");
    println!();
    println!("All using flui_types::text_path helpers!");
    println!();

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Text Effects Demo")
        .size(1200, 800);

    app.run(TextEffectsApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

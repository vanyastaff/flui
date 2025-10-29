//! Simple Transform Demo - One word, clear transformations
//!
//! Very simple demo to understand text transformations

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint};
use flui_types::{Event, Point, Offset};

struct SimpleTransformApp;

impl AppLogic for SimpleTransformApp {
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
            flui_types::Rect::from_xywh(0.0, 0.0, 800.0, 600.0),
            &Paint {
                color: [0.08, 0.08, 0.12, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            }
        );

        // Title
        painter.text(
            "SIMPLE TRANSFORM TEST",
            Point::new(200.0, 30.0),
            24.0,
            &Paint { color: [1.0, 1.0, 1.0, 1.0], ..Default::default() }
        );

        // Normal text (no transform)
        painter.text(
            "Normal:",
            Point::new(50.0, 100.0),
            16.0,
            &Paint { color: [0.6, 0.6, 0.6, 1.0], ..Default::default() }
        );

        painter.text(
            "HELLO",
            Point::new(200.0, 100.0),
            40.0,
            &Paint { color: [1.0, 1.0, 1.0, 1.0], ..Default::default() }
        );

        // Scaled 2x
        painter.text(
            "Scale 2x:",
            Point::new(50.0, 200.0),
            16.0,
            &Paint { color: [0.6, 0.6, 0.6, 1.0], ..Default::default() }
        );

        painter.save();
        painter.translate(Offset::new(200.0, 200.0));
        painter.scale(2.0, 2.0);
        painter.text(
            "HELLO",
            Point::ZERO,
            40.0,
            &Paint { color: [0.4, 0.8, 1.0, 1.0], ..Default::default() }
        );
        painter.restore();

        // Rotated
        painter.text(
            "Rotate 45°:",
            Point::new(50.0, 350.0),
            16.0,
            &Paint { color: [0.6, 0.6, 0.6, 1.0], ..Default::default() }
        );

        painter.save();
        painter.translate(Offset::new(250.0, 400.0));
        painter.rotate(0.785); // 45 degrees
        painter.text(
            "HELLO",
            Point::ZERO,
            40.0,
            &Paint { color: [1.0, 0.6, 0.4, 1.0], ..Default::default() }
        );
        painter.restore();

        // Info
        painter.text(
            "Close window to exit",
            Point::new(300.0, 570.0),
            14.0,
            &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Simple Transform Demo ===");
    println!("Shows 3 examples:");
    println!("  1. Normal text (no transform)");
    println!("  2. Scaled 2x");
    println!("  3. Rotated 45 degrees");
    println!();

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Simple Transform Demo")
        .size(800, 600);

    app.run(SimpleTransformApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

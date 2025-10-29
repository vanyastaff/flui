//! Test example to demonstrate gradient rendering with Painter primitives

use flui_engine::{App, AppConfig, AppLogic, Paint, Painter};
use flui_types::{Event, Point, Rect};

struct GradientTestApp;

impl AppLogic for GradientTestApp {
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
            Rect::from_xywh(0.0, 0.0, 800.0, 600.0),
            &Paint {
                color: [0.95, 0.95, 0.95, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // Title
        painter.text(
            "Gradient Test - Painter Primitives",
            Point::new(200.0, 30.0),
            20.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // 1. Horizontal Gradient (red to blue)
        let rect1 = Rect::from_xywh(50.0, 80.0, 300.0, 100.0);
        painter.horizontal_gradient(
            rect1,
            [1.0, 0.0, 0.0, 1.0], // red
            [0.0, 0.0, 1.0, 1.0], // blue
        );
        painter.text(
            "Horizontal: Red -> Blue",
            Point::new(60.0, 200.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // 2. Vertical Gradient (green to yellow)
        let rect2 = Rect::from_xywh(400.0, 80.0, 300.0, 100.0);
        painter.vertical_gradient(
            rect2,
            [0.0, 1.0, 0.0, 1.0], // green
            [1.0, 1.0, 0.0, 1.0], // yellow
        );
        painter.text(
            "Vertical: Green -> Yellow",
            Point::new(410.0, 200.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // 3. Radial Gradient (blue center to red edge)
        let center = Point::new(200.0, 350.0);
        painter.radial_gradient_simple(
            center,
            0.0,                  // inner radius
            100.0,                // outer radius
            [0.0, 0.0, 1.0, 1.0], // blue center
            [1.0, 0.0, 0.0, 1.0], // red edge
        );
        painter.text(
            "Radial: Blue -> Red",
            Point::new(140.0, 470.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // 4. Another radial (cyan to magenta)
        let center2 = Point::new(550.0, 350.0);
        painter.radial_gradient_simple(
            center2,
            20.0,                 // inner radius (creates a donut effect)
            100.0,                // outer radius
            [0.0, 1.0, 1.0, 1.0], // cyan center
            [1.0, 0.0, 1.0, 1.0], // magenta edge
        );
        painter.text(
            "Radial: Cyan -> Magenta",
            Point::new(470.0, 470.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Gradient Test Demo ===");
    println!("Testing Painter gradient primitives:");
    println!("  1. horizontal_gradient() - red to blue");
    println!("  2. vertical_gradient() - green to yellow");
    println!("  3. radial_gradient_simple() - blue center to red edge");
    println!("  4. radial_gradient_simple() - cyan to magenta (with inner radius)");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Gradient Test")
        .size(800, 600);

    app.run(GradientTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

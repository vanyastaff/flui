//! Systematic test for basic shapes

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint, RRect};
use flui_types::{Event, Rect, Point};

struct ShapesTestApp;

impl AppLogic for ShapesTestApp {
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
            }
        );

        // Title
        painter.text(
            "Shapes Test - Systematic",
            Point::new(270.0, 30.0),
            20.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        // 1. Rectangle (filled)
        painter.text("Rectangle (filled)", Point::new(50.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.rect(
            Rect::from_xywh(50.0, 90.0, 120.0, 80.0),
            &Paint { color: [0.2, 0.5, 0.8, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // 2. Rectangle (stroked)
        painter.text("Rectangle (stroked)", Point::new(220.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.rect(
            Rect::from_xywh(220.0, 90.0, 120.0, 80.0),
            &Paint { color: [0.8, 0.2, 0.2, 1.0], stroke_width: 3.0, anti_alias: true }
        );

        // 3. Rounded Rectangle
        painter.text("Rounded Rectangle", Point::new(390.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.rrect(
            RRect {
                rect: Rect::from_xywh(390.0, 90.0, 120.0, 80.0),
                corner_radius: 15.0,
            },
            &Paint { color: [0.2, 0.7, 0.3, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // 4. Circle (filled)
        painter.text("Circle (filled)", Point::new(580.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.circle(
            Point::new(640.0, 130.0),
            40.0,
            &Paint { color: [0.7, 0.2, 0.7, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // 5. Circle (stroked)
        painter.text("Circle (stroked)", Point::new(50.0, 210.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.circle(
            Point::new(110.0, 270.0),
            40.0,
            &Paint { color: [0.2, 0.6, 0.8, 1.0], stroke_width: 3.0, anti_alias: true }
        );

        // 6. Line
        painter.text("Line", Point::new(220.0, 210.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.line(
            Point::new(220.0, 240.0),
            Point::new(340.0, 290.0),
            &Paint { color: [0.8, 0.4, 0.0, 1.0], stroke_width: 4.0, anti_alias: true }
        );

        // 7. Arc
        painter.text("Arc (90°)", Point::new(390.0, 210.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.arc(
            Point::new(450.0, 270.0),
            40.0,
            0.0,
            std::f32::consts::PI / 2.0,
            &Paint { color: [0.0, 0.6, 0.6, 1.0], stroke_width: 3.0, anti_alias: true }
        );

        // 8. Polygon (triangle)
        painter.text("Polygon (triangle)", Point::new(560.0, 210.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let triangle = vec![
            Point::new(640.0, 240.0),
            Point::new(600.0, 300.0),
            Point::new(680.0, 300.0),
        ];
        painter.polygon(
            &triangle,
            &Paint { color: [0.8, 0.6, 0.2, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // 9. Polygon (pentagon)
        painter.text("Polygon (pentagon)", Point::new(50.0, 350.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let pentagon = vec![
            Point::new(130.0, 380.0),
            Point::new(170.0, 395.0),
            Point::new(160.0, 440.0),
            Point::new(100.0, 440.0),
            Point::new(90.0, 395.0),
        ];
        painter.polygon(
            &pentagon,
            &Paint { color: [0.3, 0.3, 0.7, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // 10. Multiple lines (star pattern)
        painter.text("Star pattern (lines)", Point::new(250.0, 350.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let center = Point::new(320.0, 420.0);
        let radius = 50.0;
        for i in 0..5 {
            let angle1 = (i as f32 * 2.0 * std::f32::consts::PI / 5.0) - std::f32::consts::PI / 2.0;
            let angle2 = ((i + 2) as f32 * 2.0 * std::f32::consts::PI / 5.0) - std::f32::consts::PI / 2.0;
            let p1 = Point::new(
                center.x + radius * angle1.cos(),
                center.y + radius * angle1.sin(),
            );
            let p2 = Point::new(
                center.x + radius * angle2.cos(),
                center.y + radius * angle2.sin(),
            );
            painter.line(
                p1, p2,
                &Paint { color: [0.8, 0.2, 0.4, 1.0], stroke_width: 2.0, anti_alias: true }
            );
        }

        // 11. Oval (using rrect with different radii)
        painter.text("Oval", Point::new(450.0, 350.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.rrect(
            RRect {
                rect: Rect::from_xywh(450.0, 380.0, 100.0, 60.0),
                corner_radius: 30.0,
            },
            &Paint { color: [0.5, 0.3, 0.7, 1.0], stroke_width: 0.0, anti_alias: true }
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Shapes Test ===");
    println!("Systematic testing of basic shapes:");
    println!("  1. Rectangle (filled)");
    println!("  2. Rectangle (stroked)");
    println!("  3. Rounded Rectangle");
    println!("  4. Circle (filled)");
    println!("  5. Circle (stroked)");
    println!("  6. Line");
    println!("  7. Arc");
    println!("  8. Polygon (triangle)");
    println!("  9. Polygon (pentagon)");
    println!("  10. Star pattern (lines)");
    println!("  11. Oval");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Shapes Test")
        .size(800, 600);

    app.run(ShapesTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

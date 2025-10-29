//! Systematic test for clipping operations

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint, RRect};
use flui_types::{Event, Rect, Point};

struct ClippingTestApp;

impl AppLogic for ClippingTestApp {
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
            "Clipping Test - Systematic",
            Point::new(260.0, 30.0),
            20.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        // 1. No clipping (baseline)
        // Expected: Full circle + cross visible
        painter.text("No Clipping", Point::new(50.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_test_pattern(painter, Point::new(110.0, 120.0));

        // 2. Clip Rect
        // Expected: Circle and cross clipped to rectangular boundary (red outline)
        painter.text("Clip Rect", Point::new(250.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.clip_rect(Rect::from_xywh(280.0, 90.0, 80.0, 80.0));
        self.draw_test_pattern(painter, Point::new(310.0, 120.0));
        painter.restore();
        // Draw clip boundary
        painter.rect(
            Rect::from_xywh(280.0, 90.0, 80.0, 80.0),
            &Paint { color: [1.0, 0.0, 0.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );

        // 3. Clip Rounded Rect
        // Expected: Circle and cross clipped to rounded rectangle with 20px corner radius
        painter.text("Clip RRect", Point::new(450.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.clip_rrect(RRect {
            rect: Rect::from_xywh(480.0, 90.0, 80.0, 80.0),
            corner_radius: 20.0,
        });
        self.draw_test_pattern(painter, Point::new(510.0, 120.0));
        painter.restore();
        // Draw clip boundary
        painter.rrect(
            RRect {
                rect: Rect::from_xywh(480.0, 90.0, 80.0, 80.0),
                corner_radius: 20.0,
            },
            &Paint { color: [1.0, 0.0, 0.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );

        // 4. Clip Oval
        // Expected: Circle and cross clipped to circular/oval boundary
        painter.text("Clip Oval", Point::new(650.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        let oval_rect = Rect::from_xywh(670.0, 90.0, 80.0, 80.0);
        painter.clip_oval(oval_rect);
        self.draw_test_pattern(painter, Point::new(710.0, 130.0));
        painter.restore();
        // Draw clip boundary (center: 670+40=710, 90+40=130)
        painter.circle(
            Point::new(710.0, 130.0),
            40.0,
            &Paint { color: [1.0, 0.0, 0.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );

        // 5. Nested Clipping (rect inside rect)
        // Expected: Pattern clipped to intersection of two rects (blue inner boundary)
        painter.text("Nested Clip", Point::new(50.0, 220.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.clip_rect(Rect::from_xywh(80.0, 240.0, 100.0, 100.0));
        painter.save();
        painter.clip_rect(Rect::from_xywh(100.0, 260.0, 60.0, 60.0));
        self.draw_test_pattern(painter, Point::new(130.0, 280.0));
        painter.restore();
        painter.restore();
        // Draw clip boundaries
        painter.rect(
            Rect::from_xywh(80.0, 240.0, 100.0, 100.0),
            &Paint { color: [1.0, 0.0, 0.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );
        painter.rect(
            Rect::from_xywh(100.0, 260.0, 60.0, 60.0),
            &Paint { color: [0.0, 0.0, 1.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );

        // 6. Clip with Transform
        // Expected: Pattern clipped to rotated rectangle (30 degrees)
        painter.text("Clip + Rotate", Point::new(250.0, 220.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(flui_types::Offset::new(320.0, 290.0));
        painter.rotate(std::f32::consts::PI / 6.0);
        painter.clip_rect(Rect::from_xywh(-40.0, -40.0, 80.0, 80.0));
        self.draw_test_pattern(painter, Point::new(-10.0, -10.0));
        painter.restore();
        // Draw clip boundary (rotated rect)
        painter.save();
        painter.translate(flui_types::Offset::new(320.0, 290.0));
        painter.rotate(std::f32::consts::PI / 6.0);
        painter.rect(
            Rect::from_xywh(-40.0, -40.0, 80.0, 80.0),
            &Paint { color: [1.0, 0.0, 0.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );
        painter.restore();

        // 7. Multiple Overlapping Clips
        // Expected: Pattern clipped to intersection of rect and oval (red rect + blue circle)
        painter.text("Overlapping Clips", Point::new(450.0, 220.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.clip_rect(Rect::from_xywh(480.0, 240.0, 100.0, 100.0));
        painter.save();
        painter.clip_oval(Rect::from_xywh(500.0, 260.0, 80.0, 80.0));
        self.draw_test_pattern(painter, Point::new(540.0, 300.0));
        painter.restore();
        painter.restore();
        // Draw clip boundaries
        painter.rect(
            Rect::from_xywh(480.0, 240.0, 100.0, 100.0),
            &Paint { color: [1.0, 0.0, 0.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );
        painter.circle(
            Point::new(540.0, 300.0),
            40.0,
            &Paint { color: [0.0, 0.0, 1.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );

        // 8. Clip with Gradient
        // Expected: Red-to-blue gradient clipped to rounded rectangle
        painter.text("Clip + Gradient", Point::new(50.0, 390.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.clip_rrect(RRect {
            rect: Rect::from_xywh(80.0, 410.0, 120.0, 120.0),
            corner_radius: 30.0,
        });
        painter.horizontal_gradient(
            Rect::from_xywh(50.0, 380.0, 180.0, 180.0),
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
        );
        painter.restore();
        // Draw clip boundary
        painter.rrect(
            RRect {
                rect: Rect::from_xywh(80.0, 410.0, 120.0, 120.0),
                corner_radius: 30.0,
            },
            &Paint { color: [0.0, 0.0, 0.0, 1.0], stroke_width: 2.0, anti_alias: true }
        );
    }
}

impl ClippingTestApp {
    /// Helper: Draw a test pattern (circle + cross)
    fn draw_test_pattern(&self, painter: &mut dyn Painter, center: Point) {
        // Blue circle
        painter.circle(
            center,
            30.0,
            &Paint { color: [0.2, 0.5, 0.8, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // Red cross
        painter.line(
            Point::new(center.x - 40.0, center.y),
            Point::new(center.x + 40.0, center.y),
            &Paint { color: [0.8, 0.2, 0.2, 1.0], stroke_width: 3.0, anti_alias: true }
        );
        painter.line(
            Point::new(center.x, center.y - 40.0),
            Point::new(center.x, center.y + 40.0),
            &Paint { color: [0.8, 0.2, 0.2, 1.0], stroke_width: 3.0, anti_alias: true }
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Clipping Test ===");
    println!("Systematic testing of clipping:");
    println!("  1. No Clipping (baseline)");
    println!("  2. Clip Rect");
    println!("  3. Clip Rounded Rect");
    println!("  4. Clip Oval");
    println!("  5. Nested Clipping");
    println!("  6. Clip with Transform");
    println!("  7. Overlapping Clips");
    println!("  8. Clip with Gradient");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Clipping Test")
        .size(800, 600);

    app.run(ClippingTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

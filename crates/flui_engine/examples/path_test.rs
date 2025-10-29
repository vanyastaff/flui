//! Systematic test for path and polygon rendering

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint};
use flui_types::{Event, Rect, Point};

struct PathTestApp;

impl PathTestApp {
    fn draw_arrow(&self, painter: &mut dyn Painter, start: Point, end: Point, color: [f32; 4]) {
        // Arrow shaft
        painter.line(start, end, &Paint { color, stroke_width: 2.0, anti_alias: true });

        // Arrow head
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let angle = dy.atan2(dx);
        let arrow_size = 10.0;

        let p1 = Point::new(
            end.x - arrow_size * (angle - 0.5).cos(),
            end.y - arrow_size * (angle - 0.5).sin(),
        );
        let p2 = Point::new(
            end.x - arrow_size * (angle + 0.5).cos(),
            end.y - arrow_size * (angle + 0.5).sin(),
        );

        painter.polygon(&[end, p1, p2], &Paint { color, stroke_width: 0.0, anti_alias: true });
    }

    fn draw_star(&self, painter: &mut dyn Painter, center: Point, outer_radius: f32, inner_radius: f32, points: usize, color: [f32; 4]) {
        let mut vertices = Vec::new();

        for i in 0..(points * 2) {
            let angle = (i as f32 * std::f32::consts::PI / points as f32) - std::f32::consts::PI / 2.0;
            let radius = if i % 2 == 0 { outer_radius } else { inner_radius };

            vertices.push(Point::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            ));
        }

        painter.polygon(&vertices, &Paint { color, stroke_width: 0.0, anti_alias: true });
    }

    fn draw_heart(&self, painter: &mut dyn Painter, center: Point, size: f32, color: [f32; 4]) {
        // Simplified heart shape using circles and triangle
        let offset = size * 0.25;

        // Left circle
        painter.circle(
            Point::new(center.x - offset, center.y - offset * 0.5),
            size * 0.3,
            &Paint { color, stroke_width: 0.0, anti_alias: true }
        );

        // Right circle
        painter.circle(
            Point::new(center.x + offset, center.y - offset * 0.5),
            size * 0.3,
            &Paint { color, stroke_width: 0.0, anti_alias: true }
        );

        // Bottom triangle
        let triangle = vec![
            Point::new(center.x - size * 0.45, center.y - offset * 0.3),
            Point::new(center.x + size * 0.45, center.y - offset * 0.3),
            Point::new(center.x, center.y + size * 0.6),
        ];
        painter.polygon(&triangle, &Paint { color, stroke_width: 0.0, anti_alias: true });
    }
}

impl AppLogic for PathTestApp {
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
            "Path & Polygon Test - Systematic",
            Point::new(220.0, 30.0),
            20.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        // 1. Triangle (3 sides)
        painter.text("1. Triangle", Point::new(50.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let triangle = vec![
            Point::new(100.0, 90.0),
            Point::new(70.0, 130.0),
            Point::new(130.0, 130.0),
        ];
        painter.polygon(&triangle, &Paint { color: [0.3, 0.6, 0.9, 1.0], stroke_width: 0.0, anti_alias: true });

        // 2. Square (using polygon)
        painter.text("2. Square", Point::new(180.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let square = vec![
            Point::new(180.0, 90.0),
            Point::new(230.0, 90.0),
            Point::new(230.0, 140.0),
            Point::new(180.0, 140.0),
        ];
        painter.polygon(&square, &Paint { color: [0.9, 0.4, 0.3, 1.0], stroke_width: 0.0, anti_alias: true });

        // 3. Pentagon (5 sides)
        painter.text("3. Pentagon", Point::new(280.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let mut pentagon = Vec::new();
        for i in 0..5 {
            let angle = (i as f32 * 2.0 * std::f32::consts::PI / 5.0) - std::f32::consts::PI / 2.0;
            pentagon.push(Point::new(
                330.0 + 25.0 * angle.cos(),
                115.0 + 25.0 * angle.sin(),
            ));
        }
        painter.polygon(&pentagon, &Paint { color: [0.3, 0.8, 0.5, 1.0], stroke_width: 0.0, anti_alias: true });

        // 4. Hexagon (6 sides)
        painter.text("4. Hexagon", Point::new(400.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let mut hexagon = Vec::new();
        for i in 0..6 {
            let angle = (i as f32 * 2.0 * std::f32::consts::PI / 6.0) - std::f32::consts::PI / 2.0;
            hexagon.push(Point::new(
                450.0 + 25.0 * angle.cos(),
                115.0 + 25.0 * angle.sin(),
            ));
        }
        painter.polygon(&hexagon, &Paint { color: [0.7, 0.3, 0.8, 1.0], stroke_width: 0.0, anti_alias: true });

        // 5. Octagon (8 sides)
        painter.text("5. Octagon", Point::new(520.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let mut octagon = Vec::new();
        for i in 0..8 {
            let angle = (i as f32 * 2.0 * std::f32::consts::PI / 8.0) - std::f32::consts::PI / 2.0;
            octagon.push(Point::new(
                570.0 + 25.0 * angle.cos(),
                115.0 + 25.0 * angle.sin(),
            ));
        }
        painter.polygon(&octagon, &Paint { color: [0.9, 0.7, 0.2, 1.0], stroke_width: 0.0, anti_alias: true });

        // 6. Star (5-point)
        painter.text("6. Star (5pt)", Point::new(50.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_star(painter, Point::new(100.0, 205.0), 30.0, 12.0, 5, [0.9, 0.8, 0.2, 1.0]);

        // 7. Star (6-point)
        painter.text("7. Star (6pt)", Point::new(180.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_star(painter, Point::new(230.0, 205.0), 30.0, 15.0, 6, [0.3, 0.7, 0.9, 1.0]);

        // 8. Arrow
        painter.text("8. Arrow", Point::new(280.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_arrow(painter, Point::new(300.0, 210.0), Point::new(360.0, 190.0), [0.9, 0.3, 0.3, 1.0]);

        // 9. Heart
        painter.text("9. Heart", Point::new(400.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_heart(painter, Point::new(450.0, 200.0), 25.0, [0.9, 0.2, 0.4, 1.0]);

        // 10. Diamond
        painter.text("10. Diamond", Point::new(520.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let diamond = vec![
            Point::new(570.0, 180.0),
            Point::new(595.0, 205.0),
            Point::new(570.0, 230.0),
            Point::new(545.0, 205.0),
        ];
        painter.polygon(&diamond, &Paint { color: [0.2, 0.8, 0.9, 1.0], stroke_width: 0.0, anti_alias: true });

        // 11. Chevron (V shape)
        painter.text("11. Chevron", Point::new(50.0, 250.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let chevron = vec![
            Point::new(70.0, 270.0),
            Point::new(100.0, 300.0),
            Point::new(130.0, 270.0),
            Point::new(120.0, 270.0),
            Point::new(100.0, 285.0),
            Point::new(80.0, 270.0),
        ];
        painter.polygon(&chevron, &Paint { color: [0.5, 0.3, 0.7, 1.0], stroke_width: 0.0, anti_alias: true });

        // 12. Plus Sign
        painter.text("12. Plus", Point::new(180.0, 250.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let plus = vec![
            Point::new(215.0, 270.0),
            Point::new(225.0, 270.0),
            Point::new(225.0, 280.0),
            Point::new(235.0, 280.0),
            Point::new(235.0, 290.0),
            Point::new(225.0, 290.0),
            Point::new(225.0, 300.0),
            Point::new(215.0, 300.0),
            Point::new(215.0, 290.0),
            Point::new(205.0, 290.0),
            Point::new(205.0, 280.0),
            Point::new(215.0, 280.0),
        ];
        painter.polygon(&plus, &Paint { color: [0.3, 0.9, 0.4, 1.0], stroke_width: 0.0, anti_alias: true });

        // 13. Trapezoid
        painter.text("13. Trapezoid", Point::new(280.0, 250.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let trapezoid = vec![
            Point::new(300.0, 270.0),
            Point::new(360.0, 270.0),
            Point::new(370.0, 300.0),
            Point::new(290.0, 300.0),
        ];
        painter.polygon(&trapezoid, &Paint { color: [0.9, 0.5, 0.2, 1.0], stroke_width: 0.0, anti_alias: true });

        // 14. Parallelogram
        painter.text("14. Parallelogram", Point::new(400.0, 250.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let parallelogram = vec![
            Point::new(420.0, 270.0),
            Point::new(480.0, 270.0),
            Point::new(470.0, 300.0),
            Point::new(410.0, 300.0),
        ];
        painter.polygon(&parallelogram, &Paint { color: [0.6, 0.4, 0.9, 1.0], stroke_width: 0.0, anti_alias: true });

        // 15. L-Shape
        painter.text("15. L-Shape", Point::new(520.0, 250.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let l_shape = vec![
            Point::new(540.0, 270.0),
            Point::new(560.0, 270.0),
            Point::new(560.0, 290.0),
            Point::new(580.0, 290.0),
            Point::new(580.0, 300.0),
            Point::new(540.0, 300.0),
        ];
        painter.polygon(&l_shape, &Paint { color: [0.3, 0.6, 0.7, 1.0], stroke_width: 0.0, anti_alias: true });

        // 16. Concave Polygon
        painter.text("16. Concave", Point::new(50.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let concave = vec![
            Point::new(70.0, 340.0),
            Point::new(100.0, 355.0),
            Point::new(130.0, 340.0),
            Point::new(110.0, 370.0),
            Point::new(90.0, 370.0),
        ];
        painter.polygon(&concave, &Paint { color: [0.8, 0.3, 0.5, 1.0], stroke_width: 0.0, anti_alias: true });

        // 17. Multi-point Star
        painter.text("17. 8pt Star", Point::new(180.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_star(painter, Point::new(230.0, 355.0), 28.0, 14.0, 8, [0.9, 0.7, 0.3, 1.0]);

        // 18. Cross
        painter.text("18. Cross", Point::new(280.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let cross_h = vec![
            Point::new(310.0, 345.0),
            Point::new(350.0, 345.0),
            Point::new(350.0, 365.0),
            Point::new(310.0, 365.0),
        ];
        painter.polygon(&cross_h, &Paint { color: [0.9, 0.3, 0.3, 1.0], stroke_width: 0.0, anti_alias: true });
        let cross_v = vec![
            Point::new(320.0, 335.0),
            Point::new(340.0, 335.0),
            Point::new(340.0, 375.0),
            Point::new(320.0, 375.0),
        ];
        painter.polygon(&cross_v, &Paint { color: [0.9, 0.3, 0.3, 1.0], stroke_width: 0.0, anti_alias: true });

        // 19. Circle approximation (many-sided polygon)
        painter.text("19. Circle (polygon)", Point::new(400.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let mut circle_poly = Vec::new();
        for i in 0..32 {
            let angle = i as f32 * 2.0 * std::f32::consts::PI / 32.0;
            circle_poly.push(Point::new(
                450.0 + 25.0 * angle.cos(),
                355.0 + 25.0 * angle.sin(),
            ));
        }
        painter.polygon(&circle_poly, &Paint { color: [0.4, 0.7, 0.9, 1.0], stroke_width: 0.0, anti_alias: true });

        // 20. Complex shape
        painter.text("20. Complex", Point::new(520.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        let complex = vec![
            Point::new(550.0, 340.0),
            Point::new(570.0, 345.0),
            Point::new(580.0, 340.0),
            Point::new(585.0, 355.0),
            Point::new(575.0, 370.0),
            Point::new(555.0, 365.0),
            Point::new(545.0, 355.0),
        ];
        painter.polygon(&complex, &Paint { color: [0.7, 0.5, 0.8, 1.0], stroke_width: 0.0, anti_alias: true });

        // Footer
        painter.text(
            "All shapes rendered using Painter::polygon() with anti-aliasing",
            Point::new(180.0, 570.0),
            11.0,
            &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Path & Polygon Test ===");
    println!("Systematic testing of polygon rendering:");
    println!("  1. Triangle - 3 sides");
    println!("  2. Square - 4 sides");
    println!("  3. Pentagon - 5 sides");
    println!("  4. Hexagon - 6 sides");
    println!("  5. Octagon - 8 sides");
    println!("  6. Star (5-point)");
    println!("  7. Star (6-point)");
    println!("  8. Arrow");
    println!("  9. Heart");
    println!("  10. Diamond");
    println!("  11. Chevron");
    println!("  12. Plus Sign");
    println!("  13. Trapezoid");
    println!("  14. Parallelogram");
    println!("  15. L-Shape");
    println!("  16. Concave Polygon");
    println!("  17. 8-point Star");
    println!("  18. Cross");
    println!("  19. Circle (32-sided polygon)");
    println!("  20. Complex Shape");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Path & Polygon Test")
        .size(800, 600);

    app.run(PathTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

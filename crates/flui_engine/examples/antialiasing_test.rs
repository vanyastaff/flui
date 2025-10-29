//! Systematic test for anti-aliasing and rendering quality

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint, RRect};
use flui_types::{Event, Rect, Point};

struct AntialiasingTestApp;

impl AppLogic for AntialiasingTestApp {
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
            "Anti-Aliasing & Quality Test",
            Point::new(230.0, 30.0),
            20.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        // 1. Diagonal Lines
        painter.text("1. Diagonal Lines", Point::new(50.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::PI / 4.0;
            let len = 35.0;
            let cx = 100.0;
            let cy = 115.0;
            painter.line(
                Point::new(cx, cy),
                Point::new(cx + len * angle.cos(), cy + len * angle.sin()),
                &Paint { color: [0.3, 0.3, 0.3, 1.0], stroke_width: 2.0, anti_alias: true }
            );
        }

        // 2. Thin Lines
        painter.text("2. Thin Lines", Point::new(200.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..5 {
            let thickness = 0.5 + i as f32 * 0.5;
            painter.line(
                Point::new(210.0, 95.0 + i as f32 * 12.0),
                Point::new(290.0, 95.0 + i as f32 * 12.0),
                &Paint { color: [0.3, 0.3, 0.3, 1.0], stroke_width: thickness, anti_alias: true }
            );
            painter.text(
                &format!("{:.1}px", thickness),
                Point::new(295.0, 92.0 + i as f32 * 12.0),
                10.0,
                &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
            );
        }

        // 3. Rotated Rectangles
        painter.text("3. Rotated Rects", Point::new(380.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        painter.save();
        painter.translate(flui_types::Offset::new(450.0, 115.0));
        for _i in 0..6 {
            painter.rotate(std::f32::consts::PI / 6.0);
            painter.rect(
                Rect::from_xywh(-15.0, -15.0, 30.0, 30.0),
                &Paint { color: [0.3, 0.6, 0.9, 0.7], stroke_width: 0.0, anti_alias: true }
            );
        }
        painter.restore();

        // 4. Circle Edges
        painter.text("4. Circle Edges", Point::new(550.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        painter.circle(
            Point::new(620.0, 115.0),
            35.0,
            &Paint { color: [0.9, 0.3, 0.3, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // 5. Small Shapes
        painter.text("5. Small Shapes", Point::new(50.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..8 {
            let size = 3.0 + i as f32 * 2.0;
            painter.rect(
                Rect::from_xywh(55.0 + i as f32 * 16.0, 185.0, size, size),
                &Paint { color: [0.3, 0.6, 0.9, 1.0], stroke_width: 0.0, anti_alias: true }
            );
        }

        // 6. Rounded Corners
        painter.text("6. Rounded Corners", Point::new(200.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..5 {
            let radius = 2.0 + i as f32 * 3.0;
            painter.rrect(
                RRect {
                    rect: Rect::from_xywh(205.0 + i as f32 * 22.0, 180.0, 18.0, 18.0),
                    corner_radius: radius,
                },
                &Paint { color: [0.3, 0.8, 0.5, 1.0], stroke_width: 0.0, anti_alias: true }
            );
        }

        // 7. Overlapping Circles
        painter.text("7. Overlapping", Point::new(380.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        painter.circle(
            Point::new(420.0, 195.0),
            20.0,
            &Paint { color: [0.9, 0.3, 0.3, 0.7], stroke_width: 0.0, anti_alias: true }
        );
        painter.circle(
            Point::new(435.0, 195.0),
            20.0,
            &Paint { color: [0.3, 0.9, 0.3, 0.7], stroke_width: 0.0, anti_alias: true }
        );
        painter.circle(
            Point::new(427.5, 208.0),
            20.0,
            &Paint { color: [0.3, 0.3, 0.9, 0.7], stroke_width: 0.0, anti_alias: true }
        );

        // 8. Line Caps
        painter.text("8. Line Caps", Point::new(550.0, 160.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..4 {
            painter.line(
                Point::new(560.0, 185.0 + i as f32 * 10.0),
                Point::new(660.0, 185.0 + i as f32 * 10.0),
                &Paint { color: [0.3, 0.3, 0.3, 1.0], stroke_width: 4.0 + i as f32, anti_alias: true }
            );
        }

        // 9. Stroke vs Fill
        painter.text("9. Stroke vs Fill", Point::new(50.0, 240.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        painter.rect(
            Rect::from_xywh(60.0, 260.0, 40.0, 40.0),
            &Paint { color: [0.3, 0.6, 0.9, 1.0], stroke_width: 0.0, anti_alias: true }
        );
        painter.rect(
            Rect::from_xywh(110.0, 260.0, 40.0, 40.0),
            &Paint { color: [0.3, 0.6, 0.9, 1.0], stroke_width: 2.0, anti_alias: true }
        );

        // 10. Pixel-Aligned vs Sub-pixel
        painter.text("10. Alignment", Point::new(200.0, 240.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        // Pixel-aligned
        painter.rect(
            Rect::from_xywh(210.0, 260.0, 40.0, 40.0),
            &Paint { color: [0.3, 0.6, 0.9, 1.0], stroke_width: 0.0, anti_alias: true }
        );
        // Sub-pixel
        painter.rect(
            Rect::from_xywh(260.5, 260.5, 40.0, 40.0),
            &Paint { color: [0.9, 0.3, 0.3, 1.0], stroke_width: 0.0, anti_alias: true }
        );

        // 11. Gradient Edges
        painter.text("11. Smooth Gradients", Point::new(380.0, 240.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..10 {
            let alpha = 1.0 - (i as f32 / 10.0);
            painter.circle(
                Point::new(440.0, 280.0),
                8.0 + i as f32 * 3.0,
                &Paint { color: [0.3, 0.6, 0.9, alpha * 0.3], stroke_width: 0.0, anti_alias: true }
            );
        }

        // 12. Angled Lines
        painter.text("12. All Angles", Point::new(550.0, 240.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..12 {
            let angle = i as f32 * std::f32::consts::PI / 6.0;
            painter.line(
                Point::new(620.0, 280.0),
                Point::new(620.0 + 40.0 * angle.cos(), 280.0 + 40.0 * angle.sin()),
                &Paint { color: [0.3, 0.3, 0.3, 1.0], stroke_width: 1.5, anti_alias: true }
            );
        }

        // 13. Very Small Circles
        painter.text("13. Tiny Circles", Point::new(50.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..10 {
            let radius = 1.0 + i as f32 * 1.5;
            painter.circle(
                Point::new(60.0 + i as f32 * 18.0, 350.0),
                radius,
                &Paint { color: [0.9, 0.3, 0.3, 1.0], stroke_width: 0.0, anti_alias: true }
            );
        }

        // 14. Polygon Smoothness
        painter.text("14. Polygon Edges", Point::new(200.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        let hexagon = vec![
            Point::new(255.0, 340.0),
            Point::new(275.0, 345.0),
            Point::new(275.0, 360.0),
            Point::new(255.0, 365.0),
            Point::new(235.0, 360.0),
            Point::new(235.0, 345.0),
        ];
        painter.polygon(&hexagon, &Paint { color: [0.3, 0.8, 0.5, 1.0], stroke_width: 0.0, anti_alias: true });

        // 15. Dashed Effect (using multiple lines)
        painter.text("15. Dash Pattern", Point::new(380.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..8 {
            painter.line(
                Point::new(390.0 + i as f32 * 12.0, 345.0),
                Point::new(395.0 + i as f32 * 12.0, 345.0),
                &Paint { color: [0.3, 0.3, 0.3, 1.0], stroke_width: 2.0, anti_alias: true }
            );
        }

        // 16. Text Anti-aliasing
        painter.text("16. Text Quality", Point::new(550.0, 320.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        painter.text("Smooth", Point::new(555.0, 345.0), 20.0,
            &Paint { color: [0.3, 0.3, 0.3, 1.0], ..Default::default() });

        // 17. Zoom Test (small shapes)
        painter.text("17. Detail Test", Point::new(50.0, 390.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..20 {
            painter.line(
                Point::new(60.0 + i as f32 * 8.0, 410.0),
                Point::new(60.0 + i as f32 * 8.0, 430.0),
                &Paint { color: [0.3, 0.6, 0.9, 1.0], stroke_width: 1.0, anti_alias: true }
            );
        }

        // 18. Transparency + AA
        painter.text("18. Alpha + AA", Point::new(250.0, 390.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..4 {
            let alpha = 0.25 + i as f32 * 0.25;
            painter.circle(
                Point::new(290.0 + i as f32 * 25.0, 420.0),
                15.0,
                &Paint { color: [0.9, 0.3, 0.3, alpha], stroke_width: 0.0, anti_alias: true }
            );
        }

        // 19. Sharp vs Smooth
        painter.text("19. Edge Quality", Point::new(450.0, 390.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        painter.circle(
            Point::new(490.0, 420.0),
            25.0,
            &Paint { color: [0.3, 0.6, 0.9, 1.0], stroke_width: 3.0, anti_alias: true }
        );

        // 20. Complex Overlap
        painter.text("20. Complex AA", Point::new(50.0, 460.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });

        for i in 0..5 {
            painter.circle(
                Point::new(100.0 + i as f32 * 12.0, 495.0),
                18.0,
                &Paint { color: [0.3 + i as f32 * 0.15, 0.5, 0.9 - i as f32 * 0.15, 0.6], stroke_width: 0.0, anti_alias: true }
            );
        }

        // Footer
        painter.text(
            "All rendering uses anti_alias: true for smooth edges",
            Point::new(200.0, 570.0),
            11.0,
            &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Anti-Aliasing & Quality Test ===");
    println!("Systematic testing of rendering quality:");
    println!("  1. Diagonal Lines - various angles");
    println!("  2. Thin Lines - 0.5px to 2.5px");
    println!("  3. Rotated Rectangles");
    println!("  4. Circle Edges - smooth curves");
    println!("  5. Small Shapes - 3px to 17px");
    println!("  6. Rounded Corners - 2px to 14px radius");
    println!("  7. Overlapping Circles - blend testing");
    println!("  8. Line Caps - various thicknesses");
    println!("  9. Stroke vs Fill");
    println!("  10. Pixel-Aligned vs Sub-pixel");
    println!("  11. Smooth Gradients - concentric circles");
    println!("  12. All Angles - 30° increments");
    println!("  13. Tiny Circles - 1px to 15px radius");
    println!("  14. Polygon Edges - hexagon smoothness");
    println!("  15. Dash Pattern - line segments");
    println!("  16. Text Quality");
    println!("  17. Detail Test - thin vertical lines");
    println!("  18. Alpha + AA - transparency blending");
    println!("  19. Edge Quality - stroked circle");
    println!("  20. Complex AA - overlapping shapes");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Anti-Aliasing Test")
        .size(800, 600);

    app.run(AntialiasingTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

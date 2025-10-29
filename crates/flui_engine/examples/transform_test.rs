//! Systematic test for transformations

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint};
use flui_types::{Event, Rect, Point, Offset};

struct TransformTestApp;

impl AppLogic for TransformTestApp {
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
            "Transform Test - Systematic",
            Point::new(250.0, 30.0),
            20.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let box_paint = Paint {
            color: [0.2, 0.5, 0.8, 1.0],
            stroke_width: 0.0,
            anti_alias: true,
        };

        // 1. Original (no transform)
        painter.text("Original", Point::new(50.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.rect(Rect::from_xywh(50.0, 90.0, 80.0, 60.0), &box_paint);

        // 2. Translate
        painter.text("Translate (100, 0)", Point::new(200.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(100.0, 0.0));
        painter.rect(Rect::from_xywh(200.0, 90.0, 80.0, 60.0), &box_paint);
        painter.restore();

        // 3. Scale 1.5x
        painter.text("Scale 1.5x", Point::new(450.0, 70.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(490.0, 120.0));
        painter.scale(1.5, 1.5);
        painter.rect(Rect::from_xywh(-40.0, -30.0, 80.0, 60.0), &box_paint);
        painter.restore();

        // 4. Rotate 45°
        painter.text("Rotate 45°", Point::new(50.0, 200.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(90.0, 250.0));
        painter.rotate(std::f32::consts::PI / 4.0);
        painter.rect(Rect::from_xywh(-40.0, -30.0, 80.0, 60.0), &box_paint);
        painter.restore();

        // 5. Skew X
        painter.text("Skew X (0.5)", Point::new(200.0, 200.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(240.0, 220.0));
        painter.skew(0.5, 0.0);
        painter.rect(Rect::from_xywh(0.0, 0.0, 80.0, 60.0), &box_paint);
        painter.restore();

        // 6. Skew Y
        painter.text("Skew Y (0.5)", Point::new(400.0, 200.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(440.0, 220.0));
        painter.skew(0.0, 0.5);
        painter.rect(Rect::from_xywh(0.0, 0.0, 80.0, 60.0), &box_paint);
        painter.restore();

        // 7. Combined: Scale + Rotate
        painter.text("Scale 1.2x + Rotate 30°", Point::new(50.0, 350.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(130.0, 410.0));
        painter.scale(1.2, 1.2);
        painter.rotate(std::f32::consts::PI / 6.0);
        painter.rect(Rect::from_xywh(-40.0, -30.0, 80.0, 60.0), &box_paint);
        painter.restore();

        // 8. Combined: Translate + Skew
        painter.text("Translate + Skew", Point::new(300.0, 350.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(340.0, 380.0));
        painter.skew(0.3, 0.2);
        painter.rect(Rect::from_xywh(0.0, 0.0, 80.0, 60.0), &box_paint);
        painter.restore();

        // 9. Nested transforms
        painter.text("Nested (outer scale, inner rotate)", Point::new(500.0, 350.0), 14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        painter.save();
        painter.translate(Offset::new(600.0, 410.0));
        painter.scale(0.8, 0.8);

        painter.save();
        painter.rotate(std::f32::consts::PI / 4.0);
        painter.rect(Rect::from_xywh(-40.0, -30.0, 80.0, 60.0), &box_paint);
        painter.restore();

        painter.restore();
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Transform Test ===");
    println!("Systematic testing of transformations:");
    println!("  1. Original (no transform)");
    println!("  2. Translate");
    println!("  3. Scale");
    println!("  4. Rotate");
    println!("  5. Skew X");
    println!("  6. Skew Y");
    println!("  7. Combined: Scale + Rotate");
    println!("  8. Combined: Translate + Skew");
    println!("  9. Nested transforms");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Transform Test")
        .size(800, 600);

    app.run(TransformTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

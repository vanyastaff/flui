//! Systematic test for opacity and transparency

use flui_engine::{App, AppConfig, AppLogic, Paint, Painter};
use flui_types::{events::Event, Point, Rect};

struct OpacityTestApp;

impl AppLogic for OpacityTestApp {
    fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Window(window_event) => {
                if let flui_types::events::WindowEvent::CloseRequested = window_event {
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
            "Opacity & Transparency Test",
            Point::new(230.0, 30.0),
            20.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // 1. Fully Opaque (baseline)
        painter.text(
            "1. Opaque (100%)",
            Point::new(50.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.rect(
            Rect::from_xywh(50.0, 90.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 2. 75% Opacity
        painter.text(
            "2. 75% Opacity",
            Point::new(200.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.rect(
            Rect::from_xywh(200.0, 90.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 0.75],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 3. 50% Opacity
        painter.text(
            "3. 50% Opacity",
            Point::new(350.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.rect(
            Rect::from_xywh(350.0, 90.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 0.5],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 4. 25% Opacity
        painter.text(
            "4. 25% Opacity",
            Point::new(500.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.rect(
            Rect::from_xywh(500.0, 90.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 0.25],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 5. Opacity Gradient (0% to 100%)
        painter.text(
            "5. Opacity Gradient",
            Point::new(50.0, 200.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        for i in 0..20 {
            let alpha = i as f32 / 19.0;
            painter.rect(
                Rect::from_xywh(50.0 + i as f32 * 5.0, 220.0, 5.0, 80.0),
                &Paint {
                    color: [0.3, 0.6, 0.9, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }

        // 6. Overlapping Transparent Shapes
        painter.text(
            "6. Overlapping",
            Point::new(200.0, 200.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Red circle
        painter.circle(
            Point::new(230.0, 260.0),
            30.0,
            &Paint {
                color: [0.9, 0.2, 0.2, 0.6],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Green circle
        painter.circle(
            Point::new(250.0, 260.0),
            30.0,
            &Paint {
                color: [0.2, 0.9, 0.2, 0.6],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Blue circle
        painter.circle(
            Point::new(240.0, 275.0),
            30.0,
            &Paint {
                color: [0.2, 0.2, 0.9, 0.6],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 7. Layered Transparency
        painter.text(
            "7. Layered",
            Point::new(350.0, 200.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        for i in 0..5 {
            let offset = i as f32 * 15.0;
            painter.rect(
                Rect::from_xywh(350.0 + offset, 220.0 + offset, 60.0, 60.0),
                &Paint {
                    color: [0.3, 0.6, 0.9, 0.5],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }

        // 8. Transparent Stroke
        painter.text(
            "8. Transparent Stroke",
            Point::new(500.0, 200.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.rect(
            Rect::from_xywh(510.0, 230.0, 80.0, 60.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 0.4],
                stroke_width: 4.0,
                anti_alias: true,
            },
        );

        // 9. Checkerboard Pattern (show transparency)
        painter.text(
            "9. Checkerboard Test",
            Point::new(50.0, 330.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Create checkerboard background
        for y in 0..4 {
            for x in 0..5 {
                if (x + y) % 2 == 0 {
                    painter.rect(
                        Rect::from_xywh(
                            50.0 + x as f32 * 20.0,
                            350.0 + y as f32 * 20.0,
                            20.0,
                            20.0,
                        ),
                        &Paint {
                            color: [0.8, 0.8, 0.8, 1.0],
                            stroke_width: 0.0,
                            anti_alias: true,
                        },
                    );
                } else {
                    painter.rect(
                        Rect::from_xywh(
                            50.0 + x as f32 * 20.0,
                            350.0 + y as f32 * 20.0,
                            20.0,
                            20.0,
                        ),
                        &Paint {
                            color: [0.6, 0.6, 0.6, 1.0],
                            stroke_width: 0.0,
                            anti_alias: true,
                        },
                    );
                }
            }
        }
        // Semi-transparent overlay
        painter.rect(
            Rect::from_xywh(50.0, 350.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 0.5],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 10. Alpha Blending Test
        painter.text(
            "10. Color Blending",
            Point::new(200.0, 330.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Base color (yellow)
        painter.rect(
            Rect::from_xywh(200.0, 350.0, 100.0, 80.0),
            &Paint {
                color: [1.0, 0.9, 0.0, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Overlay (blue, semi-transparent)
        painter.rect(
            Rect::from_xywh(210.0, 360.0, 80.0, 60.0),
            &Paint {
                color: [0.0, 0.2, 1.0, 0.5],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 11. Fading Effect
        painter.text(
            "11. Fade Out",
            Point::new(350.0, 330.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        for i in 0..10 {
            let alpha = 1.0 - (i as f32 / 10.0);
            painter.circle(
                Point::new(360.0 + i as f32 * 9.0, 390.0),
                8.0,
                &Paint {
                    color: [0.9, 0.3, 0.3, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }

        // 12. Multiple Opacity Levels
        painter.text(
            "12. Opacity Steps",
            Point::new(500.0, 330.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        let opacities = [1.0, 0.8, 0.6, 0.4, 0.2];
        for (i, &alpha) in opacities.iter().enumerate() {
            painter.rect(
                Rect::from_xywh(505.0 + i as f32 * 18.0, 350.0, 15.0, 80.0),
                &Paint {
                    color: [0.3, 0.6, 0.9, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }

        // 13. Glass Effect (multiple layers)
        painter.text(
            "13. Glass Effect",
            Point::new(50.0, 460.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Background elements
        painter.rect(
            Rect::from_xywh(60.0, 490.0, 30.0, 60.0),
            &Paint {
                color: [0.9, 0.3, 0.3, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        painter.rect(
            Rect::from_xywh(100.0, 490.0, 30.0, 60.0),
            &Paint {
                color: [0.3, 0.9, 0.3, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Glass overlay
        painter.rect(
            Rect::from_xywh(55.0, 485.0, 80.0, 70.0),
            &Paint {
                color: [0.9, 0.9, 1.0, 0.3],
                stroke_width: 2.0,
                anti_alias: true,
            },
        );

        // 14. Transparent Text
        painter.text(
            "14. Transparent Text",
            Point::new(200.0, 460.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Background
        painter.rect(
            Rect::from_xywh(200.0, 480.0, 150.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Semi-transparent text
        painter.text(
            "TRANSPARENT",
            Point::new(210.0, 510.0),
            18.0,
            &Paint {
                color: [1.0, 1.0, 1.0, 0.4],
                ..Default::default()
            },
        );

        // 15. Opacity with Circles
        painter.text(
            "15. Circular Fade",
            Point::new(400.0, 460.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        for i in 0..6 {
            let alpha = 1.0 - (i as f32 / 6.0);
            painter.circle(
                Point::new(470.0, 520.0),
                10.0 + i as f32 * 8.0,
                &Paint {
                    color: [0.9, 0.2, 0.6, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }

        // Footer note
        painter.text(
            "Alpha channel values: 1.0 = fully opaque, 0.0 = fully transparent",
            Point::new(170.0, 570.0),
            11.0,
            &Paint {
                color: [0.5, 0.5, 0.5, 1.0],
                ..Default::default()
            },
        );
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Opacity & Transparency Test ===");
    println!("Systematic testing of opacity and alpha blending:");
    println!("  1. Fully Opaque (100%) - baseline");
    println!("  2. 75% Opacity");
    println!("  3. 50% Opacity");
    println!("  4. 25% Opacity");
    println!("  5. Opacity Gradient - 0% to 100%");
    println!("  6. Overlapping Transparent Shapes");
    println!("  7. Layered Transparency");
    println!("  8. Transparent Stroke");
    println!("  9. Checkerboard Test - shows transparency clearly");
    println!("  10. Color Blending - alpha compositing");
    println!("  11. Fade Out Effect");
    println!("  12. Opacity Steps - discrete levels");
    println!("  13. Glass Effect - frosted glass simulation");
    println!("  14. Transparent Text");
    println!("  15. Circular Fade - concentric circles");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Opacity Test")
        .size(800, 600);

    app.run(OpacityTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

//! Systematic test for shadow effects

use flui_engine::{App, AppConfig, AppLogic, Paint, Painter};
use flui_types::{Event, Point, Rect};

struct ShadowTestApp;

impl AppLogic for ShadowTestApp {
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
            "Shadow Test - Systematic",
            Point::new(250.0, 30.0),
            20.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // Note: Shadow layer implementation
        painter.text(
            "Note: Shadows would be rendered using ShadowLayer in the scene graph",
            Point::new(130.0, 55.0),
            12.0,
            &Paint {
                color: [0.4, 0.4, 0.4, 1.0],
                ..Default::default()
            },
        );

        // 1. No Shadow (baseline)
        painter.text(
            "1. No Shadow",
            Point::new(50.0, 90.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.rect(
            Rect::from_xywh(50.0, 110.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 2. Simple Drop Shadow (simulated with dark rect offset)
        painter.text(
            "2. Drop Shadow",
            Point::new(200.0, 90.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Shadow (dark, offset)
        painter.rect(
            Rect::from_xywh(205.0, 115.0, 100.0, 80.0),
            &Paint {
                color: [0.0, 0.0, 0.0, 0.3],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Content
        painter.rect(
            Rect::from_xywh(200.0, 110.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 3. Soft Shadow (multiple layers)
        painter.text(
            "3. Soft Shadow",
            Point::new(350.0, 90.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Multiple shadow layers for blur effect
        for i in 0..5 {
            let offset = 3.0 + i as f32 * 1.5;
            let alpha = 0.15 - i as f32 * 0.02;
            painter.rect(
                Rect::from_xywh(350.0 + offset, 110.0 + offset, 100.0, 80.0),
                &Paint {
                    color: [0.0, 0.0, 0.0, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }
        // Content
        painter.rect(
            Rect::from_xywh(350.0, 110.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 4. Large Blur Shadow
        painter.text(
            "4. Large Blur",
            Point::new(500.0, 90.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Larger blur simulation
        for i in 0..8 {
            let offset = 5.0 + i as f32 * 2.0;
            let alpha = 0.1 - i as f32 * 0.01;
            painter.rect(
                Rect::from_xywh(500.0 + offset, 110.0 + offset, 100.0, 80.0),
                &Paint {
                    color: [0.0, 0.0, 0.0, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }
        // Content
        painter.rect(
            Rect::from_xywh(500.0, 110.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 5. Colored Shadow
        painter.text(
            "5. Colored Shadow",
            Point::new(50.0, 230.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Red shadow
        painter.rect(
            Rect::from_xywh(55.0, 255.0, 100.0, 80.0),
            &Paint {
                color: [0.9, 0.2, 0.2, 0.5],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Content
        painter.rect(
            Rect::from_xywh(50.0, 250.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 6. Multiple Shadows
        painter.text(
            "6. Multiple Shadows",
            Point::new(200.0, 230.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Shadow 1 - bottom right
        painter.rect(
            Rect::from_xywh(205.0, 255.0, 100.0, 80.0),
            &Paint {
                color: [0.0, 0.0, 0.0, 0.3],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Shadow 2 - top left (highlight)
        painter.rect(
            Rect::from_xywh(195.0, 245.0, 100.0, 80.0),
            &Paint {
                color: [1.0, 1.0, 1.0, 0.5],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Content
        painter.rect(
            Rect::from_xywh(200.0, 250.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 7. Inner Shadow (inverted)
        painter.text(
            "7. Inner Shadow",
            Point::new(350.0, 230.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Content (lighter for visibility)
        painter.rect(
            Rect::from_xywh(350.0, 250.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Inner shadow (smaller, darker rect on top)
        painter.rect(
            Rect::from_xywh(353.0, 253.0, 94.0, 74.0),
            &Paint {
                color: [0.0, 0.0, 0.0, 0.2],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 8. Elevation Shadow (material design style)
        painter.text(
            "8. Elevation Shadow",
            Point::new(500.0, 230.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Key light shadow (sharp)
        painter.rect(
            Rect::from_xywh(502.0, 254.0, 100.0, 80.0),
            &Paint {
                color: [0.0, 0.0, 0.0, 0.25],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Ambient light shadow (soft, spread)
        for i in 0..4 {
            let offset = i as f32 * 2.0;
            let alpha = 0.08 - i as f32 * 0.015;
            painter.rect(
                Rect::from_xywh(500.0 + offset, 252.0 + offset, 100.0, 80.0),
                &Paint {
                    color: [0.0, 0.0, 0.0, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }
        // Content
        painter.rect(
            Rect::from_xywh(500.0, 250.0, 100.0, 80.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 9. Circular Shadow
        painter.text(
            "9. Circular Shadow",
            Point::new(50.0, 370.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Shadow circles
        for i in 0..5 {
            let offset = 2.0 + i as f32;
            let alpha = 0.15 - i as f32 * 0.025;
            painter.circle(
                Point::new(100.0 + offset, 440.0 + offset),
                40.0,
                &Paint {
                    color: [0.0, 0.0, 0.0, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }
        // Content circle
        painter.circle(
            Point::new(100.0, 440.0),
            40.0,
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 10. Inset Shadow on Circle
        painter.text(
            "10. Inset Shadow",
            Point::new(200.0, 370.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Outer circle
        painter.circle(
            Point::new(250.0, 440.0),
            40.0,
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        // Inner shadow (smaller darker circle)
        painter.circle(
            Point::new(252.0, 442.0),
            36.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 0.2],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 11. Glow Effect (outward shadow)
        painter.text(
            "11. Glow Effect",
            Point::new(350.0, 370.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Glow layers (cyan glow)
        for i in (0..8).rev() {
            let offset = i as f32 * 3.0;
            let alpha = 0.15 - i as f32 * 0.018;
            painter.circle(
                Point::new(400.0, 440.0),
                40.0 + offset,
                &Paint {
                    color: [0.0, 0.8, 1.0, alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                },
            );
        }
        // Content
        painter.circle(
            Point::new(400.0, 440.0),
            40.0,
            &Paint {
                color: [0.0, 0.8, 1.0, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // 12. Long Shadow (flat design)
        painter.text(
            "12. Long Shadow",
            Point::new(500.0, 370.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        // Long shadow trail
        for i in 0..20 {
            let offset = i as f32 * 2.0;
            let alpha = 0.2 - i as f32 * 0.01;
            if alpha > 0.0 {
                painter.rect(
                    Rect::from_xywh(550.0 + offset, 410.0 + offset, 50.0, 50.0),
                    &Paint {
                        color: [0.0, 0.0, 0.0, alpha],
                        stroke_width: 0.0,
                        anti_alias: true,
                    },
                );
            }
        }
        // Content
        painter.rect(
            Rect::from_xywh(550.0, 410.0, 50.0, 50.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // Footer note
        painter.text(
            "All shadows are simulated using Painter primitives for testing purposes",
            Point::new(150.0, 570.0),
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
    println!("=== Shadow Test ===");
    println!("Systematic testing of shadow effects:");
    println!("  1. No Shadow (baseline)");
    println!("  2. Drop Shadow - simple offset");
    println!("  3. Soft Shadow - multi-layer blur");
    println!("  4. Large Blur - extensive shadow spread");
    println!("  5. Colored Shadow - non-black shadow");
    println!("  6. Multiple Shadows - combined shadows");
    println!("  7. Inner Shadow - inset effect");
    println!("  8. Elevation Shadow - Material Design style");
    println!("  9. Circular Shadow - shadow on circle");
    println!("  10. Inset Shadow - inner shadow on circle");
    println!("  11. Glow Effect - outward colored glow");
    println!("  12. Long Shadow - flat design style");
    println!();
    println!("Note: These are simulated using Painter primitives.");
    println!("      ShadowLayer would provide proper shadow rendering in the scene graph.");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Shadow Test")
        .size(800, 600);

    app.run(ShadowTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

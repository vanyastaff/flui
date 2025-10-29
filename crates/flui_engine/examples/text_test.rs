//! Systematic test for text rendering

use flui_engine::{App, AppConfig, AppLogic, Paint, Painter};
use flui_types::{events::Event, Point, Rect};

struct TextTestApp;

impl AppLogic for TextTestApp {
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
            "Text Rendering Test - Systematic",
            Point::new(210.0, 30.0),
            20.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // 1. Font Sizes
        painter.text(
            "1. Font Sizes",
            Point::new(50.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "10px",
            Point::new(50.0, 95.0),
            10.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "14px",
            Point::new(50.0, 110.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "18px",
            Point::new(50.0, 130.0),
            18.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "24px",
            Point::new(50.0, 155.0),
            24.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "32px",
            Point::new(50.0, 185.0),
            32.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );

        // 2. Text Colors
        painter.text(
            "2. Colors",
            Point::new(200.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "Black",
            Point::new(200.0, 95.0),
            16.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Red",
            Point::new(200.0, 120.0),
            16.0,
            &Paint {
                color: [0.9, 0.2, 0.2, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Green",
            Point::new(200.0, 145.0),
            16.0,
            &Paint {
                color: [0.2, 0.7, 0.2, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Blue",
            Point::new(200.0, 170.0),
            16.0,
            &Paint {
                color: [0.2, 0.4, 0.9, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Gray",
            Point::new(200.0, 195.0),
            16.0,
            &Paint {
                color: [0.5, 0.5, 0.5, 1.0],
                ..Default::default()
            },
        );

        // 3. Text on Colored Background
        painter.text(
            "3. On Background",
            Point::new(350.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // Dark background
        painter.rect(
            Rect::from_xywh(350.0, 90.0, 150.0, 30.0),
            &Paint {
                color: [0.2, 0.2, 0.3, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        painter.text(
            "White on Dark",
            Point::new(355.0, 107.0),
            14.0,
            &Paint {
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
        );

        // Colored background
        painter.rect(
            Rect::from_xywh(350.0, 130.0, 150.0, 30.0),
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        painter.text(
            "White on Blue",
            Point::new(355.0, 147.0),
            14.0,
            &Paint {
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
        );

        // Light background
        painter.rect(
            Rect::from_xywh(350.0, 170.0, 150.0, 30.0),
            &Paint {
                color: [0.9, 0.9, 0.7, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );
        painter.text(
            "Dark on Light",
            Point::new(355.0, 187.0),
            14.0,
            &Paint {
                color: [0.2, 0.2, 0.2, 1.0],
                ..Default::default()
            },
        );

        // 4. Text Opacity
        painter.text(
            "4. Opacity",
            Point::new(550.0, 70.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "100% Opacity",
            Point::new(550.0, 95.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "75% Opacity",
            Point::new(550.0, 120.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 0.75],
                ..Default::default()
            },
        );
        painter.text(
            "50% Opacity",
            Point::new(550.0, 145.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 0.5],
                ..Default::default()
            },
        );
        painter.text(
            "25% Opacity",
            Point::new(550.0, 170.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 0.25],
                ..Default::default()
            },
        );

        // 5. Special Characters
        painter.text(
            "5. Special Chars",
            Point::new(50.0, 230.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "Numbers: 0123456789",
            Point::new(50.0, 255.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Symbols: !@#$%^&*()",
            Point::new(50.0, 275.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Punctuation: .,;:'\"?",
            Point::new(50.0, 295.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Math: +-*/=<>[]{}|",
            Point::new(50.0, 315.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );

        // 6. Long Text
        painter.text(
            "6. Long Text",
            Point::new(350.0, 230.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "The quick brown fox jumps",
            Point::new(350.0, 255.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "over the lazy dog.",
            Point::new(350.0, 275.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Pack my box with five",
            Point::new(350.0, 295.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "dozen liquor jugs.",
            Point::new(350.0, 315.0),
            14.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );

        // 7. Text Baseline Alignment
        painter.text(
            "7. Baseline",
            Point::new(50.0, 350.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        // Draw baseline
        painter.line(
            Point::new(50.0, 385.0),
            Point::new(300.0, 385.0),
            &Paint {
                color: [0.9, 0.3, 0.3, 0.5],
                stroke_width: 1.0,
                anti_alias: true,
            },
        );

        painter.text(
            "Small",
            Point::new(55.0, 385.0),
            12.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Medium",
            Point::new(110.0, 385.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "Large",
            Point::new(195.0, 385.0),
            24.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );

        // 8. Uppercase vs Lowercase
        painter.text(
            "8. Case",
            Point::new(350.0, 350.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "lowercase text",
            Point::new(350.0, 375.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "UPPERCASE TEXT",
            Point::new(350.0, 400.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "MixedCase Text",
            Point::new(350.0, 425.0),
            16.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );

        // 9. Small Text
        painter.text(
            "9. Small Sizes",
            Point::new(50.0, 420.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "This is 8px text (very small)",
            Point::new(50.0, 445.0),
            8.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "This is 10px text (small)",
            Point::new(50.0, 460.0),
            10.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "This is 12px text (readable)",
            Point::new(50.0, 477.0),
            12.0,
            &Paint {
                color: [0.3, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );

        // 10. Large Text
        painter.text(
            "10. Large Sizes",
            Point::new(350.0, 460.0),
            14.0,
            &Paint {
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        );

        painter.text(
            "BIG",
            Point::new(350.0, 495.0),
            36.0,
            &Paint {
                color: [0.3, 0.6, 0.9, 1.0],
                ..Default::default()
            },
        );
        painter.text(
            "HUGE",
            Point::new(480.0, 505.0),
            48.0,
            &Paint {
                color: [0.9, 0.3, 0.3, 1.0],
                ..Default::default()
            },
        );

        // Footer note
        painter.text(
            "Text rendering using Painter::text() method",
            Point::new(230.0, 570.0),
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
    println!("=== Text Rendering Test ===");
    println!("Systematic testing of text rendering:");
    println!("  1. Font Sizes - from 10px to 32px");
    println!("  2. Colors - various text colors");
    println!("  3. On Background - white on dark, dark on light");
    println!("  4. Opacity - 100%, 75%, 50%, 25%");
    println!("  5. Special Characters - numbers, symbols, punctuation");
    println!("  6. Long Text - multi-line text rendering");
    println!("  7. Baseline Alignment - different sizes on same baseline");
    println!("  8. Case - uppercase, lowercase, mixed");
    println!("  9. Small Sizes - 8px, 10px, 12px");
    println!("  10. Large Sizes - 36px, 48px");

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Text Rendering Test")
        .size(800, 600);

    app.run(TextTestApp).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

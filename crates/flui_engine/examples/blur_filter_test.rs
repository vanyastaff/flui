//! Blur and Filter effects test - demonstrates image filtering capabilities
//!
//! This example shows:
//! - Blur effects with different sigma values
//! - Color filters (opacity, brightness, etc.)
//! - Backdrop blur (frosted glass effect)
//! - Composed filters (multiple effects combined)

use flui_engine::{App, AppLogic, Paint, Painter};
use flui_types::{
    events::Event,
    painting::effects::{ColorFilter, ImageFilter},
    Color, Point, Rect,
};

struct BlurFilterApp {
    // No state needed for this static demo
}

impl BlurFilterApp {
    fn new() -> Self {
        Self {}
    }

    /// Draw a colored box as content to be filtered
    fn draw_sample_content(&self, painter: &mut dyn Painter, rect: Rect) {
        // Colorful gradient background
        painter.rect(rect, &Paint::fill(Color::rgb(100, 150, 255)));

        // Some shapes to make blur visible
        let center = rect.center();
        painter.circle(
            Point::new(center.x - 30.0, center.y),
            20.0,
            &Paint::fill(Color::rgb(255, 100, 100)),
        );
        painter.circle(
            Point::new(center.x + 30.0, center.y),
            20.0,
            &Paint::fill(Color::rgb(100, 255, 100)),
        );
    }
}

impl AppLogic for BlurFilterApp {
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

    fn update(&mut self, _delta_time: f32) {
        // No updates needed
    }

    fn render(&mut self, painter: &mut dyn Painter) {
        // Background
        painter.rect(
            Rect::from_xywh(0.0, 0.0, 1200.0, 800.0),
            &Paint::fill(Color::rgb(245, 245, 250)),
        );

        // Title
        painter.text(
            "Blur & Filter Effects Test",
            Point::new(420.0, 30.0),
            24.0,
            &Paint::fill(Color::rgb(40, 40, 40)),
        );

        let y_offset = 80.0;
        let row_height = 180.0;

        // Row 1: Blur effects
        painter.text(
            "Blur Effects (sigma variations)",
            Point::new(50.0, y_offset),
            16.0,
            &Paint::fill(Color::BLACK),
        );

        // Original (no blur)
        painter.save();
        let rect1 = Rect::from_xywh(50.0, y_offset + 30.0, 150.0, 120.0);
        painter.text(
            "Original",
            Point::new(85.0, y_offset + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        self.draw_sample_content(painter, rect1);
        painter.restore();

        // Blur sigma=2
        painter.save();
        let rect2 = Rect::from_xywh(220.0, y_offset + 30.0, 150.0, 120.0);
        painter.text(
            "Blur σ=2",
            Point::new(260.0, y_offset + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        painter.save_layer(rect2, &Paint::default());
        painter.apply_image_filter(&ImageFilter::blur(2.0), rect2);
        self.draw_sample_content(painter, rect2);
        painter.restore();
        painter.restore();

        // Blur sigma=5
        painter.save();
        let rect3 = Rect::from_xywh(390.0, y_offset + 30.0, 150.0, 120.0);
        painter.text(
            "Blur σ=5",
            Point::new(430.0, y_offset + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        painter.save_layer(rect3, &Paint::default());
        painter.apply_image_filter(&ImageFilter::blur(5.0), rect3);
        self.draw_sample_content(painter, rect3);
        painter.restore();
        painter.restore();

        // Blur sigma=10
        painter.save();
        let rect4 = Rect::from_xywh(560.0, y_offset + 30.0, 150.0, 120.0);
        painter.text(
            "Blur σ=10",
            Point::new(595.0, y_offset + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        painter.save_layer(rect4, &Paint::default());
        painter.apply_image_filter(&ImageFilter::blur(10.0), rect4);
        self.draw_sample_content(painter, rect4);
        painter.restore();
        painter.restore();

        // Row 2: Color filters
        let y_offset2 = y_offset + row_height + 40.0;
        painter.text(
            "Color Filters",
            Point::new(50.0, y_offset2),
            16.0,
            &Paint::fill(Color::BLACK),
        );

        // Opacity 50%
        painter.save();
        let rect5 = Rect::from_xywh(50.0, y_offset2 + 30.0, 150.0, 120.0);
        painter.text(
            "Opacity 50%",
            Point::new(75.0, y_offset2 + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        painter.save_layer(rect5, &Paint::default());
        painter.apply_image_filter(&ImageFilter::color(ColorFilter::Opacity(0.5)), rect5);
        self.draw_sample_content(painter, rect5);
        painter.restore();
        painter.restore();

        // Brightness -0.3 (darker)
        painter.save();
        let rect6 = Rect::from_xywh(220.0, y_offset2 + 30.0, 150.0, 120.0);
        painter.text(
            "Brightness -0.3",
            Point::new(240.0, y_offset2 + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        painter.save_layer(rect6, &Paint::default());
        painter.apply_image_filter(&ImageFilter::color(ColorFilter::Brightness(-0.3)), rect6);
        self.draw_sample_content(painter, rect6);
        painter.restore();
        painter.restore();

        // Row 3: Composed filters
        let y_offset3 = y_offset2 + row_height + 40.0;
        painter.text(
            "Composed Filters",
            Point::new(50.0, y_offset3),
            16.0,
            &Paint::fill(Color::BLACK),
        );

        // Blur + Opacity
        painter.save();
        let rect7 = Rect::from_xywh(50.0, y_offset3 + 30.0, 150.0, 120.0);
        painter.text(
            "Blur + Opacity",
            Point::new(70.0, y_offset3 + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        painter.save_layer(rect7, &Paint::default());
        painter.apply_image_filter(
            &ImageFilter::Compose(vec![
                ImageFilter::blur(3.0),
                ImageFilter::color(ColorFilter::Opacity(0.7)),
            ]),
            rect7,
        );
        self.draw_sample_content(painter, rect7);
        painter.restore();
        painter.restore();

        // Blur + Brightness
        painter.save();
        let rect8 = Rect::from_xywh(220.0, y_offset3 + 30.0, 150.0, 120.0);
        painter.text(
            "Blur + Brightness",
            Point::new(230.0, y_offset3 + 20.0),
            12.0,
            &Paint::fill(Color::rgb(100, 100, 100)),
        );
        painter.save_layer(rect8, &Paint::default());
        painter.apply_image_filter(
            &ImageFilter::Compose(vec![
                ImageFilter::blur(4.0),
                ImageFilter::color(ColorFilter::Brightness(-0.2)),
            ]),
            rect8,
        );
        self.draw_sample_content(painter, rect8);
        painter.restore();
        painter.restore();

        // Info text
        painter.text(
            "Note: Egui backend provides basic blur approximation. GPU backends provide full shader-based blur.",
            Point::new(50.0, 750.0),
            11.0,
            &Paint::fill(Color::rgb(120, 120, 120)),
        );
    }
}

fn main() {
    env_logger::init();

    println!("=== FLUI Blur & Filter Effects Test ===");
    println!("Demonstrates ImageFilter and ColorFilter effects");
    println!();

    let app = App::new()
        .title("Blur & Filter Effects Test")
        .size(1200, 800);

    let logic = BlurFilterApp::new();

    app.run(logic).unwrap();
}

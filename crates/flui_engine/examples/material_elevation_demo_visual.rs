//! Material Design Elevation Demo - Visual
//!
//! Demonstrates PhysicalModelLayer with egui backend:
//! - Material Design elevation levels with actual shadows
//! - FAB (Floating Action Button)
//! - App Bar
//! - Cards
//! - Bottom Sheet

use flui_engine::{App, AppConfig, AppLogic, Layer, Paint, Painter};
use flui_engine::layer::{
    PictureLayer, PhysicalModelLayer,
    Elevation, PhysicalShape, DrawCommand,
};
use flui_types::{Color, Rect, Point, Offset, Event};
use flui_types::styling::BorderRadius;

struct MaterialElevationDemo;

impl AppLogic for MaterialElevationDemo {
    fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Window(window_event) => {
                if let flui_types::WindowEvent::CloseRequested = window_event {
                    return false; // Exit
                }
            }
            _ => {}
        }
        true
    }

    fn update(&mut self, _delta_time: f32) {
        // Static scene - no animation
    }

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
            "Material Design Elevation Demo",
            Point::new(250.0, 30.0),
            24.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        // Elevation level showcase
        self.draw_elevation_cards(painter);

        // Material Design components showcase
        self.draw_fab(painter);
        self.draw_app_bar(painter);
        self.draw_content_card(painter);
        self.draw_bottom_sheet(painter);
    }
}

impl MaterialElevationDemo {
    /// Draw elevation level cards
    fn draw_elevation_cards(&self, painter: &mut dyn Painter) {
        painter.text(
            "Elevation Levels:",
            Point::new(40.0, 80.0),
            14.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        let elevations = [
            (Elevation::LEVEL_1, "Level 1\n1dp"),
            (Elevation::LEVEL_2, "Level 2\n3dp"),
            (Elevation::LEVEL_3, "Level 3\n6dp"),
            (Elevation::LEVEL_4, "Level 4\n8dp"),
            (Elevation::LEVEL_5, "Level 5\n12dp"),
        ];

        for (i, (elevation, label)) in elevations.iter().enumerate() {
            let x = 40.0 + i as f32 * 150.0;
            let y = 100.0;

            painter.save();
            painter.translate(Offset::new(x, y));

            let card = self.create_elevated_card(
                Rect::from_xywh(0.0, 0.0, 130.0, 100.0),
                *elevation,
                label,
            );
            card.paint(painter);

            painter.restore();
        }
    }

    /// Draw FAB (Floating Action Button)
    fn draw_fab(&self, painter: &mut dyn Painter) {
        painter.text(
            "FAB:",
            Point::new(700.0, 450.0),
            12.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        painter.save();
        painter.translate(Offset::new(700.0, 500.0));

        let fab = self.create_fab(
            Point::new(28.0, 28.0),
            56.0,
            Elevation::LEVEL_3,
        );
        fab.paint(painter);

        painter.restore();
    }

    /// Draw App Bar
    fn draw_app_bar(&self, painter: &mut dyn Painter) {
        painter.text(
            "App Bar:",
            Point::new(40.0, 250.0),
            12.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        painter.save();
        painter.translate(Offset::new(40.0, 270.0));

        let app_bar = self.create_app_bar(
            Rect::from_xywh(0.0, 0.0, 400.0, 64.0),
            Elevation::LEVEL_4,
        );
        app_bar.paint(painter);

        painter.restore();
    }

    /// Draw Content Card
    fn draw_content_card(&self, painter: &mut dyn Painter) {
        painter.text(
            "Content Card:",
            Point::new(40.0, 360.0),
            12.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        painter.save();
        painter.translate(Offset::new(40.0, 380.0));

        let card = self.create_content_card(
            Rect::from_xywh(0.0, 0.0, 400.0, 180.0),
            Elevation::LEVEL_1,
        );
        card.paint(painter);

        painter.restore();
    }

    /// Draw Bottom Sheet
    fn draw_bottom_sheet(&self, painter: &mut dyn Painter) {
        painter.text(
            "Bottom Sheet:",
            Point::new(480.0, 250.0),
            12.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        painter.save();
        painter.translate(Offset::new(480.0, 270.0));

        let sheet = self.create_bottom_sheet(
            Rect::from_xywh(0.0, 0.0, 200.0, 270.0),
            Elevation::LEVEL_3,
        );
        sheet.paint(painter);

        painter.restore();
    }

    /// Create an elevated card
    fn create_elevated_card(&self, bounds: Rect, elevation: f32, label: &str) -> Box<dyn Layer> {
        let content = self.create_label_surface(bounds, Color::WHITE, label, Color::rgb(33, 33, 33));

        let physical_model = PhysicalModelLayer::new(content)
            .with_elevation(elevation)
            .with_color(Color::WHITE)
            .with_shape(PhysicalShape::Rectangle)
            .with_border_radius(BorderRadius::circular(8.0));

        Box::new(physical_model)
    }

    /// Create FAB
    fn create_fab(&self, center: Point, size: f32, elevation: f32) -> Box<dyn Layer> {
        let bounds = Rect::from_xywh(
            center.x - size / 2.0,
            center.y - size / 2.0,
            size,
            size,
        );

        // Content without background (PhysicalModelLayer draws its own background)
        let content = self.create_text_only(bounds, "+", Color::WHITE);

        let physical_model = PhysicalModelLayer::new(content)
            .with_elevation(elevation)
            .with_color(Color::rgb(98, 0, 238))
            .with_shape(PhysicalShape::Circle);

        Box::new(physical_model)
    }

    /// Create App Bar
    fn create_app_bar(&self, bounds: Rect, elevation: f32) -> Box<dyn Layer> {
        let content = self.create_label_surface(
            bounds,
            Color::rgb(98, 0, 238),
            "App Bar",
            Color::WHITE,
        );

        let physical_model = PhysicalModelLayer::new(content)
            .with_elevation(elevation)
            .with_color(Color::rgb(98, 0, 238))
            .with_shape(PhysicalShape::Rectangle)
            .with_border_radius(BorderRadius::circular(4.0));

        Box::new(physical_model)
    }

    /// Create Content Card
    fn create_content_card(&self, bounds: Rect, elevation: f32) -> Box<dyn Layer> {
        let content = self.create_multi_line_surface(
            bounds,
            Color::WHITE,
            vec![
                "Content Card",
                "",
                "This card demonstrates",
                "Material Design elevation",
                "with rounded corners.",
            ],
            Color::rgb(33, 33, 33),
        );

        let physical_model = PhysicalModelLayer::new(content)
            .with_elevation(elevation)
            .with_color(Color::WHITE)
            .with_shape(PhysicalShape::Rectangle)
            .with_border_radius(BorderRadius::circular(12.0));

        Box::new(physical_model)
    }

    /// Create Bottom Sheet
    fn create_bottom_sheet(&self, bounds: Rect, elevation: f32) -> Box<dyn Layer> {
        let content = self.create_multi_line_surface(
            bounds,
            Color::WHITE,
            vec![
                "Bottom Sheet",
                "",
                "Elevation: 6dp",
                "",
                "Used for modal",
                "content and menus.",
            ],
            Color::rgb(33, 33, 33),
        );

        let physical_model = PhysicalModelLayer::new(content)
            .with_elevation(elevation)
            .with_color(Color::WHITE)
            .with_shape(PhysicalShape::Rectangle)
            .with_border_radius(BorderRadius::circular(16.0));

        Box::new(physical_model)
    }

    /// Create text without background
    fn create_text_only(&self, bounds: Rect, text: &str, text_color: Color) -> Box<dyn Layer> {
        let mut picture = PictureLayer::new();

        // Text only (no background)
        let text_paint = Paint {
            color: [
                text_color.r as f32 / 255.0,
                text_color.g as f32 / 255.0,
                text_color.b as f32 / 255.0,
                text_color.a as f32 / 255.0,
            ],
            stroke_width: 0.0,
            anti_alias: true,
        };

        // Center text simulation
        let center_x = bounds.left() + bounds.width() / 2.0;
        let center_y = bounds.top() + bounds.height() / 2.0;

        // Draw "+" as two thick bars
        if text == "+" {
            let size = bounds.width().min(bounds.height()) * 0.5;
            let thickness = size * 0.2;

            // Horizontal bar
            picture.add_command(DrawCommand::Rect {
                rect: Rect::from_xywh(center_x - size / 2.0, center_y - thickness / 2.0, size, thickness),
                paint: text_paint.clone(),
            });

            // Vertical bar
            picture.add_command(DrawCommand::Rect {
                rect: Rect::from_xywh(center_x - thickness / 2.0, center_y - size / 2.0, thickness, size),
                paint: text_paint.clone(),
            });
        } else {
            // Draw text as centered lines (for other text)
            for (i, line) in text.lines().enumerate() {
                let line_width = line.len() as f32 * 6.0;
                let line_y = center_y - 10.0 + i as f32 * 20.0;
                let line_x = center_x - line_width / 2.0;

                let text_rect = Rect::from_xywh(line_x, line_y, line_width, 2.0);
                picture.add_command(DrawCommand::Rect {
                    rect: text_rect,
                    paint: text_paint.clone(),
                });
            }
        }

        Box::new(picture)
    }

    /// Create a simple label surface
    fn create_label_surface(&self, bounds: Rect, bg_color: Color, text: &str, text_color: Color) -> Box<dyn Layer> {
        let mut picture = PictureLayer::new();

        // Background
        let bg_paint = Paint {
            color: [
                bg_color.r as f32 / 255.0,
                bg_color.g as f32 / 255.0,
                bg_color.b as f32 / 255.0,
                bg_color.a as f32 / 255.0,
            ],
            stroke_width: 0.0,
            anti_alias: true,
        };
        picture.add_command(DrawCommand::Rect {
            rect: bounds,
            paint: bg_paint,
        });

        // Text (simulated with rectangles for this demo)
        let text_paint = Paint {
            color: [
                text_color.r as f32 / 255.0,
                text_color.g as f32 / 255.0,
                text_color.b as f32 / 255.0,
                text_color.a as f32 / 255.0,
            ],
            stroke_width: 1.0,
            anti_alias: true,
        };

        // Center text simulation
        let center_x = bounds.left() + bounds.width() / 2.0;
        let center_y = bounds.top() + bounds.height() / 2.0;

        // Draw text as centered lines
        for (i, line) in text.lines().enumerate() {
            let line_width = line.len() as f32 * 6.0;
            let line_y = center_y - 10.0 + i as f32 * 20.0;
            let line_x = center_x - line_width / 2.0;

            let text_rect = Rect::from_xywh(line_x, line_y, line_width, 2.0);
            picture.add_command(DrawCommand::Rect {
                rect: text_rect,
                paint: text_paint.clone(),
            });
        }

        Box::new(picture)
    }

    /// Create a multi-line surface
    fn create_multi_line_surface(
        &self,
        bounds: Rect,
        bg_color: Color,
        lines: Vec<&str>,
        text_color: Color,
    ) -> Box<dyn Layer> {
        let mut picture = PictureLayer::new();

        // Background
        let bg_paint = Paint {
            color: [
                bg_color.r as f32 / 255.0,
                bg_color.g as f32 / 255.0,
                bg_color.b as f32 / 255.0,
                bg_color.a as f32 / 255.0,
            ],
            stroke_width: 0.0,
            anti_alias: true,
        };
        picture.add_command(DrawCommand::Rect {
            rect: bounds,
            paint: bg_paint,
        });

        // Text lines
        let text_paint = Paint {
            color: [
                text_color.r as f32 / 255.0,
                text_color.g as f32 / 255.0,
                text_color.b as f32 / 255.0,
                text_color.a as f32 / 255.0,
            ],
            stroke_width: 1.0,
            anti_alias: true,
        };

        let start_y = bounds.top() + 20.0;
        for (i, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                let line_width = line.len() as f32 * 6.0;
                let line_y = start_y + i as f32 * 20.0;
                let line_x = bounds.left() + 15.0;

                let text_rect = Rect::from_xywh(line_x, line_y, line_width, 2.0);
                picture.add_command(DrawCommand::Rect {
                    rect: text_rect,
                    paint: text_paint.clone(),
                });
            }
        }

        Box::new(picture)
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Material Design Elevation Demo ===");
    println!("Demonstrates Material Design elevation levels with actual shadows");
    println!();

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Material Design Elevation Demo")
        .size(800, 600);

    app.run(MaterialElevationDemo).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

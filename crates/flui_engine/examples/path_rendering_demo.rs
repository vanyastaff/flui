//! Path Rendering Demo
//!
//! Demonstrates PathLayer capabilities:
//! - Various path shapes (star, polygon, curves)
//! - Fill, stroke, and fill+stroke modes
//! - Stroke options (caps, joins, dash patterns)

use flui_engine::{App, AppConfig, AppLogic, Layer, Paint, Painter, PathLayer, StrokeOptions};
use flui_types::{
    geometry::{Offset, Point},
    painting::{
        path::Path,
        StrokeCap, StrokeJoin,
    },
    Event, Rect,
};

struct PathDemo;

impl AppLogic for PathDemo {
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
            "PATH RENDERING DEMO",
            Point::new(250.0, 30.0),
            24.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        // Row 1: Basic shapes with fill
        painter.text("Star Fill:", Point::new(20.0, 80.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_star_filled(painter, Offset::new(100.0, 100.0));

        painter.text("Hexagon:", Point::new(170.0, 80.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_polygon_filled(painter, Offset::new(250.0, 100.0));

        painter.text("Circle:", Point::new(320.0, 80.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_circle_filled(painter, Offset::new(400.0, 100.0));

        painter.text("Wave:", Point::new(470.0, 80.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_wave_filled(painter, Offset::new(550.0, 100.0));

        // Row 2: Stroke styles
        painter.text("Stroke Caps:", Point::new(20.0, 230.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_stroke_caps(painter, Offset::new(100.0, 250.0));

        painter.text("Stroke Joins:", Point::new(170.0, 230.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_stroke_joins(painter, Offset::new(250.0, 250.0));

        painter.text("Dashed:", Point::new(320.0, 230.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_dashed_path(painter, Offset::new(400.0, 250.0));

        // Row 3: Combined fill + stroke
        painter.text("Star Outlined:", Point::new(20.0, 380.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_star_outlined(painter, Offset::new(100.0, 400.0));

        painter.text("Gear:", Point::new(170.0, 380.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_gear(painter, Offset::new(250.0, 400.0));

        painter.text("Heart:", Point::new(320.0, 380.0), 12.0, &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() });
        self.draw_heart(painter, Offset::new(400.0, 400.0));
    }
}

impl PathDemo {
    /// Draw a 5-point star with fill
    fn draw_star_filled(&self, painter: &mut dyn Painter, offset: Offset) {
        let mut path = Path::new();

        let center = Point::new(50.0, 50.0);
        let outer_radius = 40.0;
        let inner_radius = 16.0;

        for i in 0..10 {
            let angle = std::f32::consts::PI * 2.0 * (i as f32) / 10.0 - std::f32::consts::PI / 2.0;
            let radius = if i % 2 == 0 { outer_radius } else { inner_radius };
            let x = center.x + radius * angle.cos();
            let y = center.y + radius * angle.sin();

            if i == 0 {
                path.move_to(Point::new(x, y));
            } else {
                path.line_to(Point::new(x, y));
            }
        }
        path.close();

        let layer = PathLayer::new(path)
            .with_paint(Paint {
                color: [1.0, 0.8, 0.0, 1.0], // Gold
                stroke_width: 0.0,
                anti_alias: true,
            });

        painter.save();
        painter.translate(offset);
        Layer::paint(&layer, painter);
        painter.restore();
    }

    /// Draw a hexagon with fill
    fn draw_polygon_filled(&self, painter: &mut dyn Painter, offset: Offset) {
        let mut path = Path::new();

        let center = Point::new(50.0, 50.0);
        let radius = 40.0;

        for i in 0..6 {
            let angle = std::f32::consts::PI * 2.0 * (i as f32) / 6.0 - std::f32::consts::PI / 2.0;
            let x = center.x + radius * angle.cos();
            let y = center.y + radius * angle.sin();

            if i == 0 {
                path.move_to(Point::new(x, y));
            } else {
                path.line_to(Point::new(x, y));
            }
        }
        path.close();

        let layer = PathLayer::new(path)
            .with_paint(Paint {
                color: [0.2, 0.6, 1.0, 1.0], // Blue
                stroke_width: 0.0,
                anti_alias: true,
            });

        painter.save();
        painter.translate(offset);
        Layer::paint(&layer, painter);
        painter.restore();
    }

    /// Draw a circle using path
    fn draw_circle_filled(&self, painter: &mut dyn Painter, offset: Offset) {
        let mut path = Path::new();
        path.add_circle(Point::new(50.0, 50.0), 40.0);

        let layer = PathLayer::new(path)
            .with_paint(Paint {
                color: [0.5, 0.2, 0.8, 1.0], // Purple
                stroke_width: 0.0,
                anti_alias: true,
            });

        painter.save();
        painter.translate(offset);
        Layer::paint(&layer, painter);
        painter.restore();
    }

    /// Draw a wave shape
    fn draw_wave_filled(&self, painter: &mut dyn Painter, offset: Offset) {
        let mut path = Path::new();

        // Create a smooth wavy shape using quadratic bezier curves
        path.move_to(Point::new(20.0, 50.0));
        path.quadratic_to(
            Point::new(35.0, 20.0),
            Point::new(50.0, 50.0),
        );
        path.quadratic_to(
            Point::new(65.0, 80.0),
            Point::new(80.0, 50.0),
        );
        path.line_to(Point::new(80.0, 80.0));
        path.line_to(Point::new(20.0, 80.0));
        path.close();

        let layer = PathLayer::new(path)
            .with_paint(Paint {
                color: [1.0, 0.4, 0.4, 1.0], // Coral
                stroke_width: 0.0,
                anti_alias: true,
            });

        painter.save();
        painter.translate(offset);
        Layer::paint(&layer, painter);
        painter.restore();
    }

    /// Demonstrate different stroke caps
    fn draw_stroke_caps(&self, painter: &mut dyn Painter, offset: Offset) {
        let caps = [
            (StrokeCap::Butt, 20.0),
            (StrokeCap::Round, 50.0),
            (StrokeCap::Square, 80.0),
        ];

        painter.save();
        painter.translate(offset);

        for (cap, y) in caps {
            let mut path = Path::new();
            path.move_to(Point::new(20.0, y));
            path.line_to(Point::new(80.0, y));

            let layer = PathLayer::new(path)
                .with_stroke(
                    StrokeOptions::new()
                        .with_width(8.0)
                        .with_cap(cap),
                )
                .with_paint(Paint {
                    color: [0.2, 0.2, 0.2, 1.0],
                    stroke_width: 8.0,
                    anti_alias: true,
                });

            Layer::paint(&layer, painter);
        }

        painter.restore();
    }

    /// Demonstrate different stroke joins
    fn draw_stroke_joins(&self, painter: &mut dyn Painter, offset: Offset) {
        let joins = [
            (StrokeJoin::Miter, 0.0),
            (StrokeJoin::Round, 35.0),
            (StrokeJoin::Bevel, 70.0),
        ];

        painter.save();
        painter.translate(offset);

        for (join, y_offset) in joins {
            let mut path = Path::new();
            path.move_to(Point::new(20.0, 20.0 + y_offset));
            path.line_to(Point::new(50.0, 10.0 + y_offset));
            path.line_to(Point::new(80.0, 20.0 + y_offset));

            let layer = PathLayer::new(path)
                .with_stroke(
                    StrokeOptions::new()
                        .with_width(6.0)
                        .with_join(join),
                )
                .with_paint(Paint {
                    color: [0.2, 0.2, 0.2, 1.0],
                    stroke_width: 6.0,
                    anti_alias: true,
                });

            Layer::paint(&layer, painter);
        }

        painter.restore();
    }

    /// Demonstrate dashed stroke
    fn draw_dashed_path(&self, painter: &mut dyn Painter, offset: Offset) {
        let dash_patterns = [
            vec![10.0, 5.0],
            vec![5.0, 5.0],
            vec![15.0, 5.0, 5.0, 5.0],
        ];

        painter.save();
        painter.translate(offset);

        for (i, pattern) in dash_patterns.iter().enumerate() {
            let mut path = Path::new();
            path.move_to(Point::new(20.0, 30.0 + i as f32 * 30.0));
            path.line_to(Point::new(80.0, 30.0 + i as f32 * 30.0));

            let layer = PathLayer::new(path)
                .with_stroke(
                    StrokeOptions::new()
                        .with_width(3.0)
                        .with_dash_pattern(pattern.clone()),
                )
                .with_paint(Paint {
                    color: [0.2, 0.2, 0.2, 1.0],
                    stroke_width: 3.0,
                    anti_alias: true,
                });

            Layer::paint(&layer, painter);
        }

        painter.restore();
    }

    /// Draw a star with both fill and stroke
    fn draw_star_outlined(&self, painter: &mut dyn Painter, offset: Offset) {
        let mut path = Path::new();

        let center = Point::new(50.0, 50.0);
        let outer_radius = 40.0;
        let inner_radius = 16.0;

        for i in 0..10 {
            let angle = std::f32::consts::PI * 2.0 * (i as f32) / 10.0 - std::f32::consts::PI / 2.0;
            let radius = if i % 2 == 0 { outer_radius } else { inner_radius };
            let x = center.x + radius * angle.cos();
            let y = center.y + radius * angle.sin();

            if i == 0 {
                path.move_to(Point::new(x, y));
            } else {
                path.line_to(Point::new(x, y));
            }
        }
        path.close();

        let layer = PathLayer::new(path)
            .with_paint(Paint {
                color: [1.0, 1.0, 0.0, 1.0], // Yellow fill
                stroke_width: 0.0,
                anti_alias: true,
            })
            .with_stroke_paint(Paint {
                color: [1.0, 0.5, 0.0, 1.0], // Orange stroke
                stroke_width: 3.0,
                anti_alias: true,
            });

        painter.save();
        painter.translate(offset);
        Layer::paint(&layer, painter);
        painter.restore();
    }

    /// Draw a gear shape
    fn draw_gear(&self, painter: &mut dyn Painter, offset: Offset) {
        let mut path = Path::new();

        // Create a gear-like shape
        let center = Point::new(50.0, 50.0);
        let teeth = 8;
        let outer_radius = 40.0;
        let inner_radius = 28.0;

        for i in 0..(teeth * 2) {
            let angle = std::f32::consts::PI * 2.0 * (i as f32) / (teeth * 2) as f32;
            let radius = if i % 2 == 0 { outer_radius } else { inner_radius };
            let x = center.x + radius * angle.cos();
            let y = center.y + radius * angle.sin();

            if i == 0 {
                path.move_to(Point::new(x, y));
            } else {
                path.line_to(Point::new(x, y));
            }
        }
        path.close();

        // Add inner circle
        path.add_circle(center, 15.0);

        let layer = PathLayer::new(path)
            .with_paint(Paint {
                color: [0.3, 0.3, 0.3, 1.0], // Dark gray fill
                stroke_width: 0.0,
                anti_alias: true,
            })
            .with_stroke_paint(Paint {
                color: [0.0, 0.0, 0.0, 1.0], // Black stroke
                stroke_width: 2.0,
                anti_alias: true,
            });

        painter.save();
        painter.translate(offset);
        Layer::paint(&layer, painter);
        painter.restore();
    }

    /// Draw a heart shape
    fn draw_heart(&self, painter: &mut dyn Painter, offset: Offset) {
        let mut path = Path::new();

        // Heart shape using cubic bezier curves
        path.move_to(Point::new(50.0, 80.0));

        // Left side
        path.cubic_to(
            Point::new(20.0, 60.0),
            Point::new(20.0, 30.0),
            Point::new(35.0, 25.0),
        );
        path.cubic_to(
            Point::new(45.0, 20.0),
            Point::new(50.0, 30.0),
            Point::new(50.0, 30.0),
        );

        // Right side
        path.cubic_to(
            Point::new(50.0, 30.0),
            Point::new(55.0, 20.0),
            Point::new(65.0, 25.0),
        );
        path.cubic_to(
            Point::new(80.0, 30.0),
            Point::new(80.0, 60.0),
            Point::new(50.0, 80.0),
        );

        path.close();

        let layer = PathLayer::new(path)
            .with_paint(Paint {
                color: [1.0, 0.2, 0.4, 1.0], // Pink fill
                stroke_width: 0.0,
                anti_alias: true,
            })
            .with_stroke_paint(Paint {
                color: [0.8, 0.0, 0.2, 1.0], // Dark red stroke
                stroke_width: 2.0,
                anti_alias: true,
            });

        painter.save();
        painter.translate(offset);
        Layer::paint(&layer, painter);
        painter.restore();
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Path Rendering Demo ===");
    println!("Demonstrates PathLayer with various shapes and stroke options");
    println!();

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Path Rendering Demo")
        .size(800, 600);

    app.run(PathDemo).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

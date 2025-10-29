//! Alignment Test Demo
//!
//! Simple test to verify that alignment calculations work correctly.
//! Shows all 9 standard alignments with clear visual positioning.

use flui_engine::{App, AppConfig, AppLogic, Painter, Paint};
use flui_types::{Event, Rect, Point, Alignment, Size};

struct AlignmentTestDemo;

impl AlignmentTestDemo {
    fn new() -> Self {
        Self
    }

    /// Draw a single alignment example with correct calculation
    fn draw_alignment(
        &self,
        painter: &mut dyn Painter,
        x: f32,
        y: f32,
        label: &str,
        alignment: Alignment,
    ) {
        let container_size = Size::new(120.0, 100.0);
        let child_size = Size::new(40.0, 30.0);

        // Draw label
        painter.text(
            label,
            Point::new(x, y),
            12.0,
            &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
        );

        // Draw container background
        painter.rect(
            Rect::from_xywh(x, y + 20.0, container_size.width, container_size.height),
            &Paint {
                color: [0.9, 0.9, 0.9, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // Draw container border
        painter.rect(
            Rect::from_xywh(x, y + 20.0, container_size.width, container_size.height),
            &Paint {
                color: [0.6, 0.6, 0.6, 1.0],
                stroke_width: 2.0,
                anti_alias: true,
            },
        );

        // Calculate aligned child offset using correct formula
        let offset = alignment.calculate_offset(child_size, container_size);

        // Draw aligned child
        painter.rect(
            Rect::from_xywh(
                x + offset.dx,
                y + 20.0 + offset.dy,
                child_size.width,
                child_size.height
            ),
            &Paint {
                color: [0.8, 0.2, 0.2, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // Draw crosshair at calculated position for debugging
        let child_center_x = x + offset.dx + child_size.width / 2.0;
        let child_center_y = y + 20.0 + offset.dy + child_size.height / 2.0;

        painter.line(
            Point::new(child_center_x - 5.0, child_center_y),
            Point::new(child_center_x + 5.0, child_center_y),
            &Paint {
                color: [0.0, 0.0, 0.0, 0.3],
                stroke_width: 1.0,
                anti_alias: true,
            },
        );

        painter.line(
            Point::new(child_center_x, child_center_y - 5.0),
            Point::new(child_center_x, child_center_y + 5.0),
            &Paint {
                color: [0.0, 0.0, 0.0, 0.3],
                stroke_width: 1.0,
                anti_alias: true,
            },
        );
    }
}

impl AppLogic for AlignmentTestDemo {
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
            Rect::from_xywh(0.0, 0.0, 1200.0, 600.0),
            &Paint {
                color: [0.96, 0.96, 0.96, 1.0],
                stroke_width: 0.0,
                anti_alias: true,
            },
        );

        // Title
        painter.text(
            "Alignment Calculation Test",
            Point::new(400.0, 30.0),
            24.0,
            &Paint { color: [0.2, 0.3, 0.4, 1.0], ..Default::default() }
        );

        painter.text(
            "Red boxes should be positioned according to their alignment",
            Point::new(350.0, 60.0),
            14.0,
            &Paint { color: [0.5, 0.5, 0.5, 1.0], ..Default::default() }
        );

        // Row 1: Top alignments
        self.draw_alignment(painter, 50.0, 100.0, "TOP_LEFT", Alignment::TOP_LEFT);
        self.draw_alignment(painter, 220.0, 100.0, "TOP_CENTER", Alignment::TOP_CENTER);
        self.draw_alignment(painter, 390.0, 100.0, "TOP_RIGHT", Alignment::TOP_RIGHT);

        // Row 2: Center alignments
        self.draw_alignment(painter, 50.0, 260.0, "CENTER_LEFT", Alignment::CENTER_LEFT);
        self.draw_alignment(painter, 220.0, 260.0, "CENTER", Alignment::CENTER);
        self.draw_alignment(painter, 390.0, 260.0, "CENTER_RIGHT", Alignment::CENTER_RIGHT);

        // Row 3: Bottom alignments
        self.draw_alignment(painter, 50.0, 420.0, "BOTTOM_LEFT", Alignment::BOTTOM_LEFT);
        self.draw_alignment(painter, 220.0, 420.0, "BOTTOM_CENTER", Alignment::BOTTOM_CENTER);
        self.draw_alignment(painter, 390.0, 420.0, "BOTTOM_RIGHT", Alignment::BOTTOM_RIGHT);

        // Show calculation formula
        painter.text(
            "Formula: offset = (container_size - child_size) * (alignment + 1) / 2",
            Point::new(600.0, 200.0),
            14.0,
            &Paint { color: [0.3, 0.3, 0.3, 1.0], ..Default::default() }
        );

        // Example calculations
        let examples = [
            ("TOP_LEFT (-1, -1):", "x = (120-40) * (-1+1) / 2 = 0", "y = (100-30) * (-1+1) / 2 = 0"),
            ("CENTER (0, 0):", "x = (120-40) * (0+1) / 2 = 40", "y = (100-30) * (0+1) / 2 = 35"),
            ("BOTTOM_RIGHT (1, 1):", "x = (120-40) * (1+1) / 2 = 80", "y = (100-30) * (1+1) / 2 = 70"),
        ];

        for (i, (title, calc_x, calc_y)) in examples.iter().enumerate() {
            let y_base = 250.0 + i as f32 * 80.0;

            painter.text(
                title,
                Point::new(600.0, y_base),
                12.0,
                &Paint { color: [0.0, 0.0, 0.0, 1.0], ..Default::default() }
            );

            painter.text(
                calc_x,
                Point::new(620.0, y_base + 20.0),
                11.0,
                &Paint { color: [0.4, 0.4, 0.4, 1.0], ..Default::default() }
            );

            painter.text(
                calc_y,
                Point::new(620.0, y_base + 40.0),
                11.0,
                &Paint { color: [0.4, 0.4, 0.4, 1.0], ..Default::default() }
            );
        }
    }
}

#[cfg(feature = "egui")]
fn main() {
    println!("=== Alignment Calculation Test ===");
    println!();
    println!("Testing alignment.calculate_offset() formula:");
    println!("  offset.x = (container_width - child_width) * (alignment.x + 1.0) / 2.0");
    println!("  offset.y = (container_height - child_height) * (alignment.y + 1.0) / 2.0");
    println!();
    println!("Verifying all 9 standard alignments:");
    println!("  TOP_LEFT, TOP_CENTER, TOP_RIGHT");
    println!("  CENTER_LEFT, CENTER, CENTER_RIGHT");
    println!("  BOTTOM_LEFT, BOTTOM_CENTER, BOTTOM_RIGHT");
    println!();

    let app = App::with_config(AppConfig::new().backend(flui_engine::Backend::Egui))
        .title("Alignment Test")
        .size(1200, 600);

    app.run(AlignmentTestDemo::new()).expect("Failed to run app");
}

#[cfg(not(feature = "egui"))]
fn main() {
    panic!("This example requires the 'egui' feature");
}

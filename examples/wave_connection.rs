//! Wave Connection Example - Drawing smooth curves for workflow connections
//!
//! Demonstrates:
//! - Drawing smooth cubic Bézier curves
//! - Creating wave-like connections between points
//! - Path API with custom shapes
//! - Useful for workflow/node graph editors

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::painting::Path;
use flui_types::styling::{BorderRadius, BoxDecoration};
use flui_types::{Color, Offset, Point, Size};
use flui_widgets::{Center, Container, CustomPaint, Scaffold};
use std::f32::consts::PI;

// Re-export from flui_painting for use in callbacks
use flui_painting::{Canvas, Paint};

/// Application demonstrating wave connections
#[derive(Debug, Clone)]
struct WaveConnectionApp;

impl View for WaveConnectionApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Scaffold::builder()
            .background_color(Color::rgb(30, 30, 35))
            .body(
                Center::builder()
                    .child(
                        Container::builder()
                            .width(800.0)
                            .height(600.0)
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(45, 45, 50)),
                                // Removed border_radius to prevent clipping of nodes at edges
                                ..Default::default()
                            })
                            .child(
                                CustomPaint::builder()
                                    .painter(|canvas, size, offset| {
                                        draw_wave_demo(canvas, size, offset);
                                    })
                                    .size(Size::new(800.0, 600.0))
                                    .build(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .build()
    }
}

/// Helper function to create smooth wave curve between two points
fn create_wave_path(start: Point, end: Point) -> Path {
    let mut path = Path::new();

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let distance = (dx * dx + dy * dy).sqrt();

    // Calculate control points for smooth S-curve
    let control_distance = distance * 0.4;

    // For horizontal connections (typical in workflow editors)
    if dx.abs() > dy.abs() {
        let mid_x = (start.x + end.x) / 2.0;

        path.move_to(start);

        // First half - curve out from start
        let cp1 = Point::new(start.x + control_distance, start.y);
        let cp2 = Point::new(mid_x - control_distance * 0.3, start.y + dy * 0.5);
        let mid = Point::new(mid_x, (start.y + end.y) / 2.0);

        path.cubic_to(cp1, cp2, mid);

        // Second half - curve into end
        let cp3 = Point::new(mid_x + control_distance * 0.3, end.y - dy * 0.5);
        let cp4 = Point::new(end.x - control_distance, end.y);

        path.cubic_to(cp3, cp4, end);
    } else {
        // Vertical connections
        let mid_y = (start.y + end.y) / 2.0;

        path.move_to(start);

        let cp1 = Point::new(start.x, start.y + control_distance);
        let cp2 = Point::new(start.x + dx * 0.5, mid_y - control_distance * 0.3);
        let mid = Point::new((start.x + end.x) / 2.0, mid_y);

        path.cubic_to(cp1, cp2, mid);

        let cp3 = Point::new(end.x - dx * 0.5, mid_y + control_distance * 0.3);
        let cp4 = Point::new(end.x, end.y - control_distance);

        path.cubic_to(cp3, cp4, end);
    }

    path
}

/// Helper function to create sine wave path
fn create_sine_wave(start: Point, end: Point, amplitude: f32, frequency: f32) -> Path {
    let mut path = Path::new();

    let dx = end.x - start.x;
    let dy = end.y - start.y;

    path.move_to(start);

    // Create smooth sine wave using multiple cubic Bézier curves
    let segments = 20;
    let segment_length = 1.0 / segments as f32;

    for i in 0..segments {
        let t = i as f32 * segment_length;
        let next_t = (i + 1) as f32 * segment_length;

        // Next point
        let x2 = start.x + dx * next_t;
        let wave2 = (next_t * frequency * 2.0 * PI).sin() * amplitude;
        let y2 = start.y + dy * next_t + wave2;

        // Control points for smooth curve
        let cp1_x = start.x + dx * (t + segment_length * 0.33);
        let cp1_wave = ((t + segment_length * 0.33) * frequency * 2.0 * PI).sin() * amplitude;
        let cp1 = Point::new(cp1_x, start.y + dy * (t + segment_length * 0.33) + cp1_wave);

        let cp2_x = start.x + dx * (t + segment_length * 0.67);
        let cp2_wave = ((t + segment_length * 0.67) * frequency * 2.0 * PI).sin() * amplitude;
        let cp2 = Point::new(cp2_x, start.y + dy * (t + segment_length * 0.67) + cp2_wave);

        let end_point = Point::new(x2, y2);

        path.cubic_to(cp1, cp2, end_point);
    }

    path
}

/// Main drawing function
fn draw_wave_demo(canvas: &mut Canvas, _size: Size, offset: Offset) {
    // Draw background grid - manually apply offset to coordinates
    let grid_paint = Paint::stroke(Color::rgba(255, 255, 255, 20), 1.0);
    for i in 0..40 {
        let x = offset.dx + i as f32 * 20.0;
        canvas.draw_line(
            Point::new(x, offset.dy),
            Point::new(x, offset.dy + 600.0),
            &grid_paint,
        );
    }
    for i in 0..30 {
        let y = offset.dy + i as f32 * 20.0;
        canvas.draw_line(
            Point::new(offset.dx, y),
            Point::new(offset.dx + 800.0, y),
            &grid_paint,
        );
    }

    // Define node positions and colors
    // Added 40px margin from edges to prevent clipping (node radius is 27px)
    let nodes = vec![
        (Point::new(140.0, 150.0), Color::rgb(66, 165, 245)), // Blue
        (Point::new(400.0, 140.0), Color::rgb(102, 187, 106)), // Green
        (Point::new(400.0, 300.0), Color::rgb(255, 167, 38)), // Orange
        (Point::new(660.0, 200.0), Color::rgb(171, 71, 188)), // Purple
    ];

    // Draw connections with different styles
    let connections = vec![
        (0, 1), // Node 1 -> 2
        (0, 2), // Node 1 -> 3
        (1, 3), // Node 2 -> 4
        (2, 3), // Node 3 -> 4
    ];

    // Draw connection paths
    for (start_idx, end_idx) in connections {
        let start = Point::new(
            offset.dx + nodes[start_idx].0.x + 30.0,
            offset.dy + nodes[start_idx].0.y,
        );
        let end = Point::new(
            offset.dx + nodes[end_idx].0.x - 30.0,
            offset.dy + nodes[end_idx].0.y,
        );

        // DEBUG: Draw simple straight line first to test
        let color = nodes[start_idx].1;
        let debug_paint = Paint::stroke(color, 5.0);
        canvas.draw_line(start, end, &debug_paint);

        // Original wave path code (commented for now)
        // let path = create_wave_path(start, end);
        // let glow_paint = Paint::stroke(Color::rgba(color.r, color.g, color.b, 40), 8.0);
        // canvas.draw_path(&path, &glow_paint);
        // let line_paint = Paint::stroke(color, 3.0);
        // canvas.draw_path(&path, &line_paint);
    }

    // Example of sine wave for decoration (commented for debugging)
    // let sine_start = Point::new(offset.dx + 100.0, offset.dy + 450.0);
    // let sine_end = Point::new(offset.dx + 700.0, offset.dy + 450.0);
    // let sine_path = create_sine_wave(sine_start, sine_end, 20.0, 3.0);
    // let sine_glow = Paint::stroke(Color::rgba(255, 64, 129, 60), 6.0);
    // canvas.draw_path(&sine_path, &sine_glow);
    // let sine_line = Paint::stroke(Color::rgb(255, 64, 129), 2.0);
    // canvas.draw_path(&sine_path, &sine_line);

    // Draw nodes (circles)
    for (pos, color) in &nodes {
        let center = Point::new(offset.dx + pos.x, offset.dy + pos.y);

        // Outer ring
        let ring_paint = Paint::stroke(*color, 3.0);
        canvas.draw_circle(center, 27.0, &ring_paint);

        // Inner fill
        let fill_paint = Paint::fill(Color::rgb(45, 45, 50));
        canvas.draw_circle(center, 25.0, &fill_paint);

        // Center dot
        let dot_paint = Paint::fill(*color);
        canvas.draw_circle(center, 8.0, &dot_paint);
    }

    // Draw title
    let title_pos = Offset::new(offset.dx + 30.0, offset.dy + 30.0);
    let title_style = flui_types::typography::TextStyle {
        font_size: Some(24.0),
        color: Some(Color::WHITE),
        ..Default::default()
    };
    let title_paint = Paint::fill(Color::WHITE);
    canvas.draw_text(
        "Wave Connections Demo",
        title_pos,
        &title_style,
        &title_paint,
    );

    // Draw description
    let desc_pos = Offset::new(offset.dx + 30.0, offset.dy + 60.0);
    let desc_style = flui_types::typography::TextStyle {
        font_size: Some(14.0),
        color: Some(Color::rgba(255, 255, 255, 180)),
        ..Default::default()
    };
    let desc_paint = Paint::fill(Color::rgba(255, 255, 255, 180));
    canvas.draw_text(
        "Smooth Bézier curves for workflow/node connections",
        desc_pos,
        &desc_style,
        &desc_paint,
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Wave Connection Demo ===");
    run_app(Box::new(WaveConnectionApp))
}

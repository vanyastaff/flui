//! Simple tessellation example - demonstrates Canvas → Path → Tessellation
//!
//! This example shows the complete flow from high-level Canvas API
//! to low-level GPU-ready triangle meshes.
//!
//! Usage:
//! ```bash
//! cargo run --example simple_tessellation -p flui_painting --features tessellation
//! ```

use flui_painting::prelude::*;
use flui_painting::tessellation::{tessellate_fill, tessellate_stroke, TessellationOptions};
use flui_types::geometry::{px, Point, Rect};
use flui_types::painting::Path;
use flui_types::styling::Color;

fn main() {
    println!("===========================================");
    println!("  FLUI Painting Tessellation Demo");
    println!("===========================================\n");

    // ========================================================================
    // Example 1: Simple Rectangle
    // ========================================================================
    println!("1. Rectangle Tessellation");
    println!("   ----------------------");

    let rect_path = Path::rectangle(Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)));

    let rect_fill = tessellate_fill(&rect_path, &TessellationOptions::default()).unwrap();
    println!(
        "   Fill:   {} vertices, {} triangles",
        rect_fill.vertices.len(),
        rect_fill.triangle_count()
    );

    let rect_stroke = tessellate_stroke(&rect_path, 2.0, &TessellationOptions::default()).unwrap();
    println!(
        "   Stroke: {} vertices, {} triangles\n",
        rect_stroke.vertices.len(),
        rect_stroke.triangle_count()
    );

    // ========================================================================
    // Example 2: Circle
    // ========================================================================
    println!("2. Circle Tessellation");
    println!("   -------------------");

    let circle_path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);

    let circle_fill = tessellate_fill(&circle_path, &TessellationOptions::default()).unwrap();
    println!(
        "   Fill:   {} vertices, {} triangles",
        circle_fill.vertices.len(),
        circle_fill.triangle_count()
    );

    let circle_stroke =
        tessellate_stroke(&circle_path, 2.0, &TessellationOptions::default()).unwrap();
    println!(
        "   Stroke: {} vertices, {} triangles\n",
        circle_stroke.vertices.len(),
        circle_stroke.triangle_count()
    );

    // ========================================================================
    // Example 3: Canvas Recording → DisplayList
    // ========================================================================
    println!("3. Canvas Recording");
    println!("   ----------------");

    let mut canvas = Canvas::new();

    // Draw multiple shapes
    canvas.draw_rect(
        Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(100.0)),
        &Paint::fill(Color::RED),
    );

    canvas.draw_circle(
        Point::new(px(200.0), px(60.0)),
        px(50.0),
        &Paint::fill(Color::BLUE),
    );

    canvas.draw_rrect(
        flui_types::geometry::RRect::from_rect_xy(
            Rect::from_xywh(px(300.0), px(10.0), px(100.0), px(100.0)),
            px(10.0),
            px(10.0),
        ),
        &Paint::fill(Color::GREEN),
    );

    // Finish recording
    let picture = canvas.finish();
    println!("   Recorded {} draw commands", picture.len());
    println!("   Picture bounds: {:?}\n", picture.bounds());

    // ========================================================================
    // Example 4: Quality Comparison
    // ========================================================================
    println!("4. Quality Comparison (Circle)");
    println!("   ---------------------------");

    let test_path = Path::circle(Point::new(px(50.0), px(50.0)), 25.0);

    let low_quality =
        tessellate_fill(&test_path, &TessellationOptions::with_tolerance(1.0)).unwrap();

    let medium_quality =
        tessellate_fill(&test_path, &TessellationOptions::with_tolerance(0.1)).unwrap();

    let high_quality =
        tessellate_fill(&test_path, &TessellationOptions::with_tolerance(0.01)).unwrap();

    println!(
        "   Low    (tolerance 1.0):  {} triangles",
        low_quality.triangle_count()
    );
    println!(
        "   Medium (tolerance 0.1):  {} triangles",
        medium_quality.triangle_count()
    );
    println!(
        "   High   (tolerance 0.01): {} triangles\n",
        high_quality.triangle_count()
    );

    // ========================================================================
    // Example 5: Complex Path
    // ========================================================================
    println!("5. Complex Path (Polygon)");
    println!("   ----------------------");

    let points = vec![
        Point::new(px(0.0), px(0.0)),
        Point::new(px(100.0), px(0.0)),
        Point::new(px(100.0), px(50.0)),
        Point::new(px(50.0), px(100.0)),
        Point::new(px(0.0), px(50.0)),
    ];

    let polygon = Path::polygon(&points);
    let polygon_fill = tessellate_fill(&polygon, &TessellationOptions::default()).unwrap();

    println!(
        "   Pentagon: {} vertices, {} triangles",
        polygon_fill.vertices.len(),
        polygon_fill.triangle_count()
    );

    // Print first few vertices
    println!("\n   First 5 vertices:");
    for (i, vertex) in polygon_fill.vertices.iter().take(5).enumerate() {
        println!(
            "     {}: ({:.2}, {:.2})",
            i, vertex.position[0], vertex.position[1]
        );
    }

    println!("\n===========================================");
    println!("✓ All examples completed successfully!");
    println!("===========================================");
}

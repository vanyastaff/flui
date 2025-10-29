//! Material Design elevation demo
//!
//! Demonstrates PhysicalModelLayer with different elevation levels,
//! shapes, and Material Design effects.

use flui_engine::layer::{
    PictureLayer, PhysicalModelLayer, ContainerLayer, Layer, BoxedLayer,
    Elevation, PhysicalShape, DrawCommand,
};
use flui_engine::painter::Paint;
use flui_types::{Color, Rect, Point};
use flui_types::styling::BorderRadius;

fn main() {
    println!("Material Design Elevation Demo");
    println!("==============================\n");

    // Create demo with different elevation levels
    let _demo = create_material_demo();

    println!("Created Material Design elevation showcase:");
    println!("- Cards at different elevation levels (1dp, 2dp, 3dp, 4dp, 5dp)");
    println!("- Floating Action Button (FAB) with circle shape");
    println!("- App Bar with elevation");
    println!("- Bottom sheet with elevation");
    println!("\nAll components follow Material Design 3 elevation guidelines.");
    println!("Shadows are automatically calculated based on elevation level.");
    println!("\nNote: This is a layer structure demo.");
    println!("For visual rendering, integrate with a backend like egui.");
}

fn create_material_demo() -> BoxedLayer {
    let mut container = ContainerLayer::new();

    // Title background
    let title = create_surface(
        Rect::from_xywh(20.0, 20.0, 760.0, 80.0),
        Color::rgb(98, 0, 238),
        "Material Design Elevation Showcase",
        Color::WHITE,
    );
    container.add_child(title);

    // Elevation level showcase
    let elevations = [
        (Elevation::LEVEL_1, "Level 1\n1dp"),
        (Elevation::LEVEL_2, "Level 2\n3dp"),
        (Elevation::LEVEL_3, "Level 3\n6dp"),
        (Elevation::LEVEL_4, "Level 4\n8dp"),
        (Elevation::LEVEL_5, "Level 5\n12dp"),
    ];

    for (i, (elevation, label)) in elevations.iter().enumerate() {
        let x = 40.0 + i as f32 * 150.0;
        let y = 140.0;

        let card = create_elevated_card(
            Rect::from_xywh(x, y, 130.0, 100.0),
            *elevation,
            label,
        );
        container.add_child(card);
    }

    // Material Design components showcase

    // FAB (Floating Action Button) - Circle with high elevation
    let fab = create_fab(
        Point::new(700.0, 500.0),
        56.0,
        Elevation::LEVEL_3,
    );
    container.add_child(fab);

    // App Bar - Rectangle with moderate elevation
    let app_bar = create_app_bar(
        Rect::from_xywh(40.0, 280.0, 400.0, 64.0),
        Elevation::LEVEL_4,
    );
    container.add_child(app_bar);

    // Card with content - Rectangle with low elevation
    let content_card = create_content_card(
        Rect::from_xywh(40.0, 370.0, 400.0, 180.0),
        Elevation::LEVEL_1,
    );
    container.add_child(content_card);

    // Bottom sheet - Rectangle with medium elevation
    let bottom_sheet = create_bottom_sheet(
        Rect::from_xywh(480.0, 280.0, 300.0, 270.0),
        Elevation::LEVEL_3,
    );
    container.add_child(bottom_sheet);

    Box::new(container)
}

fn create_elevated_card(bounds: Rect, elevation: f32, label: &str) -> BoxedLayer {
    let content = create_label_surface(bounds, Color::WHITE, label, Color::rgb(33, 33, 33));

    let physical_model = PhysicalModelLayer::new(content)
        .with_elevation(elevation)
        .with_color(Color::WHITE)
        .with_shape(PhysicalShape::Rectangle)
        .with_border_radius(BorderRadius::circular(8.0));

    Box::new(physical_model)
}

fn create_fab(center: Point, size: f32, elevation: f32) -> BoxedLayer {
    let bounds = Rect::from_xywh(
        center.x - size / 2.0,
        center.y - size / 2.0,
        size,
        size,
    );

    let content = create_label_surface(bounds, Color::rgb(98, 0, 238), "+", Color::WHITE);

    let physical_model = PhysicalModelLayer::new(content)
        .with_elevation(elevation)
        .with_color(Color::rgb(98, 0, 238))
        .with_shape(PhysicalShape::Circle);

    Box::new(physical_model)
}

fn create_app_bar(bounds: Rect, elevation: f32) -> BoxedLayer {
    let content = create_label_surface(
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

fn create_content_card(bounds: Rect, elevation: f32) -> BoxedLayer {
    let content = create_multi_line_surface(
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

fn create_bottom_sheet(bounds: Rect, elevation: f32) -> BoxedLayer {
    let content = create_multi_line_surface(
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

fn create_surface(bounds: Rect, bg_color: Color, text: &str, text_color: Color) -> BoxedLayer {
    create_label_surface(bounds, bg_color, text, text_color)
}

fn create_label_surface(bounds: Rect, bg_color: Color, text: &str, text_color: Color) -> BoxedLayer {
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

fn create_multi_line_surface(
    bounds: Rect,
    bg_color: Color,
    lines: Vec<&str>,
    text_color: Color,
) -> BoxedLayer {
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

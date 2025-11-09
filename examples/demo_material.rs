//! Material widget demo - testing Material Design elevation and shadows
//!
//! This demo tests the Material widget with:
//! - Different elevation levels
//! - Different shapes (rounded corners)
//! - Different colors
//! - Custom shadow colors

use flui::prelude::*;
use flui_app::{launch, EguiAppBuilder};
use flui_types::styling::BorderRadius;
use flui_widgets::{Container, Material, SizedBox, Text};

fn main() {
    println!("=== Material Widget Demo ===");
    println!("Testing Material widget with various configurations:");
    println!("1. Elevation levels: 0, 2, 4, 8, 16");
    println!("2. Different border radii");
    println!("3. Different colors");
    println!("4. Custom shadow colors");

    launch(EguiAppBuilder::new(app_view));
}

fn app_view() -> impl View {
    Column::new().children(vec![
        // Test 1: Elevation 0 (flat)
        Box::new(
            Container::builder()
                .height(100.0)
                .padding(EdgeInsets::all(10.0))
                .child(
                    Material::builder()
                        .elevation(0.0)
                        .color(Color::rgb(255, 255, 255))
                        .child(
                            Container::builder()
                                .width(150.0)
                                .height(80.0)
                                .child(Text::builder().text("Elevation: 0").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 2: Elevation 2
        Box::new(
            Container::builder()
                .height(110.0)
                .padding(EdgeInsets::all(10.0))
                .child(
                    Material::builder()
                        .elevation(2.0)
                        .color(Color::rgb(255, 255, 255))
                        .child(
                            Container::builder()
                                .width(150.0)
                                .height(80.0)
                                .child(Text::builder().text("Elevation: 2").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 3: Elevation 4 with rounded corners
        Box::new(
            Container::builder()
                .height(110.0)
                .padding(EdgeInsets::all(10.0))
                .child(
                    Material::builder()
                        .elevation(4.0)
                        .color(Color::rgb(255, 255, 255))
                        .border_radius(BorderRadius::circular(8.0))
                        .child(
                            Container::builder()
                                .width(150.0)
                                .height(80.0)
                                .child(Text::builder().text("Elevation: 4, Rounded").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 4: Elevation 8 with color
        Box::new(
            Container::builder()
                .height(120.0)
                .padding(EdgeInsets::all(10.0))
                .child(
                    Material::builder()
                        .elevation(8.0)
                        .color(Color::rgb(200, 230, 255))
                        .border_radius(BorderRadius::circular(12.0))
                        .child(
                            Container::builder()
                                .width(150.0)
                                .height(80.0)
                                .child(Text::builder().text("Elevation: 8, Blue").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 5: Elevation 16 with custom shadow
        Box::new(
            Container::builder()
                .height(130.0)
                .padding(EdgeInsets::all(10.0))
                .child(
                    Material::builder()
                        .elevation(16.0)
                        .color(Color::rgb(255, 240, 200))
                        .border_radius(BorderRadius::circular(16.0))
                        .shadow_color(Color::rgba(255, 0, 0, 100))
                        .child(
                            Container::builder()
                                .width(150.0)
                                .height(80.0)
                                .child(Text::builder().text("Elevation: 16, Red Shadow").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 6: Very rounded material (pill shape)
        Box::new(
            Container::builder()
                .height(110.0)
                .padding(EdgeInsets::all(10.0))
                .child(
                    Material::builder()
                        .elevation(4.0)
                        .color(Color::rgb(255, 200, 255))
                        .border_radius(BorderRadius::circular(40.0))
                        .child(
                            Container::builder()
                                .width(200.0)
                                .height(80.0)
                                .child(Text::builder().text("Pill Shape").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
    ])
}

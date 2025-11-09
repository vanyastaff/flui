//! Container widget demo - testing sizing, padding, margin, and color
//!
//! This demo tests the Container widget with:
//! - Fixed width and height
//! - Padding and margin
//! - Background color
//! - Combinations of all properties

use flui::prelude::*;
use flui_app::{launch, EguiAppBuilder};
use flui_widgets::{ColoredBox, Container, SizedBox, Text};

fn main() {
    println!("=== Container Widget Demo ===");
    println!("Testing Container widget with various configurations:");
    println!("1. Fixed size container");
    println!("2. Container with padding");
    println!("3. Container with margin");
    println!("4. Container with color");
    println!("5. Container with all properties");

    launch(EguiAppBuilder::new(app_view));
}

fn app_view() -> impl View {
    Column::new().children(vec![
        // Test 1: Fixed size container
        Box::new(
            ColoredBox::builder()
                .color(Color::rgb(240, 240, 240))
                .child(
                    Container::builder()
                        .width(150.0)
                        .height(80.0)
                        .child(
                            ColoredBox::builder()
                                .color(Color::rgb(255, 0, 0))
                                .child(Text::builder().text("150x80").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 2: Container with padding
        Box::new(
            ColoredBox::builder()
                .color(Color::rgb(220, 220, 220))
                .child(
                    Container::builder()
                        .padding(EdgeInsets::all(20.0))
                        .child(
                            ColoredBox::builder()
                                .color(Color::rgb(0, 255, 0))
                                .child(SizedBox::builder().width(100.0).height(60.0).build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 3: Container with margin
        Box::new(
            ColoredBox::builder()
                .color(Color::rgb(200, 200, 200))
                .child(
                    Container::builder()
                        .margin(EdgeInsets::symmetric(30.0, 15.0))
                        .child(
                            ColoredBox::builder()
                                .color(Color::rgb(0, 0, 255))
                                .child(SizedBox::builder().width(120.0).height(70.0).build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
        // Test 4: Container with color
        Box::new(
            Container::builder()
                .color(Color::rgb(255, 200, 100))
                .width(180.0)
                .height(90.0)
                .child(Text::builder().text("Colored Container").build())
                .build(),
        ),
        // Test 5: Container with all properties
        Box::new(
            ColoredBox::builder()
                .color(Color::rgb(250, 250, 250))
                .child(
                    Container::builder()
                        .width(200.0)
                        .height(100.0)
                        .padding(EdgeInsets::all(15.0))
                        .margin(EdgeInsets::symmetric(20.0, 10.0))
                        .color(Color::rgb(255, 255, 200))
                        .child(
                            ColoredBox::builder()
                                .color(Color::rgb(128, 0, 128))
                                .child(Text::builder().text("All Properties").build())
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ),
    ])
}

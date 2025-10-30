//! Demo: Interactive button with hover effect
//!
//! Demonstrates GestureDetector, Transform, Container, and Center

use flui_widgets::prelude::*;

fn main() {
    // Button with scale transform
    let _button = GestureDetector::builder()
        .on_tap(|_| {
            println!("Button clicked!");
        })
        .child(
            Transform::scale(1.05, 1.05) // Slightly scaled up
                .child(
                    Center::builder()
                        .child(
                            Container::builder()
                                .width(200.0)
                                .height(60.0)
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(52, 168, 83)), // Green
                                    border_radius: Some(BorderRadius::all(Radius::circular(30.0))),
                                    ..Default::default()
                                })
                                .child(
                                    Center::builder()
                                        .child(
                                            Text::builder()
                                                .data("Click Me!")
                                                .size(18.0)
                                                .color(Color::WHITE)
                                                .build(),
                                        )
                                        .build(),
                                )
                                .build(),
                        )
                        .build()
                        .into(),
                )
                .build()
                .into(),
        )
        .build();

    println!("Interactive button created successfully!");
    println!("Widget structure:");
    println!("  GestureDetector (handles taps)");
    println!("    └─ Transform (scale 1.05)");
    println!("       └─ Center");
    println!("          └─ Container (200x60, green, pill-shaped)");
    println!("             └─ Center");
    println!("                └─ Text ('Click Me!', 18px, white)");
}

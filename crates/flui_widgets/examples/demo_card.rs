//! Demo: Card with visual effects
//!
//! Demonstrates Container, Padding, ClipRRect, Column, Opacity, and Text

use flui_widgets::prelude::*;

fn main() {
    // Card with rounded corners and semi-transparent text
    let _card = Container::builder()
        .width(300.0)
        .height(200.0)
        .decoration(BoxDecoration {
            color: Some(Color::rgb(66, 133, 244)), // Blue
            border_radius: Some(BorderRadius::all(Radius::circular(16.0))),
            ..Default::default()
        })
        .padding(EdgeInsets::all(20.0))
        .child(
            ClipRRect::builder()
                .border_radius(BorderRadius::all(Radius::circular(16.0)))
                .child(
                    Column::builder()
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .children(vec![
                            Text::builder()
                                .data("Welcome to Flui!")
                                .size(24.0)
                                .color(Color::WHITE)
                                .build()
                                .into(),
                            Opacity::builder()
                                .opacity(0.8)
                                .child(
                                    Text::builder()
                                        .data("A modern UI framework for Rust")
                                        .size(16.0)
                                        .color(Color::WHITE)
                                        .build(),
                                )
                                .build()
                                .into(),
                        ])
                        .build(),
                )
                .build(),
        )
        .build();

    println!("Card widget created successfully!");
    println!("Widget structure:");
    println!("  Container (300x200, blue, rounded corners, padding)");
    println!("    └─ ClipRRect (rounded)");
    println!("       └─ Column");
    println!("          ├─ Text (title, 24px, white)");
    println!("          └─ Opacity (0.8)");
    println!("             └─ Text (subtitle, 16px, white)");
}

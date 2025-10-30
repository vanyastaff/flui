//! Demo: Grid-like layout using Rows and Columns
//!
//! Demonstrates nested Row/Column, Expanded, AspectRatio, and ClipRRect

use flui_widgets::prelude::*;

fn main() {
    // 2x2 grid of colored cards
    let _grid = Container::builder()
        .width(400.0)
        .padding(EdgeInsets::all(10.0))
        .color(Color::rgb(245, 245, 245)) // Light grey background
        .child(
            Column::builder()
                .main_axis_size(MainAxisSize::Min)
                .children(vec![
                    Row::builder()
                        .children(vec![
                            Expanded::builder()
                                .child(grid_item(Color::rgb(244, 67, 54), "1"))
                                .build()
                                .into(),
                            SizedBox::width(10.0).into(),
                            Expanded::builder()
                                .child(grid_item(Color::rgb(33, 150, 243), "2"))
                                .build()
                                .into(),
                        ])
                        .build()
                        .into(),
                    SizedBox::height(10.0).into(),
                    Row::builder()
                        .children(vec![
                            Expanded::builder()
                                .child(grid_item(Color::rgb(76, 175, 80), "3"))
                                .build()
                                .into(),
                            SizedBox::width(10.0).into(),
                            Expanded::builder()
                                .child(grid_item(Color::rgb(255, 152, 0), "4"))
                                .build()
                                .into(),
                        ])
                        .build()
                        .into(),
                ])
                .build(),
        )
        .build();

    println!("Grid layout created successfully!");
    println!("Widget structure:");
    println!("  Container (400px wide, light grey, padding)");
    println!("    └─ Column");
    println!("       ├─ Row (first row)");
    println!("       │  ├─ Expanded (red card)");
    println!("       │  └─ Expanded (blue card)");
    println!("       └─ Row (second row)");
    println!("          ├─ Expanded (green card)");
    println!("          └─ Expanded (orange card)");
}

fn grid_item(color: Color, number: &str) -> AspectRatio {
    AspectRatio::builder()
        .aspect_ratio(1.0) // Square aspect ratio
        .child(
            ClipRRect::builder()
                .border_radius(BorderRadius::all(Radius::circular(12.0)))
                .child(
                    Container::builder()
                        .color(color)
                        .child(
                            Center::builder()
                                .child(
                                    Text::builder()
                                        .data(number)
                                        .size(48.0)
                                        .color(Color::WHITE)
                                        .build(),
                                )
                                .build(),
                        )
                        .build(),
                )
                .build(),
        )
        .build()
}

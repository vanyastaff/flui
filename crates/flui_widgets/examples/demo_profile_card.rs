//! Demo: Simple card layout
//!
//! Demonstrates Column, Row, Container, ClipOval, and SizedBox

use flui_widgets::prelude::*;

fn main() {
    // Simple info card with avatar
    let _card = Container::builder()
        .width(300.0)
        .padding(EdgeInsets::all(20.0))
        .decoration(BoxDecoration {
            color: Some(Color::WHITE),
            border_radius: Some(BorderRadius::all(Radius::circular(16.0))),
            ..Default::default()
        })
        .child(
            Column::builder()
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(vec![
                    // Avatar
                    ClipOval::builder()
                        .child(
                            Container::builder()
                                .width(80.0)
                                .height(80.0)
                                .color(Color::rgb(103, 58, 183)) // Purple
                                .child(
                                    Center::builder()
                                        .child(
                                            Text::builder()
                                                .data("JS")
                                                .size(32.0)
                                                .color(Color::WHITE)
                                                .build(),
                                        )
                                        .build(),
                                )
                                .build(),
                        )
                        .build()
                        .into(),
                    SizedBox::height(16.0).into(),
                    // Name
                    Text::builder()
                        .data("John Smith")
                        .size(24.0)
                        .color(Color::rgb(33, 33, 33))
                        .build()
                        .into(),
                    SizedBox::height(8.0).into(),
                    // Title
                    Opacity::builder()
                        .opacity(0.7)
                        .child(
                            Text::builder()
                                .data("UI/UX Designer")
                                .size(16.0)
                                .color(Color::rgb(66, 66, 66))
                                .build(),
                        )
                        .build()
                        .into(),
                    SizedBox::height(20.0).into(),
                    // Stats
                    Row::builder()
                        .main_axis_alignment(MainAxisAlignment::SpaceAround)
                        .children(vec![
                            stat_item("128", "Posts").into(),
                            stat_item("2.5K", "Followers").into(),
                            stat_item("892", "Following").into(),
                        ])
                        .build()
                        .into(),
                ])
                .build(),
        )
        .build();

    println!("Info card created successfully!");
    println!("Widget structure:");
    println!("  Container (300px wide, white, rounded, padding)");
    println!("    └─ Column (centered)");
    println!("       ├─ ClipOval (avatar, purple, 80x80)");
    println!("       ├─ Text (name, 24px)");
    println!("       ├─ Opacity + Text (title, 16px)");
    println!("       └─ Row (stats)");
    println!("          ├─ Posts");
    println!("          ├─ Followers");
    println!("          └─ Following");
}

fn stat_item(value: &str, label: &str) -> Column {
    Column::builder()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(vec![
            Text::builder()
                .data(value)
                .size(20.0)
                .color(Color::rgb(33, 33, 33))
                .build()
                .into(),
            SizedBox::height(4.0).into(),
            Opacity::builder()
                .opacity(0.6)
                .child(
                    Text::builder()
                        .data(label)
                        .size(12.0)
                        .color(Color::rgb(117, 117, 117))
                        .build(),
                )
                .build()
                .into(),
        ])
        .build()
}

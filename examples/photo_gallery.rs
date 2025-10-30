//! Photo Gallery Example
//!
//! Demonstrates building a responsive photo gallery using:
//! - Wrap widget for responsive grid layout
//! - AspectRatio to maintain image proportions
//! - ClipRRect for rounded corners
//! - Stack for overlay effects
//! - Container for styling

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

/// Photo gallery application
#[derive(Debug, Clone)]
struct PhotoGalleryApp;

flui_core::impl_into_widget!(PhotoGalleryApp, stateless);

impl StatelessWidget for PhotoGalleryApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(250, 250, 250))
            .child(
                Column::builder()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(vec![
                        // Header
                        build_gallery_header(),

                        SizedBox::builder()
                            .height(24.0)
                            .build()
                            .into(),

                        // Photo Grid using Wrap
                        Wrap::builder()
                            .spacing(16.0)
                            .run_spacing(16.0)
                            .children(vec![
                                build_photo_card("Sunset", "Landscape", Color::rgb(255, 87, 34)),
                                build_photo_card("Mountains", "Nature", Color::rgb(63, 81, 181)),
                                build_photo_card("Ocean", "Seascape", Color::rgb(0, 150, 136)),
                                build_photo_card("Forest", "Nature", Color::rgb(76, 175, 80)),
                                build_photo_card("City", "Urban", Color::rgb(156, 39, 176)),
                                build_photo_card("Desert", "Landscape", Color::rgb(255, 152, 0)),
                                build_photo_card("Northern Lights", "Sky", Color::rgb(103, 58, 183)),
                                build_photo_card("Waterfall", "Nature", Color::rgb(3, 169, 244)),
                            ])
                            .build()
                            .into(),
                    ])
                    .build()
            )
            .build()
    }
}

/// Build gallery header with title and filters
fn build_gallery_header() -> Widget {
    Column::builder()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(vec![
            Text::builder()
                .data("Photo Gallery")
                .size(32.0)
                .color(Color::rgb(33, 33, 33))
                .build()
                .into(),
            SizedBox::builder().height(8.0).build().into(),
            Text::builder()
                .data("Explore beautiful landscapes and nature photography")
                .size(16.0)
                .color(Color::rgb(117, 117, 117))
                .build()
                .into(),
            SizedBox::builder().height(16.0).build().into(),

            // Filter buttons
            Row::builder()
                .main_axis_alignment(MainAxisAlignment::Start)
                .children(vec![
                    build_filter_chip("All", true),
                    SizedBox::builder().width(8.0).build().into(),
                    build_filter_chip("Landscape", false),
                    SizedBox::builder().width(8.0).build().into(),
                    build_filter_chip("Nature", false),
                    SizedBox::builder().width(8.0).build().into(),
                    build_filter_chip("Urban", false),
                ])
                .build()
                .into(),
        ])
        .build()
}

/// Build a filter chip button
fn build_filter_chip(label: &str, active: bool) -> Widget {
    Container::builder()
        .padding(EdgeInsets::symmetric(8.0, 16.0))
        .decoration(BoxDecoration {
            color: Some(if active {
                Color::rgb(33, 150, 243)
            } else {
                Color::rgb(238, 238, 238)
            }),
            border_radius: Some(BorderRadius::circular(20.0)),
            ..Default::default()
        })
        .child(
            Text::builder()
                .data(label)
                .size(14.0)
                .color(if active {
                    Color::WHITE
                } else {
                    Color::rgb(97, 97, 97)
                })
                .build()
        )
        .build()
}

/// Build a photo card with AspectRatio and ClipRRect
fn build_photo_card(title: &str, category: &str, color: Color) -> Widget {
    SizedBox::builder()
        .width(200.0)
        .child(
            Column::builder()
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(vec![
                    // Photo with rounded corners
                    ClipRRect::builder()
                        .border_radius(BorderRadius::circular(12.0))
                        .child(
                            AspectRatio::builder()
                                .aspect_ratio(4.0 / 3.0)
                                .child(
                                    Stack::builder()
                                        .children(vec![
                                            // Photo background (simulated with color)
                                            Positioned::builder()
                                                .top(0.0)
                                                .left(0.0)
                                                .right(0.0)
                                                .bottom(0.0)
                                                .child(
                                                    Container::builder()
                                                        .color(color)
                                                        .build()
                                                )
                                                .build()
                                                .into(),

                                            // Gradient overlay at bottom
                                            Positioned::builder()
                                                .left(0.0)
                                                .right(0.0)
                                                .bottom(0.0)
                                                .child(
                                                    Container::builder()
                                                        .height(60.0)
                                                        .decoration(BoxDecoration {
                                                            color: Some(Color::rgba(0, 0, 0, 0.4)),
                                                            ..Default::default()
                                                        })
                                                        .build()
                                                )
                                                .build()
                                                .into(),

                                            // Photo title overlay
                                            Positioned::builder()
                                                .left(12.0)
                                                .right(12.0)
                                                .bottom(12.0)
                                                .child(
                                                    Text::builder()
                                                        .data(title)
                                                        .size(16.0)
                                                        .color(Color::WHITE)
                                                        .build()
                                                )
                                                .build()
                                                .into(),
                                        ])
                                        .build()
                                )
                                .build()
                        )
                        .build()
                        .into(),

                    SizedBox::builder().height(8.0).build().into(),

                    // Category label
                    Container::builder()
                        .padding(EdgeInsets::symmetric(4.0, 8.0))
                        .decoration(BoxDecoration {
                            color: Some(color.with_opacity(0.1)),
                            border_radius: Some(BorderRadius::circular(4.0)),
                            ..Default::default()
                        })
                        .child(
                            Text::builder()
                                .data(category)
                                .size(12.0)
                                .color(color)
                                .build()
                        )
                        .build()
                        .into(),
                ])
                .build()
        )
        .build()
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Photo Gallery Example ===");
    println!("Demonstrates:");
    println!("  • Wrap widget for responsive grid");
    println!("  • AspectRatio to maintain proportions");
    println!("  • ClipRRect for rounded corners");
    println!("  • Stack for layered overlay effects");
    println!("  • Positioned for absolute positioning");
    println!("  • Filter chips with active states");
    println!();

    run_app(PhotoGalleryApp.into_widget())
}

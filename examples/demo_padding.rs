//! Padding widget demo - testing padding with nested colored boxes
//!
//! This demo shows padding effect with visible borders

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Center, ColoredBox, Container, Padding, SizedBox};

#[derive(Debug, Clone)]
struct PaddingDemoApp;

impl View for PaddingDemoApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Outer gray box
        let mut gray_box = ColoredBox::builder()
            .color(Color::rgb(200, 200, 200))
            .build();

        // Padding widget (20px all sides)
        let mut padding = Padding::builder().padding(EdgeInsets::all(20.0)).build();

        // Inner red box
        let mut red_box = ColoredBox::builder().color(Color::rgb(255, 0, 0)).build();

        red_box.child = Some(Box::new(
            SizedBox::builder().width(100.0).height(100.0).build(),
        ));

        padding.child = Some(Box::new(red_box));
        gray_box.child = Some(Box::new(padding));

        // Center everything
        let mut center = Center::builder().build();
        center.child = Some(Box::new(gray_box));

        center
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Padding Widget Demo ===");
    println!("You should see:");
    println!("- Gray outer box");
    println!("- 20px padding (visible as gray border)");
    println!("- Red inner box 100x100");

    run_app(Box::new(PaddingDemoApp))
}

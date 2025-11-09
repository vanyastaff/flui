//! Align widget demo - testing alignment positions
//!
//! This demo shows a blue box aligned to bottom-right

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::Color;
use flui_widgets::{Align, Alignment, ColoredBox, SizedBox};

#[derive(Debug, Clone)]
struct AlignDemoApp;

impl View for AlignDemoApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create background
        let mut background = ColoredBox::builder()
            .color(Color::rgb(240, 240, 240))
            .build();

        // Align widget with bottom-right alignment
        let mut align = Align::builder().alignment(Alignment::BOTTOM_RIGHT).build();

        // Blue box to be aligned
        let mut blue_box = ColoredBox::builder().color(Color::rgb(0, 0, 255)).build();

        blue_box.child = Some(Box::new(
            SizedBox::builder().width(80.0).height(80.0).build(),
        ));

        align.child = Some(Box::new(blue_box));
        background.child = Some(Box::new(align));

        background
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Align Widget Demo ===");
    println!("You should see:");
    println!("- Gray background");
    println!("- Blue 80x80 box aligned to BOTTOM-RIGHT corner");

    run_app(Box::new(AlignDemoApp))
}

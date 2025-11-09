//! Center widget demo - testing positioning and alignment
//!
//! This demo tests the Center widget with a colored box

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::Color;
use flui_widgets::{Center, ColoredBox, SizedBox};

#[derive(Debug, Clone)]
struct CenterDemoApp;

impl View for CenterDemoApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Test Center widget with a red box
        let mut center = Center::builder().build();

        let mut colored_box = ColoredBox::builder().color(Color::rgb(255, 0, 0)).build();

        colored_box.child = Some(Box::new(
            SizedBox::builder().width(100.0).height(100.0).build(),
        ));

        center.child = Some(Box::new(colored_box));
        center
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Center Widget Demo ===");
    println!("You should see: A red 100x100 box centered in the window");

    run_app(Box::new(CenterDemoApp))
}

//! Hello World - High-Level Widget Example
//!
//! This is the simplest possible Flui app using the Widget API.
//! It demonstrates the high-level approach to building UIs.
//!
//! Compare this to low-level examples that use Painter directly!

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

/// Our root application widget
#[derive(Debug, Clone)]
struct HelloWorldApp;

// Implement IntoWidget for HelloWorldApp
flui_core::impl_into_widget!(HelloWorldApp, stateless);

impl StatelessWidget for HelloWorldApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(245, 245, 245))
            .child(
                Center::builder()
                    .child(
                        Container::builder()
                            .padding(EdgeInsets::all(24.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(66, 165, 245)),
                                border_radius: Some(BorderRadius::circular(12.0)),
                                ..Default::default()
                            })
                            .child(
                                Text::builder()
                                    .data("Hello, Flui!")
                                    .size(32.0)
                                    .color(Color::WHITE)
                                    .build(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .build()
    }
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Flui Widget Hello World ===");
    println!("High-level Widget API example with ergonomic IntoWidget trait");
    println!();
    println!("Architecture:");
    println!("  HelloWorldApp (StatelessWidget)");
    println!("    → build() creates widget tree");
    println!("    → Container → Center → Container → Text");
    println!();
    println!("This uses the full Widget → RenderObject → Layer pipeline!");
    println!("No more verbose Widget::stateless() or Widget::render_object() wrappers!");
    println!();

    run_app(HelloWorldApp.into_widget())
}

//! Hello World - High-Level Widget Example
//!
//! This is the simplest possible Flui app using the Widget API.
//! It demonstrates the high-level approach to building UIs.
//!
//! Compare this to low-level examples that use Painter directly!

use flui_app::{BuildContext, StatelessWidget, run_app};
use flui_core::{DynWidget, Widget};
use flui_widgets::prelude::*;

/// Our root application widget
#[derive(Debug, Clone)]
struct HelloWorldApp;

impl StatelessWidget for HelloWorldApp {
    fn build(&self, _ctx: &BuildContext) -> Box<dyn DynWidget> {
        Box::new(
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
                                    Text::new("Hello, Flui!")
                                        .font_size(32.0)
                                        .color(Color::WHITE),
                                )
                                .build(),
                        )
                        .build(),
                )
                .build(),
        )
    }
}

impl Widget for HelloWorldApp {
    // StatelessWidget provides default implementation
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Flui Widget Hello World ===");
    println!("High-level Widget API example");
    println!();
    println!("Architecture:");
    println!("  HelloWorldApp (StatelessWidget)");
    println!("    → build() creates widget tree");
    println!("    → Container → Center → Container → Text");
    println!();
    println!("This uses the full Widget → RenderObject → Layer pipeline!");
    println!();

    run_app(Box::new(HelloWorldApp))
}

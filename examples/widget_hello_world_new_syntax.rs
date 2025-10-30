//! Hello World - DEMO of New Ergonomic API
//!
//! This demonstrates the improved syntax with IntoWidget!
//! Compare this to widget_hello_world.rs to see the difference!

use flui_app::run_app;
use flui_core::{BuildContext, StatelessWidget, Widget};
use flui_widgets::prelude::*;

/// Our root application widget
#[derive(Debug, Clone)]
struct HelloWorldApp;

impl StatelessWidget for HelloWorldApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        // ✨ NEW ERGONOMIC SYNTAX ✨
        // No more Widget::stateless() or Widget::render_object() wrappers!
        Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(245, 245, 245))
            .child(
                // child() accepts impl IntoWidget - no wrapping needed!
                Widget::render_object(
                    Center::builder()
                        .child(
                            // Nested Container - just pass it directly!
                            Container::builder()
                                .padding(EdgeInsets::all(24.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(66, 165, 245)),
                                    border_radius: Some(BorderRadius::circular(12.0)),
                                    ..Default::default()
                                })
                                .child(Widget::render_object(
                                    Text::builder()
                                        .data("Hello, Flui!")
                                        .size(32.0)
                                        .color(Color::WHITE)
                                        .build(),
                                ))
                                .build(), // build() returns Widget!
                        )
                        .build(),
                ),
            )
            .build() // build() returns Widget!
    }
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Flui Widget Hello World (New Syntax) ===");
    println!("Demonstrating IntoWidget trait");
    println!();
    println!("Benefits:");
    println!("  ✅ Container doesn't need Widget::stateless() wrapper");
    println!("  ✅ Can use .into() for explicit conversion");
    println!("  ✅ Or pass directly if function accepts impl Into<Widget>");
    println!();

    run_app(Widget::stateless(HelloWorldApp))
}
